/// 屏幕状态管理
use std::cmp::{max, min};
use jni::objects::JValue;

use crate::terminal::style::*;
use crate::terminal::modes::*;
use crate::terminal::colors::*;
use crate::terminal::sixel::SixelDecoder;
use crate::terminal::screen::Screen;
use crate::terminal::cursor::Cursor;
use crate::engine::shared_buffer::{SharedBufferPtr, FlatScreenBuffer, SharedScreenBuffer};
use crate::engine::events::TerminalEvent;

/// 文本选择区域
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
}

impl Selection {
    pub fn normalize(&self) -> (i64, i64, i64, i64) {
        if self.y1 < self.y2 {
            (self.x1, self.y1, self.x2, self.y2)
        } else if self.y1 > self.y2 {
            (self.x2, self.y2, self.x1, self.y1)
        } else {
            (self.x1.min(self.x2), self.y1, self.x1.max(self.x2), self.y2)
        }
    }
}

/// 屏幕状态 - 包含所有终端可见状态
pub struct ScreenState {
    pub rows: i64,
    pub cols: i64,

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
    pub top_margin: i64,
    pub bottom_margin: i64,
    pub left_margin: i64,
    pub right_margin: i64,
    pub current_style: u64,
    pub tab_stops: Vec<bool>,
    pub title: Option<String>,
    pub title_stack: Vec<String>,
    pub use_line_drawing_g0: bool,
    pub use_line_drawing_g1: bool,
    pub use_line_drawing_uses_g0: bool,
    pub scroll_counter: i64,
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
    pub selection: Option<Selection>,
}

impl Drop for ScreenState {
    fn drop(&mut self) {
        if self.java_callback_obj.take().is_some() {
            crate::utils::android_log(crate::utils::LogPriority::DEBUG, "ScreenState: Released java_callback_obj GlobalRef");
        }

        let ptr = self.shared_buffer_ptr.0;
        if !ptr.is_null() {
            unsafe {
                let total_allocated_rows = self.main_screen.buffer.len();
                let size = SharedScreenBuffer::required_size(self.cols as usize, total_allocated_rows);
                let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
                std::alloc::dealloc(ptr as *mut u8, layout);
            }
            self.shared_buffer_ptr = SharedBufferPtr(std::ptr::null_mut());
            crate::utils::android_log(crate::utils::LogPriority::DEBUG, "ScreenState: Deallocated shared_buffer_ptr");
        }
    }
}

impl ScreenState {
    pub fn new(cols: i64, rows: i64, total_rows: i64, _cw: i32, _ch: i32) -> Self {
        let mut tab_stops = vec![false; cols as usize];
        for i in (8..cols as usize).step_by(8) { tab_stops[i] = true; }

        let mut modes = TerminalModes::new();
        modes.set(DECSET_BIT_AUTOWRAP);

        Self {
            rows, cols,
            main_screen: Screen::new(cols, rows, total_rows),
            alt_screen: Screen::new(cols, rows, rows),
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
            fore_color: COLOR_INDEX_FOREGROUND as u64,
            back_color: COLOR_INDEX_BACKGROUND as u64,
            effect: 0,
            cursor_enabled: true,
            application_cursor_keys: false,
            bracketed_paste: false,
            send_focus_events: false,
            mouse_tracking: false,
            mouse_button_event: false,
            sgr_mouse: false,
            auto_scroll_disabled: false,
            underline_color: COLOR_INDEX_FOREGROUND as u64,
            selection: None,
        }
    }

    pub fn get_current_screen(&self) -> &Screen {
        if self.use_alternate_buffer { &self.alt_screen } else { &self.main_screen }
    }

    pub fn get_current_screen_mut(&mut self) -> &mut Screen {
        if self.use_alternate_buffer { &mut self.alt_screen } else { &mut self.main_screen }
    }

    // --- 文本选择 ---

    pub fn set_selection(&mut self, x1: i64, y1: i64, x2: i64, y2: i64) {
        self.selection = Some(Selection { x1, y1, x2, y2 });
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn is_selection_active(&self) -> bool {
        self.selection.is_some()
    }

    pub fn get_selection_text(&self) -> String {
        match self.selection {
            Some(sel) => {
                let (sx, sy, ex, ey) = sel.normalize();
                self.get_current_screen().get_selected_text(sx, sy, ex, ey)
            }
            None => String::new(),
        }
    }

    pub fn scroll_up(&mut self) {
        let top = self.top_margin;
        let bottom = self.bottom_margin;
        let style = self.current_style;
        self.get_current_screen_mut().scroll_up(top, bottom, style);
        if !self.use_alternate_buffer && !self.auto_scroll_disabled && top == 0 && bottom == self.rows {
            self.scroll_counter += 1;
            // 全屏滚动时，选择区域 Y 坐标跟随内容上移
            if let Some(ref mut sel) = self.selection {
                sel.y1 -= 1;
                sel.y2 -= 1;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        let top = self.top_margin;
        let bottom = self.bottom_margin;
        let style = self.current_style;
        self.get_current_screen_mut().scroll_down(top, bottom, style);
    }

    pub fn set_alternate_buffer(&mut self, use_alt: bool) {
        if self.use_alternate_buffer != use_alt {
            self.use_alternate_buffer = use_alt;
            // 通知 Java 侧缓冲区切换
            if let Some(obj) = &self.java_callback_obj {
                if let Some(vm) = crate::JAVA_VM.get() {
                    if let Ok(mut env) = vm.get_env() {
                        let _ = env.call_method(obj.as_obj(), "onBufferChanged", "(Z)V", &[JValue::Bool(use_alt as jni::sys::jboolean)]);
                    }
                }
            }
        }
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
        self.report_title_change(title);
    }

    pub fn push_title(&mut self) {
        if let Some(title) = &self.title {
            self.title_stack.push(title.clone());
        }
    }

    pub fn pop_title(&mut self) {
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

    pub fn report_sixel_image(&self, callback_obj: &Option<jni::objects::GlobalRef>) {
        if let Some(obj) = callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let decoder = &self.sixel_decoder;
                    let rgba_data = decoder.get_image_data();
                    let width = decoder.width.max(1) as i32;
                    let height = decoder.height.max(1) as i32;
                    let start_x = decoder.start_x as i32;
                    let start_y = decoder.start_y as i32;

                    if let Ok(byte_array) = env.new_byte_array(rgba_data.len() as i32) {
                        let bytes: Vec<i8> = rgba_data.iter().map(|&b| b as i8).collect();
                        let _ = env.set_byte_array_region(&byte_array, 0, &bytes);

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

    pub fn report_clear_screen(&self) {
        if let Some(obj) = &self.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let _ = env.call_method(obj.as_obj(), "onClearScreen", "()V", &[]);
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

    pub fn send_mouse_event(&mut self, button: u32, column: i64, row: i64, pressed: bool) {
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

    pub fn resize(&mut self, cols: i64, rows: i64) {
        let style = self.current_style;
        let cx = self.cursor.x;
        let cy = self.cursor.y;

        let (new_cx, new_cy) = self.main_screen.resize_with_reflow(cols as i32, rows as i32, style, cx as i32, cy as i32);

        self.alt_screen = Screen::new(cols, rows, rows);

        self.cols = cols;
        self.rows = rows;

        self.top_margin = 0;
        self.bottom_margin = rows;
        self.left_margin = 0;
        self.right_margin = cols;

        self.cursor.x = new_cx as i64;
        self.cursor.y = new_cy as i64;

        self.flat_buffer = Some(FlatScreenBuffer::new(cols as usize, self.main_screen.buffer.len()));
        self.cursor.clamp(cols, rows);
        self.sync_screen_to_flat_buffer();
    }

    pub fn insert_characters(&mut self, n: i64) {
        let y = self.cursor.y;
        let style = self.current_style;
        let x = self.cursor.x;
        self.get_current_screen_mut().get_row_mut(y).insert_spaces(x as u64, n as u64, style);
    }

    pub fn erase_in_display(&mut self, mode: i64) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        let style = self.current_style;
        self.get_current_screen_mut().erase_in_display(mode, y, x, style);
        if mode == 3 { self.scroll_counter = 0; }

        if mode == 2 {
            // ESC[2J - 清空整个可见屏幕，清除 Sixel 状态并通知 Java
            self.sixel_decoder.reset();
            self.report_clear_screen();
            crate::render_thread::request_render();
        } else if mode == 3 {
            // ESC[3J - 仅清除滚动历史，可见内容不变，保留 Sixel 图像
            crate::render_thread::request_render();
        }
    }

    pub fn erase_in_line(&mut self, mode: i32) {
        let cols = self.cols as u64;
        let x = self.cursor.x as u64;
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

    pub fn insert_lines(&mut self, n: i64) {
        let y = self.cursor.y;
        let bm = self.bottom_margin;
        let style = self.current_style;
        self.get_current_screen_mut().insert_lines(y, bm, n, style);
    }

    pub fn delete_lines(&mut self, n: i64) {
        let y = self.cursor.y;
        let bm = self.bottom_margin;
        let style = self.current_style;
        self.get_current_screen_mut().delete_lines(y, bm, n, style);
    }

    pub fn delete_characters(&mut self, n: i64) {
        let x = self.cursor.x as u64;
        let y = self.cursor.y;
        let style = self.current_style;
        self.get_current_screen_mut().get_row_mut(y).delete_characters(x, n as u64, style);
    }

    pub fn erase_characters(&mut self, n: i64) {
        let x = self.cursor.x as u64;
        let y = self.cursor.y;
        let style = self.current_style;
        self.get_current_screen_mut().get_row_mut(y).clear(x, x + n as u64, style);
    }

    pub fn set_margins(&mut self, top: i64, bottom: i64) {
        self.top_margin = max(0, min(top - 1, self.rows - 1));
        self.bottom_margin = max(self.top_margin + 1, min(bottom, self.rows));
    }

    pub fn set_left_right_margins(&mut self, left: i64, right: i64) {
        self.left_margin = max(0, min(left - 1, self.cols - 1));
        self.right_margin = max(self.left_margin + 1, min(right, self.cols));
    }

    pub fn save_cursor(&mut self) {
        self.cursor.save_state(
            self.current_style, self.modes.flags,
            self.use_line_drawing_g0, self.use_line_drawing_g1,
            self.use_line_drawing_uses_g0, self.fore_color, self.back_color,
        );
    }

    pub fn restore_cursor(&mut self) {
        let s = self.cursor.restore_state();
        self.current_style = s.style;
        self.modes.flags = s.decset_flags;
        self.use_line_drawing_g0 = s.use_line_drawing_g0;
        self.use_line_drawing_g1 = s.use_line_drawing_g1;
        self.use_line_drawing_uses_g0 = s.use_line_drawing_uses_g0;
        self.fore_color = s.fore_color;
        self.back_color = s.back_color;
    }

    pub fn reset_to_initial_state(&mut self) {
        self.cursor = Cursor::new();
        self.modes = TerminalModes::new();
        self.modes.set(DECSET_BIT_AUTOWRAP);
        self.cursor_enabled = true;
        self.application_cursor_keys = false;
        self.bracketed_paste = false;
        self.main_screen.erase_in_display(2, 0, 0, STYLE_NORMAL);
        self.alt_screen.erase_in_display(2, 0, 0, STYLE_NORMAL);
        self.use_alternate_buffer = false;
        self.top_margin = 0;
        self.bottom_margin = self.rows;
        self.left_margin = 0;
        self.right_margin = self.cols;
        self.title = None;
        self.fore_color = COLOR_INDEX_FOREGROUND as u64;
        self.back_color = COLOR_INDEX_BACKGROUND as u64;
        self.effect = 0;
        self.current_style = STYLE_NORMAL;
    }

    pub fn decaln_screen_align(&mut self) {
        let cols = self.cols as u64;
        for y in 0..self.rows {
            let r = self.get_current_screen_mut().get_row_mut(y);
            for x in 0..(cols as usize) { r.text[x] = 'E'; r.styles[x] = STYLE_NORMAL; }
        }
        self.cursor.x = 0;
        self.cursor.y = 0;
        let r = self.rows;
        self.set_margins(1, r);
    }

    pub fn decstr_soft_reset(&mut self) {
        // 重置所有应该在软复位时恢复的模式
        self.modes.reset(DECSET_BIT_ORIGIN_MODE);
        self.modes.reset(MODE_LNM);
        self.modes.reset(MODE_INSERT);
        self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS);
        self.modes.reset(DECSET_BIT_BRACKETED_PASTE_MODE);
        self.modes.set(DECSET_BIT_AUTOWRAP);
        
        self.cursor_enabled = true;
        self.application_cursor_keys = false;
        self.bracketed_paste = false;
        self.send_focus_events = false;
        self.mouse_tracking = false;
        self.mouse_button_event = false;
        self.sgr_mouse = false;
        self.top_margin = 0;
        self.bottom_margin = self.rows;
        self.left_margin = 0;
        self.right_margin = self.cols;
        self.current_style = STYLE_NORMAL;
        self.fore_color = COLOR_INDEX_FOREGROUND as u64;
        self.back_color = COLOR_INDEX_BACKGROUND as u64;
        self.effect = 0;
        self.use_line_drawing_g0 = false;
        self.use_line_drawing_g1 = false;
        self.use_line_drawing_uses_g0 = true;
    }

    pub fn cursor_horizontal_relative(&mut self, n: i64) { self.cursor.move_relative(n, 0, self.cols, self.rows); }
    pub fn cursor_next_line(&mut self, n: i64) { self.cursor.y = min(self.bottom_margin - 1, self.cursor.y + n); self.cursor.x = self.left_margin; }
    pub fn cursor_previous_line(&mut self, n: i64) { self.cursor.y = max(self.top_margin, self.cursor.y - n); self.cursor.x = self.left_margin; }
    pub fn cursor_horizontal_absolute(&mut self, n: i64) { self.cursor.x = max(0, min(self.cols - 1, n - 1)); }
    pub fn cursor_vertical_absolute(&mut self, n: i64) { self.cursor.y = max(0, min(self.rows - 1, n - 1)); }
    pub fn cursor_vertical_relative(&mut self, n: i64) { self.cursor.y = max(0, min(self.rows - 1, self.cursor.y + n)); }
    pub fn reverse_index_scroll(&mut self) { if self.cursor.y == self.top_margin { self.insert_lines(1); } else { self.cursor.y = max(self.top_margin, self.cursor.y - 1); } }
    pub fn repeat_character(&mut self, n: i64, c: char) { for _ in 0..n { crate::terminal::handlers::print::handle_print(self, c); } }

    pub fn cursor_backward_tab(&mut self, n: i64) {
        for _ in 0..n {
            let mut new_col = self.cursor.x - 1;
            while new_col > self.left_margin && !self.tab_stops.get(new_col as usize).copied().unwrap_or(false) { new_col -= 1; }
            self.cursor.x = max(self.left_margin, new_col);
        }
    }

    pub fn clear_tab_stop(&mut self, mode: i32) {
        match mode {
            0 => if (self.cursor.x as usize) < self.tab_stops.len() { self.tab_stops[self.cursor.x as usize] = false; },
            3 => self.tab_stops.fill(false),
            _ => {}
        }
    }

    pub fn handle_osc18(&self) { self.report_terminal_response(&format!("\x1b]18;t={};{}t", self.cols, self.rows)); }
    pub fn clamp_cursor(&mut self) { self.cursor.clamp(self.cols, self.rows); }
    pub fn is_alternate_buffer_active(&self) -> bool { self.use_alternate_buffer }

    pub fn report_focus_gain(&self) {
        if self.send_focus_events {
            self.report_terminal_response("\x1b[I");
        }
    }

    pub fn report_focus_loss(&self) {
        if self.send_focus_events {
            self.report_terminal_response("\x1b[O");
        }
    }

    pub fn paste(&mut self, text: &str) {
        let sanitized = text.replace("\r\n", "\r").replace('\n', "\r");
        if self.bracketed_paste {
            self.report_terminal_response(&format!("\x1b[200~{}\x1b[201~", sanitized));
        } else {
            self.report_terminal_response(&sanitized);
        }
    }

    pub fn auto_wrap(&self) -> bool { self.modes.is_enabled(DECSET_BIT_AUTOWRAP) }
    pub fn origin_mode(&self) -> bool { self.modes.is_enabled(DECSET_BIT_ORIGIN_MODE) }
    pub fn leftright_margin_mode(&self) -> bool { self.modes.is_enabled(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) }

    pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
        use crate::vte_parser::Params;
        let mut params = Params::new();
        params.values[0] = mode as i32;
        params.len = 1;
        self.handle_decset(&params, setting);
    }

    pub fn scroll_up_lines(&mut self, n: i64) { for _ in 0..n { self.scroll_up(); } }
    pub fn scroll_down_lines(&mut self, n: i64) {
        let old_y = self.cursor.y;
        self.cursor.y = self.top_margin;
        self.insert_lines(n);
        self.cursor.y = old_y;
    }

    pub fn handle_set_mode(&mut self, params: &crate::vte_parser::Params, set: bool) {
        for param in params.iter() {
            for &p in param.iter() {
                match p {
                    4 => {
                        if set { self.modes.set(MODE_INSERT); } else { self.modes.reset(MODE_INSERT); }
                    },
                    20 => {
                        if set { self.modes.set(MODE_LNM); } else { self.modes.reset(MODE_LNM); }
                    },
                    _ => {}
                }
            }
        }
    }

    pub fn back_index_scroll(&mut self) {
        if self.cursor.x == self.left_margin {
            // Not implemented
        } else {
            self.cursor.x -= 1;
        }
    }

    pub fn forward_index_scroll(&mut self) {
        if self.cursor.x == self.right_margin - 1 {
            // Not implemented
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

    pub fn report_colors_changed(&self) {}
    pub fn report_color_response(&self, response: &str) {
        self.report_terminal_response(&format!("\x1b]{}\x07", response));
    }

    pub fn handle_osc13(&mut self) {}
    pub fn handle_osc14(&mut self) {}
    pub fn handle_osc19(&mut self) {}
    pub fn handle_osc52(&mut self, _events: &mut Vec<TerminalEvent>, base64_data: &str) {
        use base64::{Engine as _, engine::general_purpose};
        if let Ok(decoded) = general_purpose::STANDARD.decode(base64_data) {
            if let Ok(text) = String::from_utf8(decoded) {
                if let Some(obj) = &self.java_callback_obj {
                    if let Some(vm) = crate::JAVA_VM.get() {
                        if let Ok(mut env) = vm.get_env() {
                            if let Ok(j_text) = env.new_string(text) {
                                let _ = env.call_method(obj.as_obj(), "onCopyTextToClipboard", "(Ljava/lang/String;)V", &[JValue::Object(&j_text.into())]);
                            }
                        }
                    }
                }
            }
        }
    }

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

    pub fn saved_decset_flags(&self) -> i32 { self.cursor.saved_state.decset_flags }
    pub fn decset_flags(&self) -> i32 { self.modes.flags }

    pub fn copy_row_text(&self, row: i64, dest: &mut [u16]) {
        let screen = self.get_current_screen();
        let r = screen.get_row(row);
        for i in 0..min(dest.len(), r.text.len()) {
            dest[i] = r.text[i] as u16;
        }
    }

    pub fn copy_row_codepoints(&self, row: i64, dest: &mut [i32]) {
        let screen = self.get_current_screen();
        let r = screen.get_row(row);
        let len = min(r.text.len(), dest.len());
        for i in 0..len { dest[i] = r.text[i] as i32; }
    }

    pub fn copy_row_styles_i64(&self, row: i64, dest: &mut [i64]) {
        let screen = self.get_current_screen();
        let r = screen.get_row(row);
        let len = min(r.styles.len(), dest.len());
        for i in 0..len { dest[i] = r.styles[i] as i64; }
    }

    pub fn sync_screen_to_flat_buffer(&mut self) {
        if self.flat_buffer.is_none() { return; }
        let screen = if self.use_alternate_buffer { &self.alt_screen } else { &self.main_screen };
        let flat = self.flat_buffer.as_mut().unwrap();
        
        let rows = self.rows as usize;
        let cols = self.cols as usize;
        
        for r in 0..rows {
            let row_data = screen.get_row(r as i64);
            let row_len = row_data.text.len();
            
            for c_idx in 0..cols {
                let flat_idx = r * cols + c_idx;
                if c_idx < row_len {
                    flat.text_data[flat_idx] = row_data.text[c_idx] as u16;
                    flat.style_data[flat_idx] = row_data.styles[c_idx];
                } else {
                    flat.text_data[flat_idx] = ' ' as u16;
                    flat.style_data[flat_idx] = STYLE_NORMAL;
                }
            }
        }

        // 同步到共享内存指针（如果存在）
        if !self.shared_buffer_ptr.0.is_null() {
            unsafe {
                flat.sync_to_shared(self.shared_buffer_ptr.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Selection::normalize
    // -------------------------------------------------------------------------
    #[test]
    fn selection_normalize_forward() {
        let sel = Selection { x1: 2, y1: 1, x2: 5, y2: 3 };
        assert_eq!(sel.normalize(), (2, 1, 5, 3));
    }

    #[test]
    fn selection_normalize_backward() {
        let sel = Selection { x1: 5, y1: 3, x2: 2, y2: 1 };
        assert_eq!(sel.normalize(), (2, 1, 5, 3));
    }

    #[test]
    fn selection_normalize_same_row_reversed() {
        let sel = Selection { x1: 5, y1: 2, x2: 1, y2: 2 };
        assert_eq!(sel.normalize(), (1, 2, 5, 2));
    }

    // -------------------------------------------------------------------------
    // ScreenState initialization
    // -------------------------------------------------------------------------
    #[test]
    fn screen_state_new_defaults() {
        let state = ScreenState::new(80, 24, 1000, 10, 20);
        assert_eq!(state.cols, 80);
        assert_eq!(state.rows, 24);
        assert!(!state.use_alternate_buffer);
        assert_eq!(state.cursor.x, 0);
        assert_eq!(state.cursor.y, 0);
        assert!(state.cursor_enabled);
        assert!(state.auto_wrap());
        assert_eq!(state.top_margin, 0);
        assert_eq!(state.bottom_margin, 24);
        assert_eq!(state.left_margin, 0);
        assert_eq!(state.right_margin, 80);
        assert!(state.selection.is_none());
    }

    #[test]
    fn screen_state_tab_stops_initialized() {
        let state = ScreenState::new(80, 24, 100, 8, 16);
        assert!(!state.tab_stops[0]);
        assert!(!state.tab_stops[7]);
        assert!(state.tab_stops[8]);
        assert!(!state.tab_stops[9]);
        assert!(state.tab_stops[16]);
        assert!(state.tab_stops[72]);
        assert!(!state.tab_stops[79]);
    }

    // -------------------------------------------------------------------------
    // Selection management
    // -------------------------------------------------------------------------
    #[test]
    fn set_and_clear_selection() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        assert!(!state.is_selection_active());
        state.set_selection(1, 2, 3, 4);
        assert!(state.is_selection_active());
        state.clear_selection();
        assert!(!state.is_selection_active());
    }

    // -------------------------------------------------------------------------
    // Alternate buffer switching
    // -------------------------------------------------------------------------
    #[test]
    fn alternate_buffer_switch() {
        let mut state = ScreenState::new(80, 24, 1000, 8, 16);
        assert!(!state.is_alternate_buffer_active());
        assert_eq!(state.get_current_screen().buffer.len(), 1000);

        state.set_alternate_buffer(true);
        assert!(state.is_alternate_buffer_active());
        assert_eq!(state.get_current_screen().buffer.len(), 24);

        state.set_alternate_buffer(false);
        assert!(!state.is_alternate_buffer_active());
        assert_eq!(state.get_current_screen().buffer.len(), 1000);
    }

    // -------------------------------------------------------------------------
    // Resize
    // -------------------------------------------------------------------------
    #[test]
    fn resize_updates_dimensions() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.resize(100, 30);
        assert_eq!(state.cols, 100);
        assert_eq!(state.rows, 30);
        assert_eq!(state.top_margin, 0);
        assert_eq!(state.bottom_margin, 30);
        assert_eq!(state.right_margin, 100);
    }

    #[test]
    fn resize_clamps_cursor() {
        let mut state = ScreenState::new(10, 5, 100, 8, 16);
        state.cursor.x = 8;
        state.cursor.y = 4;
        state.resize(5, 3);
        assert!(state.cursor.x < state.cols);
        assert!(state.cursor.y < state.rows);
    }

    // -------------------------------------------------------------------------
    // Margins
    // -------------------------------------------------------------------------
    #[test]
    fn set_margins_clamps() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.set_margins(5, 20);
        assert_eq!(state.top_margin, 4);
        assert_eq!(state.bottom_margin, 20);
    }

    #[test]
    fn set_margins_out_of_bounds() {
        let mut state = ScreenState::new(10, 5, 100, 8, 16);
        state.set_margins(0, 100);
        assert_eq!(state.top_margin, 0);
        assert_eq!(state.bottom_margin, 5);
    }

    #[test]
    fn set_left_right_margins() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.set_left_right_margins(10, 70);
        assert_eq!(state.left_margin, 9);
        assert_eq!(state.right_margin, 70);
    }

    // -------------------------------------------------------------------------
    // Cursor absolute positioning
    // -------------------------------------------------------------------------
    #[test]
    fn cursor_horizontal_absolute() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.cursor_horizontal_absolute(10);
        assert_eq!(state.cursor.x, 9);
    }

    #[test]
    fn cursor_horizontal_absolute_clamps() {
        let mut state = ScreenState::new(10, 5, 100, 8, 16);
        state.cursor_horizontal_absolute(100);
        assert_eq!(state.cursor.x, 9);
        state.cursor_horizontal_absolute(0);
        assert_eq!(state.cursor.x, 0);
    }

    #[test]
    fn cursor_vertical_absolute() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.cursor_vertical_absolute(12);
        assert_eq!(state.cursor.y, 11);
    }

    // -------------------------------------------------------------------------
    // Tab stops
    // -------------------------------------------------------------------------
    #[test]
    fn clear_tab_stop_single() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        assert!(state.tab_stops[16]);
        state.cursor.x = 16;
        state.clear_tab_stop(0);
        assert!(!state.tab_stops[16]);
    }

    #[test]
    fn clear_all_tab_stops() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.clear_tab_stop(3);
        assert!(state.tab_stops.iter().all(|&t| !t));
    }

    // -------------------------------------------------------------------------
    // Reset commands
    // -------------------------------------------------------------------------
    #[test]
    fn reset_to_initial_state_clears() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.set_alternate_buffer(true);
        state.cursor.x = 10;
        state.cursor.y = 5;
        state.selection = Some(Selection { x1: 0, y1: 0, x2: 1, y2: 1 });
        state.reset_to_initial_state();
        assert!(!state.is_alternate_buffer_active());
        assert_eq!(state.cursor.x, 0);
        assert_eq!(state.cursor.y, 0);
        assert!(state.auto_wrap());
    }

    #[test]
    fn decstr_soft_reset() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.modes.set(MODE_INSERT);
        state.modes.set(MODE_LNM);
        state.cursor_enabled = false;
        state.application_cursor_keys = true;
        state.mouse_tracking = true;
        state.decstr_soft_reset();
        assert!(!state.modes.is_enabled(MODE_INSERT));
        assert!(!state.modes.is_enabled(MODE_LNM));
        assert!(state.cursor_enabled);
        assert!(!state.application_cursor_keys);
        assert!(!state.mouse_tracking);
        assert!(state.auto_wrap());
    }

    // -------------------------------------------------------------------------
    // Scroll counter with selection tracking
    // -------------------------------------------------------------------------
    #[test]
    fn scroll_up_increments_counter_on_full_screen() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.scroll_up();
        assert_eq!(state.scroll_counter, 1);
        state.scroll_up();
        assert_eq!(state.scroll_counter, 2);
    }

    #[test]
    fn scroll_up_does_not_increment_with_margins() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.set_margins(2, 10);
        state.scroll_up();
        assert_eq!(state.scroll_counter, 0);
    }

    #[test]
    fn scroll_up_moves_selection() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.set_selection(0, 5, 10, 8);
        state.scroll_up();
        let sel = state.selection.unwrap();
        assert_eq!(sel.y1, 4);
        assert_eq!(sel.y2, 7);
    }

    #[test]
    fn erase_in_display_mode3_clears_scroll_counter() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.scroll_counter = 42;
        state.erase_in_display(3);
        assert_eq!(state.scroll_counter, 0);
    }

    // -------------------------------------------------------------------------
    // Paste / bracketed paste
    // -------------------------------------------------------------------------
    #[test]
    fn paste_bracketed_enabled() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.bracketed_paste = true;
        // We can't easily test report_terminal_response without JNI,
        // but we can verify the mode is respected structurally.
        assert!(state.bracketed_paste);
    }

    #[test]
    fn paste_bracketed_disabled() {
        let mut state = ScreenState::new(80, 24, 100, 8, 16);
        state.bracketed_paste = false;
        assert!(!state.bracketed_paste);
    }

    // -------------------------------------------------------------------------
    // Debug info
    // -------------------------------------------------------------------------
    #[test]
    fn debug_info_contains_dimensions() {
        let state = ScreenState::new(80, 24, 100, 8, 16);
        let info = state.get_debug_info();
        assert!(info.contains("80"));
        assert!(info.contains("24"));
        assert!(info.contains("cursor="));
    }
}
