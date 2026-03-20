use crate::terminal::style::STYLE_NORMAL;
use std::cmp::{max, min};

#[derive(Clone, Copy)]
pub struct CursorState {
    pub x: i32,
    pub y: i32,
    pub style: u64,
    pub about_to_wrap: bool,
    pub decset_flags: i32,
    pub use_line_drawing_g0: bool,
    pub use_line_drawing_g1: bool,
    pub use_line_drawing_uses_g0: bool,
    pub fore_color: u64,
    pub back_color: u64,
}

pub struct Cursor {
    pub x: i32,
    pub y: i32,
    pub about_to_wrap: bool,
    pub style: i32, // 0=block, 1=underline, 2=bar
    pub blinking_enabled: bool,
    pub blink_state: bool,
    
    // 保存的状态栈（对应 DECSC/DECRC）
    pub saved_state: CursorState,
}

impl Cursor {
    pub fn new() -> Self {
        let default_state = CursorState {
            x: 0, y: 0,
            style: STYLE_NORMAL,
            about_to_wrap: false,
            decset_flags: 0,
            use_line_drawing_g0: false,
            use_line_drawing_g1: false,
            use_line_drawing_uses_g0: true,
            fore_color: 256,
            back_color: 257,
        };

        Self {
            x: 0,
            y: 0,
            about_to_wrap: false,
            style: 0,
            blinking_enabled: false,
            blink_state: true,
            saved_state: default_state,
        }
    }

    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.about_to_wrap = false;
    }

    pub fn clamp(&mut self, cols: i32, rows: i32) {
        self.x = max(0, min(cols - 1, self.x));
        self.y = max(0, min(rows - 1, self.y));
    }

    pub fn move_relative(&mut self, dx: i32, dy: i32, cols: i32, rows: i32) {
        self.x = max(0, min(cols - 1, self.x + dx));
        self.y = max(0, min(rows - 1, self.y + dy));
        self.about_to_wrap = false;
    }

    pub fn save_state(&mut self, current_style: u64, decset_flags: i32, g0: bool, g1: bool, uses_g0: bool, fg: u64, bg: u64) {
        self.saved_state = CursorState {
            x: self.x,
            y: self.y,
            style: current_style,
            about_to_wrap: self.about_to_wrap,
            decset_flags,
            use_line_drawing_g0: g0,
            use_line_drawing_g1: g1,
            use_line_drawing_uses_g0: uses_g0,
            fore_color: fg,
            back_color: bg,
        };
    }

    pub fn restore_state(&mut self) -> CursorState {
        let s = self.saved_state;
        self.x = s.x;
        self.y = s.y;
        self.about_to_wrap = s.about_to_wrap;
        s
    }
}
