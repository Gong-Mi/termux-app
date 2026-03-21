use jni::objects::JValue;
use std::cmp::{max, min};

use crate::vte_parser::{Params, Parser, Perform};
pub use crate::terminal::style::*;
pub use crate::terminal::modes::*;
pub use crate::terminal::colors::*;
pub use crate::terminal::sixel::{SixelDecoder, SixelState};

use crate::terminal::{
    screen::{Screen},
    cursor::{Cursor},
};

/// Base64 解码辅助函数

// -----------------------------------------------------------------------------
// DirectByteBuffer 零拷贝支持
// -----------------------------------------------------------------------------

#[repr(C)]
pub struct SharedScreenBuffer {
    pub version: u32,
    pub cols: u32,
    pub rows: u32,
    pub style_offset: u32,
    pub text_data: [u16; 0],
}

impl SharedScreenBuffer {
    pub fn required_size(cols: usize, rows: usize) -> usize {
        let header_size = 16;
        let text_size = cols * rows * 2;
        let aligned_text_size = (text_size + 7) & !7;
        let style_size = cols * rows * 8;
        header_size + aligned_text_size + style_size
    }

    pub fn style_data_ptr(&self) -> *const u64 {
        let cell_count = self.cols as usize * self.rows as usize;
        let text_size = cell_count * 2;
        let aligned_text_size = (text_size + 7) & !7;
        unsafe { (self.text_data.as_ptr() as *const u8).add(aligned_text_size) as *const u64 }
    }
}

pub struct FlatScreenBuffer {
    pub text_data: Vec<u16>,
    pub style_data: Vec<u64>,
    pub cols: usize,
    pub rows: usize,
}

impl FlatScreenBuffer {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cell_count = cols * rows;
        Self {
            text_data: vec![0u16; cell_count],
            style_data: vec![0u64; cell_count],
            cols,
            rows,
        }
    }

    pub fn create_shared_buffer(&self) -> *mut SharedScreenBuffer {
        let size = SharedScreenBuffer::required_size(self.cols, self.rows);
        unsafe {
            let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
            let ptr = std::alloc::alloc(layout) as *mut SharedScreenBuffer;
            if !ptr.is_null() {
                (*ptr).version = 0;
                (*ptr).cols = self.cols as u32;
                (*ptr).rows = self.rows as u32;
            }
            ptr
        }
    }

    pub fn cell_index(&self, col: usize, row: usize) -> usize {
        row * self.cols + col
    }

    pub fn get_cell(&self, col: usize, row: usize) -> (u16, u64) {
        let idx = self.cell_index(col, row);
        (self.text_data[idx], self.style_data[idx])
    }

    pub unsafe fn sync_to_shared(&self, shared_ptr: *mut SharedScreenBuffer) {
        if shared_ptr.is_null() { return; }
        unsafe {
            let base_ptr = shared_ptr as *mut u8;
            std::ptr::write(base_ptr.add(4) as *mut u32, self.cols as u32);
            std::ptr::write(base_ptr.add(8) as *mut u32, self.rows as u32);
            let text_size = (self.cols * self.rows * 2) as usize;
            let aligned_text_size = (text_size + 7) & !7;
            let style_offset = (16 + aligned_text_size) as u32;
            std::ptr::write(base_ptr.add(12) as *mut u32, style_offset);

            let cell_count = self.cols * self.rows;
            if cell_count > 0 {
                std::ptr::copy_nonoverlapping(self.text_data.as_ptr(), base_ptr.add(16) as *mut u16, cell_count);
                std::ptr::copy_nonoverlapping(self.style_data.as_ptr(), base_ptr.add(style_offset as usize) as *mut u64, cell_count);
            }
            let version_ptr = base_ptr.add(0) as *mut u32;
            let old_version = std::ptr::read(version_ptr);
            std::ptr::write(version_ptr, old_version.wrapping_add(1));
        }
    }
}

pub struct TerminalContext {
    pub engine: TerminalEngine,
}

// -----------------------------------------------------------------------------
// ScreenState
// -----------------------------------------------------------------------------

pub struct ScreenState {
    pub rows: i32,
    pub cols: i32,
    
    // 缓冲区对
    pub main_screen: Screen,
    pub alt_screen: Screen,
    pub use_alternate_buffer: bool,

    pub cursor: Cursor,
    pub modes: TerminalModes,
    pub colors: TerminalColors,
    pub sixel_decoder: SixelDecoder,
    pub flat_buffer: Option<FlatScreenBuffer>,
    pub shared_buffer_ptr: *mut SharedScreenBuffer,
    pub top_margin: i32,
    pub bottom_margin: i32,
    pub left_margin: i32,
    pub right_margin: i32,
    pub current_style: u64,
    pub tab_stops: Vec<bool>,
    pub title: Option<String>,
    pub title_stack: Vec<String>,
    pub use_line_drawing_g0: bool,
    pub use_line_drawing_g1: bool,
    pub use_line_drawing_uses_g0: bool,
    pub scroll_counter: i32,
    pub java_callback_obj: Option<jni::objects::GlobalRef>,

    // 辅助状态
    pub last_printed_char: Option<char>,
    pub fore_color: u64,
    pub back_color: u64,
    pub effect: u64,
    pub cursor_enabled: bool,
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub send_focus_events: bool,
    pub mouse_tracking: bool,
    pub mouse_button_event: bool,
    pub sgr_mouse: bool,
    pub auto_scroll_disabled: bool,
    pub underline_color: u64,
}

impl ScreenState {
    pub fn new(cols: i32, rows: i32, total_rows: i32, _cw: i32, _ch: i32) -> Self {
        let mut tab_stops = vec![false; cols as usize];
        for i in (8..cols as usize).step_by(8) { tab_stops[i] = true; }

        let mut modes = TerminalModes::new();
        modes.set(DECSET_BIT_AUTOWRAP);

        Self {
            rows, cols,
            main_screen: Screen::new(cols, rows, total_rows),
            alt_screen: Screen::new(cols, rows, rows), // 备用屏不需要滚动历史
            use_alternate_buffer: false,
            cursor: Cursor::new(),
            modes,
            colors: TerminalColors::new(),
            sixel_decoder: SixelDecoder::new(),
            flat_buffer: Some(FlatScreenBuffer::new(cols as usize, max(rows, total_rows) as usize)),
            shared_buffer_ptr: std::ptr::null_mut(),
            top_margin: 0,
            bottom_margin: rows,
            left_margin: 0,
            right_margin: cols,
            current_style: STYLE_NORMAL,
            tab_stops,
            title: None,
            title_stack: Vec::new(),
            use_line_drawing_g0: false,
            use_line_drawing_g1: false,
            use_line_drawing_uses_g0: true,
            scroll_counter: 0,
            java_callback_obj: None,
            last_printed_char: None,
            fore_color: COLOR_INDEX_FOREGROUND,
            back_color: COLOR_INDEX_BACKGROUND,
            effect: 0,
            cursor_enabled: true,
            application_cursor_keys: false,
            bracketed_paste: false,
            send_focus_events: false,
            mouse_tracking: false,
            mouse_button_event: false,
            sgr_mouse: false,
            auto_scroll_disabled: false,
            underline_color: COLOR_INDEX_FOREGROUND,
        }
    }


    #[inline]
    pub fn get_current_screen(&self) -> &Screen {
        if self.use_alternate_buffer { &self.alt_screen } else { &self.main_screen }
    }

    #[inline]
    pub fn get_current_screen_mut(&mut self) -> &mut Screen {
        if self.use_alternate_buffer { &mut self.alt_screen } else { &mut self.main_screen }
    }

    pub fn auto_wrap(&self) -> bool { self.modes.is_enabled(DECSET_BIT_AUTOWRAP) }
    pub fn origin_mode(&self) -> bool { self.modes.is_enabled(DECSET_BIT_ORIGIN_MODE) }
    pub fn leftright_margin_mode(&self) -> bool { self.modes.is_enabled(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) }

    pub fn screen_first_row(&self) -> usize { self.get_current_screen().first_row }
    pub fn saved_decset_flags(&self) -> i32 { self.cursor.saved_state.decset_flags }
    pub fn decset_flags(&self) -> i32 { self.modes.flags }

    /// 执行 DECSET/DECRST 命令（设置/重置 DEC 私有模式）
    pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
        match mode {
            1 => { // Application Cursor Keys (DECCKM)
                if setting { self.modes.set(DECSET_BIT_APPLICATION_CURSOR_KEYS); }
                else { self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS); }
            }
            3 => { // 132 column mode (DECCOLM)
                // 清除滚动边距并重置光标位置
                let rows = self.rows as usize;
                let cols = self.cols as usize;
                let style = self.current_style;
                self.top_margin = 0;
                self.bottom_margin = self.rows;
                self.left_margin = 0;
                self.right_margin = self.cols;
                self.modes.reset(DECSET_BIT_LEFTRIGHT_MARGIN_MODE);
                // 清屏并重置光标
                self.get_current_screen_mut().block_clear(0, 0, rows, cols, style);
                self.cursor.x = 0;
                self.cursor.y = 0;
            }
            4 => { // DECSCLM-Scrolling Mode - 忽略
            }
            5 => { // Reverse video
                if setting { self.modes.set(DECSET_BIT_REVERSE_VIDEO); }
                else { self.modes.reset(DECSET_BIT_REVERSE_VIDEO); }
            }
            6 => { // Origin Mode (DECOM)
                if setting { self.modes.set(DECSET_BIT_ORIGIN_MODE); self.cursor.x = 0; self.cursor.y = 0; }
                else { self.modes.reset(DECSET_BIT_ORIGIN_MODE); }
            }
            7 => { // Auto-wrap (DECAWM)
                if setting { self.modes.set(DECSET_BIT_AUTOWRAP); }
                else { self.modes.reset(DECSET_BIT_AUTOWRAP); }
            }
            8 => { // Auto-repeat Keys (DECARM) - 不实现
            }
            9 => { // X10 mouse - 不实现
            }
            12 => { // Control cursor blinking - 忽略
            }
            25 => { // Show/hide cursor
                if setting { self.cursor_enabled = true; }
                else { self.cursor_enabled = false; }
            }
            40 => { // Allow 80 => 132 Mode - 忽略
            }
            45 => { // Reverse wrap-around - 忽略
            }
            66 => { // Application keypad (DECNKM)
                if setting { self.modes.set(DECSET_BIT_APPLICATION_KEYPAD); }
                else { self.modes.reset(DECSET_BIT_APPLICATION_KEYPAD); }
            }
            69 => { // Left and right margin mode (DECLRMM)
                if !setting { self.left_margin = 0; self.right_margin = self.cols; }
            }
            1000 | 1001 | 1002 | 1003 | 1004 | 1005 => { // Mouse tracking - 忽略
            }
            1006 => { // SGR Mouse Mode
                if setting { self.sgr_mouse = true; }
                else { self.sgr_mouse = false; }
            }
            1015 => { // URXVT mouse - 忽略
            }
            1034 => { // Interpret "meta" key - 忽略
            }
            1048 => { // Save/restore cursor
                if setting { self.save_cursor(); }
                else { self.restore_cursor(); }
            }
            47 | 1047 | 1049 => { // Alternate screen buffer
                if setting {
                    self.use_alternate_buffer = true;
                    self.save_cursor();
                } else {
                    self.use_alternate_buffer = false;
                    self.restore_cursor();
                }
            }
            2004 => { // Bracketed paste mode
                if setting { self.bracketed_paste = true; }
                else { self.bracketed_paste = false; }
            }
            _ => { /* 未知模式 - 忽略 */ }
        }
    }

    pub fn scroll_up(&mut self) {
        let style = self.current_style;
        let top = self.top_margin;
        let bottom = self.bottom_margin;
        self.get_current_screen_mut().scroll_up(top, bottom, style);
        if !self.use_alternate_buffer && !self.auto_scroll_disabled { self.scroll_counter += 1; }
    }

    pub fn sync_screen_to_flat_buffer(&mut self) {
        let cols = self.cols as usize;
        let use_alt = self.use_alternate_buffer;
        let screen = if use_alt { &self.alt_screen } else { &self.main_screen };
        let rows_in_buffer = screen.rows as usize;
        
        if let Some(flat) = &mut self.flat_buffer {
            // 使用 get_row 而不是直接访问 buffer，以正确处理 first_row 偏移
            for r in 0..rows_in_buffer {
                let row_data = screen.get_row(r as i32);
                for c in 0..cols {
                    let idx = r * cols + c;
                    if c < row_data.text.len() {
                        flat.text_data[idx] = row_data.text[c] as u16;
                        flat.style_data[idx] = row_data.styles[c];
                    }
                }
            }
        }
    }

    pub fn scroll_up_lines(&mut self, n: i32) { for _ in 0..n { self.scroll_up(); } }
    pub fn scroll_down_lines(&mut self, n: i32) {
        let old_y = self.cursor.y;
        self.cursor.y = self.top_margin;
        self.insert_lines(n);
        self.cursor.y = old_y;
    }

    pub fn set_title(&mut self, title: &str) { self.title = Some(title.to_string()); self.report_title_change(title); }

    pub fn push_title(&mut self, _opcode: &str) {
        let t = self.title.clone().unwrap_or_default();
        self.title_stack.push(t);
    }
    
    pub fn pop_title(&mut self, _opcode: &str) {
        if let Some(title) = self.title_stack.pop() {
            let t = title.clone();
            self.set_title(&t);
        }
    }

    pub fn report_title_change(&self, title: &str) {
        if let Some(obj) = &self.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    if let Ok(java_title) = env.new_string(title) {
                        let _ = env.call_method(obj.as_obj(), "reportTitleChange", "(Ljava/lang/String;)V", &[JValue::Object(&java_title.into())]);
                    }
                }
            }
        }
    }

    pub fn report_terminal_response(&self, response: &str) {
        if let Some(obj) = &self.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    if let Ok(java_response) = env.new_string(response) {
                        let _ = env.call_method(obj.as_obj(), "write", "(Ljava/lang/String;)V", &[JValue::Object(&java_response.into())]);
                    }
                }
            }
        }
    }

    pub fn send_mouse_event(&mut self, button: u32, column: i32, row: i32, pressed: bool) {
        if self.modes.is_enabled(DECSET_BIT_MOUSE_PROTOCOL_SGR) {
            let suffix = if pressed { 'M' } else { 'm' };
            let response = format!("\x1b[<{};{};{}{}", button, column + 1, row + 1, suffix);
            self.report_terminal_response(&response);
        }
    }

    pub fn cursor_forward_tab(&mut self) {
        let mut new_col = self.cursor.x + 1;
        while new_col < self.cols && !self.tab_stops.get(new_col as usize).copied().unwrap_or(false) { new_col += 1; }
        self.cursor.x = min(self.right_margin - 1, max(self.left_margin, new_col));
    }
    
    pub fn resize(&mut self, cols: i32, rows: i32) {
        self.main_screen.resize_with_reflow(cols, rows);
        self.alt_screen = Screen::new(cols, rows, rows);
        self.cols = cols; self.rows = rows;
        self.flat_buffer = Some(FlatScreenBuffer::new(cols as usize, self.main_screen.buffer.len()));
        self.cursor.clamp(cols, rows);
        self.sync_screen_to_flat_buffer();
    }

    pub fn insert_characters(&mut self, n: i32) {
        let y = self.cursor.y;
        let style = self.current_style;
        let x = self.cursor.x;
        self.get_current_screen_mut().get_row_mut(y).insert_spaces(x as usize, n as usize, style);
    }

    pub fn erase_in_display(&mut self, mode: i32) {
        let y = self.cursor.y;
        let style = self.current_style;
        self.get_current_screen_mut().erase_in_display(mode, y, style);
        if mode == 3 { self.scroll_counter = 0; }
    }

    pub fn erase_in_line(&mut self, mode: i32) {
        let cols = self.cols as usize;
        let x = self.cursor.x as usize;
        let y = self.cursor.y;
        let style = self.current_style;
        let row = self.get_current_screen_mut().get_row_mut(y);
        match mode {
            0 => row.clear(x, cols, style),
            1 => row.clear(0, x + 1, style),
            2 => row.clear(0, cols, style),
            _ => {}
        }
    }

    pub fn insert_lines(&mut self, n: i32) {
        let y = self.cursor.y;
        let bm = self.bottom_margin;
        let style = self.current_style;
        self.get_current_screen_mut().insert_lines(y, bm, n, style);
    }

    pub fn delete_lines(&mut self, n: i32) {
        let y = self.cursor.y;
        let bm = self.bottom_margin;
        let style = self.current_style;
        self.get_current_screen_mut().delete_lines(y, bm, n, style);
    }

    pub fn delete_characters(&mut self, n: i32) {
        let x = self.cursor.x as usize;
        let y = self.cursor.y;
        let style = self.current_style;
        self.get_current_screen_mut().get_row_mut(y).delete_characters(x, n as usize, style);
    }

    pub fn erase_characters(&mut self, n: i32) {
        let x = self.cursor.x as usize;
        let y = self.cursor.y;
        let style = self.current_style;
        self.get_current_screen_mut().get_row_mut(y).clear(x, x + n as usize, style);
    }

    pub fn set_margins(&mut self, top: i32, bottom: i32) {
        self.top_margin = max(0, min(top - 1, self.rows - 1));
        self.bottom_margin = max(self.top_margin + 1, min(bottom, self.rows));
    }

    pub fn handle_sgr(&mut self, params: &Params) {
        if params.len == 0 {
            self.current_style = STYLE_NORMAL; self.fore_color = COLOR_INDEX_FOREGROUND;
            self.back_color = COLOR_INDEX_BACKGROUND; self.effect = 0; return;
        }
        
        let mut i = 0;
        while i < params.len {
            let p = params.values[i];
            match p {
                0 => { self.fore_color = COLOR_INDEX_FOREGROUND; self.back_color = COLOR_INDEX_BACKGROUND; self.effect = 0; }
                1 => self.effect |= EFFECT_BOLD,
                2 => self.effect |= EFFECT_DIM,
                3 => self.effect |= EFFECT_ITALIC,
                4 => self.effect |= EFFECT_UNDERLINE,
                7 => self.effect |= EFFECT_REVERSE,
                30..=37 => self.fore_color = (p - 30) as u64,
                38 => {
                    if i + 2 < params.len && params.values[i+1] == 5 { self.fore_color = params.values[i+2] as u64; i += 2; }
                    else if i + 4 < params.len && params.values[i+1] == 2 {
                        let r = params.values[i+2] as u32; let g = params.values[i+3] as u32; let b = params.values[i+4] as u32;
                        self.fore_color = (0xff000000 | (r << 16) | (g << 8) | b) as u64; i += 4;
                    }
                }
                39 => self.fore_color = COLOR_INDEX_FOREGROUND,
                40..=47 => self.back_color = (p - 40) as u64,
                48 => {
                    if i + 2 < params.len && params.values[i+1] == 5 { self.back_color = params.values[i+2] as u64; i += 2; }
                    else if i + 4 < params.len && params.values[i+1] == 2 {
                        let r = params.values[i+2] as u32; let g = params.values[i+3] as u32; let b = params.values[i+4] as u32;
                        self.back_color = (0xff000000 | (r << 16) | (g << 8) | b) as u64; i += 4;
                    }
                }
                49 => self.back_color = COLOR_INDEX_BACKGROUND,
                58 => {
                    if i + 2 < params.len && params.values[i+1] == 5 { self.underline_color = params.values[i+2] as u64; i += 2; }
                    else if i + 4 < params.len && params.values[i+1] == 2 {
                        let r = params.values[i+2] as u32; let g = params.values[i+3] as u32; let b = params.values[i+4] as u32;
                        self.underline_color = (0xff000000 | (r << 16) | (g << 8) | b) as u64; i += 4;
                    }
                }
                59 => self.underline_color = COLOR_INDEX_FOREGROUND,
                90..=97 => self.fore_color = (p - 90 + 8) as u64,
                100..=107 => self.back_color = (p - 100 + 8) as u64,
                _ => {}
            }
            i += 1;
        }
        self.current_style = encode_style(self.fore_color, self.back_color, self.effect);
    }

    pub fn handle_decset(&mut self, params: &Params, set: bool) {
println!("handle_decset called with set={}, len={}", set, params.len);
for (i, v) in params.values[..params.len].iter().enumerate() { println!("param {}: {}", i, v); }
        for param in params.iter() {
            for &p in param.iter() {
                match p {
                    1 => {
                        if set { self.modes.set(DECSET_BIT_APPLICATION_CURSOR_KEYS) } else { self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS) }
                        self.application_cursor_keys = set;
                    },
                    6 => if set { self.modes.set(DECSET_BIT_ORIGIN_MODE) } else { self.modes.reset(DECSET_BIT_ORIGIN_MODE) },
                    7 => if set { self.modes.set(DECSET_BIT_AUTOWRAP) } else { self.modes.reset(DECSET_BIT_AUTOWRAP) },
                    25 => self.cursor_enabled = set,
                    69 => if set { self.modes.set(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) } else { self.modes.reset(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) },
                    1000 => {
                        if set { 
                            self.modes.set(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE);
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT);
                            self.mouse_tracking = true;
                            self.mouse_button_event = false;
                        } else { 
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE);
                            self.mouse_tracking = false;
                        }
                    },
                    1002 => {
                        if set { 
                            self.modes.set(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT);
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE);
                            self.mouse_button_event = true;
                            self.mouse_tracking = false;
                        } else { 
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT);
                            self.mouse_button_event = false;
                        }
                    },
                    1004 => {
                        if set { self.modes.set(DECSET_BIT_SEND_FOCUS_EVENTS) } else { self.modes.reset(DECSET_BIT_SEND_FOCUS_EVENTS) }
                        self.send_focus_events = set;
                    },
                    1006 => {
                        if set { self.modes.set(DECSET_BIT_MOUSE_PROTOCOL_SGR) } else { self.modes.reset(DECSET_BIT_MOUSE_PROTOCOL_SGR) }
                        self.sgr_mouse = set;
                    },
                    1047 | 1048 | 1049 => {
                        if set {
                            if p == 1049 || p == 1048 { self.save_cursor(); }
                            if p == 1047 || p == 1049 {
                                self.use_alternate_buffer = true;
                                self.erase_in_display(2);
                            }
                        } else {
                            if p == 1047 || p == 1049 { self.use_alternate_buffer = false; }
                            if p == 1049 || p == 1048 { self.restore_cursor(); }
                        }
                    }
                    2004 => {
                        if set { self.modes.set(DECSET_BIT_BRACKETED_PASTE_MODE) } else { self.modes.reset(DECSET_BIT_BRACKETED_PASTE_MODE) }
                        self.bracketed_paste = set;
                    },
                    _ => {}
                }
            }
        }
    }

    pub fn save_cursor(&mut self) { self.cursor.save_state(self.current_style, self.modes.flags, self.use_line_drawing_g0, self.use_line_drawing_g1, self.use_line_drawing_uses_g0, self.fore_color, self.back_color); }
    pub fn restore_cursor(&mut self) {
        let s = self.cursor.restore_state();
        self.current_style = s.style; self.modes.flags = s.decset_flags;
        self.use_line_drawing_g0 = s.use_line_drawing_g0; self.use_line_drawing_g1 = s.use_line_drawing_g1;
        self.use_line_drawing_uses_g0 = s.use_line_drawing_uses_g0; self.fore_color = s.fore_color; self.back_color = s.back_color;
    }

    pub fn reset_to_initial_state(&mut self) {
        self.cursor = Cursor::new(); self.modes = TerminalModes::new();
        self.modes.set(DECSET_BIT_AUTOWRAP);
        self.cursor_enabled = true;
        self.application_cursor_keys = false;
        self.bracketed_paste = false;
        self.main_screen.erase_in_display(2, 0, STYLE_NORMAL);
        self.alt_screen.erase_in_display(2, 0, STYLE_NORMAL);
        self.use_alternate_buffer = false;
        self.top_margin = 0; self.bottom_margin = self.rows; self.title = None;
        self.fore_color = COLOR_INDEX_FOREGROUND; self.back_color = COLOR_INDEX_BACKGROUND; self.effect = 0; self.current_style = STYLE_NORMAL;
    }

    pub fn decaln_screen_align(&mut self) {
        let cols = self.cols as usize;
        for y in 0..self.rows {
            let r = self.get_current_screen_mut().get_row_mut(y);
            for x in 0..cols { r.text[x] = 'E'; r.styles[x] = STYLE_NORMAL; }
        }
        self.cursor.x = 0;
        self.cursor.y = 0;
        let r = self.rows;
        self.set_margins(1, r);
    }

    pub fn decstr_soft_reset(&mut self) {
        self.modes.reset(DECSET_BIT_ORIGIN_MODE); self.modes.set(DECSET_BIT_AUTOWRAP);
        self.cursor_enabled = true; self.top_margin = 0; self.bottom_margin = self.rows;
        self.current_style = STYLE_NORMAL; self.fore_color = COLOR_INDEX_FOREGROUND; self.back_color = COLOR_INDEX_BACKGROUND; self.effect = 0;
    }

    pub fn cursor_horizontal_relative(&mut self, n: i32) { self.cursor.move_relative(n, 0, self.cols, self.rows); }
    pub fn cursor_next_line(&mut self, n: i32) { self.cursor.y = min(self.bottom_margin - 1, self.cursor.y + n); self.cursor.x = self.left_margin; }
    pub fn cursor_previous_line(&mut self, n: i32) { self.cursor.y = max(self.top_margin, self.cursor.y - n); self.cursor.x = self.left_margin; }
    pub fn cursor_horizontal_absolute(&mut self, n: i32) { self.cursor.x = max(0, min(self.cols - 1, n - 1)); }
    pub fn cursor_vertical_absolute(&mut self, n: i32) { self.cursor.y = max(0, min(self.rows - 1, n - 1)); }
    pub fn cursor_vertical_relative(&mut self, n: i32) { self.cursor.y = max(0, min(self.rows - 1, self.cursor.y + n)); }
    pub fn reverse_index_scroll(&mut self) { if self.cursor.y == self.top_margin { self.insert_lines(1); } else { self.cursor.y = max(self.top_margin, self.cursor.y - 1); } }
    pub fn repeat_character(&mut self, n: i32, c: char) { for _ in 0..n { crate::terminal::handlers::print::handle_print(self, c); } }
    pub fn clear_tab_stop(&mut self, mode: i32) {
        match mode { 0 => if (self.cursor.x as usize) < self.tab_stops.len() { self.tab_stops[self.cursor.x as usize] = false; }, 3 => self.tab_stops.fill(false), _ => {} }
    }
    pub fn handle_osc18(&self) { self.report_terminal_response(&format!("\x1b]18;t={};{}t", self.cols, self.rows)); }
    pub fn clamp_cursor(&mut self) { self.cursor.clamp(self.cols, self.rows); }
    pub fn is_alternate_buffer_active(&self) -> bool { self.use_alternate_buffer }
    pub fn send_key_event(&mut self, _k: i32, _s: Option<String>, _m: i32) {}
    pub fn report_focus_gain(&self) {}
    pub fn report_focus_loss(&self) {}
    pub fn paste(&mut self, text: &str) { self.report_terminal_response(text); }

    pub fn copy_row_text(&self, row: i32, dest: &mut [u16]) {
        let r = self.get_current_screen().get_row(row);
        for i in 0..min(dest.len(), r.text.len()) { dest[i] = r.text[i] as u16; }
    }
    pub fn copy_row_styles_i64(&self, row: i32, dest: &mut [i64]) {
        let r = self.get_current_screen().get_row(row);
        for i in 0..min(dest.len(), r.styles.len()) { dest[i] = r.styles[i] as i64; }
    }

    // --- Added methods to fix compilation errors ---

    pub fn report_colors_changed(&self) {
        if let Some(obj) = &self.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let _ = env.call_method(obj.as_obj(), "onColorsChanged", "()V", &[]);
                }
            }
        }
    }

    pub fn report_color_response(&self, response: &str) {
        self.report_terminal_response(&format!("\x1b]{}\x07", response));
    }

    pub fn handle_osc13(&mut self) { /* Placeholder */ }
    pub fn handle_osc14(&mut self) { /* Placeholder */ }
    pub fn handle_osc19(&mut self) { /* Placeholder */ }
    
    pub fn handle_osc52(&mut self, _base64_data: &str) { /* Placeholder */ }

    pub fn cursor_backward_tab(&mut self, n: i32) {
        for _ in 0..n {
            let mut new_col = self.cursor.x - 1;
            while new_col >= self.left_margin && !self.tab_stops.get(new_col as usize).copied().unwrap_or(false) {
                new_col -= 1;
            }
            self.cursor.x = max(self.left_margin, new_col);
        }
    }

    pub fn handle_set_mode(&mut self, _params: &Params, _set: bool) { /* Placeholder */ }

    pub fn set_left_right_margins(&mut self, left: i32, right: i32) {
        self.left_margin = max(0, min(left - 1, self.cols - 1));
        self.right_margin = max(self.left_margin + 1, min(right, self.cols));
    }

    pub fn back_index_scroll(&mut self) {
        if self.cursor.x == self.left_margin {
            // Scroll right? Actually back_index is usually just cursor left or scroll
            // but in many emulators it's not implemented or simple.
        } else {
            self.cursor.x -= 1;
        }
    }

    pub fn forward_index_scroll(&mut self) {
        if self.cursor.x == self.right_margin - 1 {
            // Scroll left?
        } else {
            self.cursor.x += 1;
        }
    }

    pub fn report_bell(&self) {
        if let Some(obj) = &self.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let _ = env.call_method(obj.as_obj(), "onBell", "()V", &[]);
                }
            }
        }
    }
}

pub struct TerminalEngine { pub parser: Parser, pub state: ScreenState }
impl TerminalEngine {
    pub fn new(cols: i32, rows: i32, total_rows: i32, cw: i32, ch: i32) -> Self {
        Self { parser: Parser::new(), state: ScreenState::new(cols, rows, total_rows, cw, ch) }
    }
    pub fn process_bytes(&mut self, data: &[u8]) {
        let mut handler = PerformHandler { state: &mut self.state };
        self.parser.advance(&mut handler, data);
        self.state.sync_screen_to_flat_buffer();
        if !self.state.shared_buffer_ptr.is_null() {
            unsafe { if let Some(flat) = &self.state.flat_buffer { flat.sync_to_shared(self.state.shared_buffer_ptr); } }
        }
    }
    pub fn process_code_point(&mut self, code_point: u32) {
        // 将 Unicode 码点转换为 UTF-8 字节序列并处理
        let mut utf8_buf = [0u8; 4];
        let utf8_str = char::from_u32(code_point)
            .unwrap_or('\u{FFFD}') // 使用替换字符处理无效码点
            .encode_utf8(&mut utf8_buf);
        self.process_bytes(utf8_str.as_bytes());
    }
}
struct PerformHandler<'a> { state: &'a mut ScreenState }
impl<'a> Perform for PerformHandler<'a> {
    fn print(&mut self, c: char) {
        self.state.last_printed_char = Some(c);
        crate::terminal::handlers::print::handle_print(self.state, c); 
    }
    fn execute(&mut self, byte: u8) { crate::terminal::handlers::control::handle_control(self.state, byte); }
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        crate::terminal::handlers::csi::handle_csi(self.state, params, intermediates, action);
    }
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.len() > 0 { if let Ok(opcode) = std::str::from_utf8(params[0]) { crate::terminal::handlers::osc::handle_osc(self.state, opcode, params); } }
    }
    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) { crate::terminal::handlers::esc::handle_esc(self.state, intermediates, byte); }
    fn hook(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) { if action == 'q' && intermediates.is_empty() { self.state.sixel_decoder.start(params); } }
    fn put(&mut self, byte: u8) { self.state.sixel_decoder.process_data(&[byte]); }
    fn unhook(&mut self) { self.state.sixel_decoder.finish(); }
}
