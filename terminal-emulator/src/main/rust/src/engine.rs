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

/// 安全包装原始指针，允许在线程间传递
#[derive(Clone, Copy)]
pub struct SharedBufferPtr(pub *mut SharedScreenBuffer);
unsafe impl Send for SharedBufferPtr {}
unsafe impl Sync for SharedBufferPtr {}

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

use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

pub struct TerminalContext {
    pub lock: RwLock<TerminalEngine>,
    pub running: AtomicBool,
}

impl TerminalContext {
    pub fn new(engine: TerminalEngine) -> Self {
        Self { 
            lock: RwLock::new(engine),
            running: AtomicBool::new(true),
        }
    }

    pub fn start_io_thread(self: std::sync::Arc<Self>, pty_fd: i32) {
        let context = self.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            let mut pty_file = unsafe { std::fs::File::from_raw_fd(pty_fd) };
            
            while context.running.load(std::sync::atomic::Ordering::Relaxed) {
                match std::io::Read::read(&mut pty_file, &mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let mut engine = context.lock.write().unwrap();
                        engine.process_bytes(&buffer[..n]);
                        engine.notify_screen_updated();
                    }
                    Err(_) => break,
                }
            }
        });
    }
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
    pub shared_buffer_ptr: SharedBufferPtr,
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
            shared_buffer_ptr: SharedBufferPtr(std::ptr::null_mut()),
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
    /// 统一委托给 handle_decset() 处理，避免代码重复和状态不一致
    pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
        // 使用 handle_decset() 统一处理所有 DECSET/DECRST 模式
        // 这样可以确保状态一致性，避免 do_decset_or_reset 和 handle_decset 处理逻辑不同步
        // 构造 Params 对象，模拟 CSI?h 和 CSI?l 命令的参数格式
        use crate::vte_parser::Params;
        let mut params = Params::new();
        // 直接设置参数值
        if params.len < params.values.len() {
            params.values[params.len] = mode as i32;
            params.len += 1;
        }
        self.handle_decset(&params, setting);
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

    /// 报告 Sixel 图像到 Java 侧进行渲染
    pub fn report_sixel_image(&self, callback_obj: &Option<jni::objects::GlobalRef>) {
        if let Some(obj) = callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let decoder = &self.sixel_decoder;
                    let rgba_data = decoder.get_image_data();
                    let width = decoder.width.max(1) as i32;
                    let height = decoder.height.max(1) as i32;
                    let start_x = decoder.start_x;
                    let start_y = decoder.start_y;

                    // 创建 byte 数组
                    if let Ok(byte_array) = env.new_byte_array(rgba_data.len() as i32) {
                        let bytes: Vec<i8> = rgba_data.iter().map(|&b| b as i8).collect();
                        let _ = env.set_byte_array_region(&byte_array, 0, &bytes);

                        // 调用 Java 回调方法 onSixelImage
                        let _ = env.call_method(
                            obj.as_obj(),
                            "onSixelImage",
                            "([BIIII)V",
                            &[
                                JValue::Object(&byte_array.into()),
                                JValue::Int(width),
                                JValue::Int(height),
                                JValue::Int(start_x),
                                JValue::Int(start_y),
                            ]
                        );
                    }
                }
            }
        }
    }

    /// 报告清屏事件到 Java 侧
    pub fn report_clear_screen(&self) {
        if let Some(obj) = &self.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    // 调用 Java 回调方法 onClearScreen
                    let _ = env.call_method(
                        obj.as_obj(),
                        "onClearScreen",
                        "()V",
                        &[]
                    );
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
        let style = self.current_style;
        let cx = self.cursor.x;
        let cy = self.cursor.y;
        
        let (new_cx, new_cy) = self.main_screen.resize_with_reflow(cols, rows, style, cx, cy);
        
        // 副屏幕目前简单重置
        self.alt_screen = Screen::new(cols, rows, rows);
        
        self.cols = cols; 
        self.rows = rows;
        
        // 关键修复：同步更新滚动区域和侧边边距
        // 确保 resize 后 bottom_margin 等于新的物理行数，从而触发全屏滚动逻辑并记录历史
        self.top_margin = 0;
        self.bottom_margin = rows;
        self.left_margin = 0;
        self.right_margin = cols;

        self.cursor.x = new_cx;
        self.cursor.y = new_cy;
        
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
        let x = self.cursor.x;
        let y = self.cursor.y;
        let style = self.current_style;
        self.get_current_screen_mut().erase_in_display(mode, y, x, style);
        if mode == 3 { self.scroll_counter = 0; }

        // 清屏时通知 Java 侧清除 Sixel 图像
        // mode 0=从光标到末尾，1=从开头到光标，2=整个屏幕，3=整个屏幕并清除滚动缓冲区
        if mode == 2 || mode == 3 {
            self.report_clear_screen();
        }
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
                5 => self.effect |= EFFECT_BLINK,
                7 => self.effect |= EFFECT_REVERSE,
                8 => self.effect |= EFFECT_INVISIBLE,
                9 => self.effect |= EFFECT_STRIKETHROUGH,
                21 => self.effect |= EFFECT_UNDERLINE, // Double underline (treat as single)
                22 => { self.effect &= !EFFECT_BOLD; self.effect &= !EFFECT_DIM; }
                23 => self.effect &= !EFFECT_ITALIC,
                24 => self.effect &= !EFFECT_UNDERLINE,
                25 => self.effect &= !EFFECT_BLINK,
                27 => self.effect &= !EFFECT_REVERSE,
                28 => self.effect &= !EFFECT_INVISIBLE,
                29 => self.effect &= !EFFECT_STRIKETHROUGH,
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
        for param in params.iter() {
            for &p in param.iter() {
                match p {
                    1 => { // DECCKM - 应用光标键模式
                        if set { self.modes.set(DECSET_BIT_APPLICATION_CURSOR_KEYS) } else { self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS) }
                        self.application_cursor_keys = set;
                    },
                    3 => { // DECCOLM - 132 列模式（未实现，忽略）
                        // 不实现，避免屏幕闪烁
                    },
                    5 => { // DECSCNM - 反色模式
                        if set { self.modes.set(DECSET_BIT_REVERSE_VIDEO) } else { self.modes.reset(DECSET_BIT_REVERSE_VIDEO) }
                    },
                    6 => { // DECOM - 原点模式
                        if set { self.modes.set(DECSET_BIT_ORIGIN_MODE) } else { self.modes.reset(DECSET_BIT_ORIGIN_MODE) }
                    },
                    7 => { // DECAWM - 自动换行
                        if set { self.modes.set(DECSET_BIT_AUTOWRAP) } else { self.modes.reset(DECSET_BIT_AUTOWRAP) }
                    },
                    12 => { // 光标闪烁启动（未完全实现）
                        // 简单处理，不实现完整逻辑
                    },
                    25 => { // DECTCEM - 光标显示/隐藏
                        self.cursor_enabled = set;
                    },
                    40 => { // 132 列模式切换（未实现，忽略）
                        // 不实现
                    },
                    45 => { // 反向换行（未实现，忽略）
                        // 不实现
                    },
                    66 => { // DECNKM - 应用小键盘模式
                        if set { self.modes.set(DECSET_BIT_APPLICATION_KEYPAD) } else { self.modes.reset(DECSET_BIT_APPLICATION_KEYPAD) }
                    },
                    69 => { // DECLRMM - 左右边距模式
                        if set { self.modes.set(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) } else { self.modes.reset(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) }
                    },
                    1000 => { // 鼠标追踪 - 按下和释放
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
                    1002 => { // 鼠标追踪 - 按钮事件
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
                    1003 => { // 鼠标追踪 - 所有事件（未实现，忽略）
                        // 暂时不实现
                    },
                    1004 => { // 焦点事件
                        if set { self.modes.set(DECSET_BIT_SEND_FOCUS_EVENTS) } else { self.modes.reset(DECSET_BIT_SEND_FOCUS_EVENTS) }
                        self.send_focus_events = set;
                    },
                    1006 => { // SGR 鼠标协议
                        if set { self.modes.set(DECSET_BIT_MOUSE_PROTOCOL_SGR) } else { self.modes.reset(DECSET_BIT_MOUSE_PROTOCOL_SGR) }
                        self.sgr_mouse = set;
                    },
                    1034 => { // 8 位输入模式（未实现，忽略）
                        // 不实现
                    },
                    1047 => { // 备用屏幕
                        if set {
                            self.use_alternate_buffer = true;
                            self.erase_in_display(2);
                        } else {
                            self.use_alternate_buffer = false;
                        }
                    },
                    1048 => { // 保存/恢复光标
                        if set { self.save_cursor(); } else { self.restore_cursor(); }
                    },
                    1049 => { // 备用屏幕 + 保存/恢复光标
                        if set {
                            self.save_cursor();
                            self.use_alternate_buffer = true;
                            self.erase_in_display(2);
                        } else {
                            self.use_alternate_buffer = false;
                            self.restore_cursor();
                        }
                    },
                    2004 => { // 括号粘贴模式
                        if set { self.modes.set(DECSET_BIT_BRACKETED_PASTE_MODE) } else { self.modes.reset(DECSET_BIT_BRACKETED_PASTE_MODE) }
                        self.bracketed_paste = set;
                    },
                    _ => {
                        // 未知的 DECSET 模式，忽略
                    }
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
        // ED mode 2: Clear entire screen (cursor position doesn't matter for mode 2)
        self.main_screen.erase_in_display(2, 0, 0, STYLE_NORMAL);
        self.alt_screen.erase_in_display(2, 0, 0, STYLE_NORMAL);
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
    
    /// 报告焦点获得 - 发送 DECSET 1004 焦点事件
    pub fn report_focus_gain(&self) {
        if self.send_focus_events {
            self.report_terminal_response("\x1b[I");
        }
    }
    
    /// 报告焦点丢失 - 发送 DECSET 1004 焦点事件
    pub fn report_focus_loss(&self) {
        if self.send_focus_events {
            self.report_terminal_response("\x1b[O");
        }
    }
    
    pub fn paste(&mut self, text: &str) { self.report_terminal_response(text); }

    pub fn copy_row_text(&self, row: i32, dest: &mut [u16]) {
        let r = self.get_current_screen().get_row(row);
        for i in 0..min(dest.len(), r.text.len()) { dest[i] = r.text[i] as u16; }
    }
    pub fn copy_row_codepoints(&self, row: i32, dest: &mut [i32]) {
        let r = self.get_current_screen().get_row(row);
        for i in 0..min(dest.len(), r.text.len()) { dest[i] = r.text[i] as i32; }
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
        if !self.state.shared_buffer_ptr.0.is_null() {
            unsafe { if let Some(flat) = &self.state.flat_buffer { flat.sync_to_shared(self.state.shared_buffer_ptr.0); } }
        }
        self.notify_screen_updated();
    }
    
    /// 处理按键事件 - 实现 KeyHandler.getCode() 的逻辑
    pub fn send_key_event(&mut self, key_code: i32, _char_str: Option<String>, meta_state: i32) {
        // KeyEvent 常量定义
        const KEYCODE_DPAD_UP: i32 = 19;
        const KEYCODE_DPAD_DOWN: i32 = 20;
        const KEYCODE_DPAD_LEFT: i32 = 21;
        const KEYCODE_DPAD_RIGHT: i32 = 22;
        const KEYCODE_MOVE_HOME: i32 = 122;
        const KEYCODE_MOVE_END: i32 = 123;
        const KEYCODE_PAGE_UP: i32 = 92;
        const KEYCODE_PAGE_DOWN: i32 = 93;
        const KEYCODE_F1: i32 = 131;
        const KEYCODE_F2: i32 = 132;
        const KEYCODE_F3: i32 = 133;
        const KEYCODE_F4: i32 = 134;
        const KEYCODE_F5: i32 = 135;
        const KEYCODE_F6: i32 = 136;
        const KEYCODE_F7: i32 = 137;
        const KEYCODE_F8: i32 = 138;
        const KEYCODE_F9: i32 = 139;
        const KEYCODE_F10: i32 = 140;
        const KEYCODE_F11: i32 = 141;
        const KEYCODE_F12: i32 = 142;
        const KEYCODE_ENTER: i32 = 66;
        const KEYCODE_TAB: i32 = 61;
        const KEYCODE_ESCAPE: i32 = 111;
        const KEYCODE_DEL: i32 = 67;
        const KEYCODE_FORWARD_DEL: i32 = 112;
        const KEYCODE_INSERT: i32 = 124;
        
        const KEYMOD_SHIFT: i32 = 1;
        const KEYMOD_ALT: i32 = 2;
        const KEYMOD_CTRL: i32 = 4;
        
        let shift_down = (meta_state & KEYMOD_SHIFT) != 0;
        let alt_down = (meta_state & KEYMOD_ALT) != 0;
        let ctrl_down = (meta_state & KEYMOD_CTRL) != 0;
        
        // 确定 keyMode
        let key_mode = if shift_down { 1 } 
            else if alt_down { 2 } 
            else if ctrl_down { 4 } 
            else { 0 };
        
        let cursor_app = self.state.application_cursor_keys;
        
        // 生成转义序列
        let escape_seq: Option<String> = match key_code {
            KEYCODE_DPAD_UP => {
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOA".to_string() } else { "\x1b[A".to_string() })
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'A'))
                }
            },
            KEYCODE_DPAD_DOWN => {
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOB".to_string() } else { "\x1b[B".to_string() })
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'B'))
                }
            },
            KEYCODE_DPAD_LEFT => {
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOC".to_string() } else { "\x1b[C".to_string() })
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'D'))
                }
            },
            KEYCODE_DPAD_RIGHT => {
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOD".to_string() } else { "\x1b[D".to_string() })
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'C'))
                }
            },
            KEYCODE_MOVE_HOME => {
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOH".to_string() } else { "\x1b[H".to_string() })
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'H'))
                }
            },
            KEYCODE_MOVE_END => {
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOF".to_string() } else { "\x1b[F".to_string() })
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'F'))
                }
            },
            KEYCODE_PAGE_UP => Some(self.transform_for_modifiers("\x1b[5", key_mode, '~')),
            KEYCODE_PAGE_DOWN => Some(self.transform_for_modifiers("\x1b[6", key_mode, '~')),
            KEYCODE_ENTER => {
                // 修复：ENTER 键应该只发送 \r，只有 Alt 修饰时才加 ESC 前缀
                if alt_down { Some("\x1b\r".to_string()) } else { Some("\r".to_string()) }
            },
            KEYCODE_TAB => {
                // Shift+Tab 发送 \x1b[Z，普通 Tab 发送 \t
                if shift_down { Some("\x1b[Z".to_string()) } else { Some("\t".to_string()) }
            },
            KEYCODE_ESCAPE => Some("\x1b".to_string()),
            KEYCODE_DEL => Some("\x7f".to_string()),
            KEYCODE_FORWARD_DEL => Some(self.transform_for_modifiers("\x1b[3", key_mode, '~')),
            KEYCODE_INSERT => Some(self.transform_for_modifiers("\x1b[2", key_mode, '~')),
            KEYCODE_F1 => {
                if key_mode == 0 {
                    Some("\x1bOP".to_string())
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'P'))
                }
            },
            KEYCODE_F2 => {
                if key_mode == 0 {
                    Some("\x1bOQ".to_string())
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'Q'))
                }
            },
            KEYCODE_F3 => {
                if key_mode == 0 {
                    Some("\x1bOR".to_string())
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'R'))
                }
            },
            KEYCODE_F4 => {
                if key_mode == 0 {
                    Some("\x1bOS".to_string())
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'S'))
                }
            },
            KEYCODE_F5 => Some(self.transform_for_modifiers("\x1b[15", key_mode, '~')),
            KEYCODE_F6 => Some(self.transform_for_modifiers("\x1b[17", key_mode, '~')),
            KEYCODE_F7 => Some(self.transform_for_modifiers("\x1b[18", key_mode, '~')),
            KEYCODE_F8 => Some(self.transform_for_modifiers("\x1b[19", key_mode, '~')),
            KEYCODE_F9 => Some(self.transform_for_modifiers("\x1b[20", key_mode, '~')),
            KEYCODE_F10 => Some(self.transform_for_modifiers("\x1b[21", key_mode, '~')),
            KEYCODE_F11 => Some(self.transform_for_modifiers("\x1b[23", key_mode, '~')),
            KEYCODE_F12 => Some(self.transform_for_modifiers("\x1b[24", key_mode, '~')),
            _ => None,
        };
        
        if let Some(seq) = escape_seq {
            self.process_bytes(seq.as_bytes());
        }
    }
    
    /// 为功能键转换修饰符
    fn transform_for_modifiers(&self, base: &str, key_mode: i32, suffix: char) -> String {
        let mut result = base.to_string();
        // 添加修饰符参数：1=shift, 2=alt, 4=ctrl
        let modifier = key_mode + 1;
        result.push_str(&format!(";{}", modifier));
        result.push(suffix);
        result
    }
    
    pub fn process_code_point(&mut self, code_point: u32) {
        // 将 Unicode 码点转换为 UTF-8 字节序列并处理
        let mut utf8_buf = [0u8; 4];
        let utf8_str = char::from_u32(code_point)
            .unwrap_or('\u{FFFD}') // 使用替换字符处理无效码点
            .encode_utf8(&mut utf8_buf);
        self.process_bytes(utf8_str.as_bytes());
        // process_bytes 内部已经调用了 notify_screen_updated
    }

    pub fn notify_screen_updated(&self) {
        if let Some(obj) = &self.state.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                // 尝试获取当前线程的 JNIEnv，如果未附加则尝试附加
                let env_res = vm.get_env().or_else(|_| vm.attach_current_thread_as_daemon());
                if let Ok(mut env) = env_res {
                    let _ = env.call_method(obj.as_obj(), "onScreenUpdated", "()V", &[]);
                }
            }
        }
    }
}

impl ScreenState {
    /// 获取调试信息（用于 toString() 方法）
    pub fn get_debug_info(&self) -> String {
        format!(
            "TerminalEngine[cursor=({},{}),style={},size={}x{},rows={},cols={},alt={}]",
            self.cursor.y,
            self.cursor.x,
            self.cursor.style,
            self.rows,
            self.cols,
            self.main_screen.rows,
            self.main_screen.cols,
            self.use_alternate_buffer
        )
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
    fn unhook(&mut self) {
        self.state.sixel_decoder.finish();
        // Sixel 图像完成，回调到 Java 渲染
        self.state.report_sixel_image(&self.state.java_callback_obj);
    }

    // 实现解析器缺失的直接回调，统一走 handle_control
    fn bell(&mut self) { crate::terminal::handlers::control::handle_control(self.state, 0x07); }
    fn backspace(&mut self) { crate::terminal::handlers::control::handle_control(self.state, 0x08); }
    fn tab(&mut self) { crate::terminal::handlers::control::handle_control(self.state, 0x09); }
    fn linefeed(&mut self) { crate::terminal::handlers::control::handle_control(self.state, 0x0a); }
    fn carriage_return(&mut self) { crate::terminal::handlers::control::handle_control(self.state, 0x0d); }
}
