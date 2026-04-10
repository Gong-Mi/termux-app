use crate::terminal::style::STYLE_NORMAL;
use crate::terminal::colors::{COLOR_INDEX_FOREGROUND, COLOR_INDEX_BACKGROUND};
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
            fore_color: COLOR_INDEX_FOREGROUND as u64,
            back_color: COLOR_INDEX_BACKGROUND as u64,
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

    pub fn should_be_visible(&self, cursor_enabled: bool) -> bool {
        if !cursor_enabled { return false; }
        if self.blinking_enabled { self.blink_state } else { true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_new_defaults() {
        let c = Cursor::new();
        assert_eq!(c.x, 0);
        assert_eq!(c.y, 0);
        assert_eq!(c.style, 0); // block
        assert!(!c.about_to_wrap);
        assert!(!c.blinking_enabled);
        assert!(c.blink_state); // starts visible
        assert_eq!(c.saved_state.fore_color, COLOR_INDEX_FOREGROUND as u64);
        assert_eq!(c.saved_state.back_color, COLOR_INDEX_BACKGROUND as u64);
    }

    #[test]
    fn test_set_position() {
        let mut c = Cursor::new();
        c.set_position(10, 5);
        assert_eq!(c.x, 10);
        assert_eq!(c.y, 5);
        assert!(!c.about_to_wrap);
    }

    #[test]
    fn test_clamp_within_bounds() {
        let mut c = Cursor::new();
        c.x = 5; c.y = 3;
        c.clamp(80, 24);
        assert_eq!(c.x, 5);
        assert_eq!(c.y, 3);
    }

    #[test]
    fn test_clamp_out_of_bounds() {
        let mut c = Cursor::new();
        c.x = 100; c.y = 50;
        c.clamp(80, 24);
        assert_eq!(c.x, 79);
        assert_eq!(c.y, 23);
    }

    #[test]
    fn test_clamp_negative() {
        let mut c = Cursor::new();
        c.x = -5; c.y = -3;
        c.clamp(80, 24);
        assert_eq!(c.x, 0);
        assert_eq!(c.y, 0);
    }

    #[test]
    fn test_move_relative() {
        let mut c = Cursor::new();
        c.x = 10; c.y = 5;
        c.move_relative(3, 2, 80, 24);
        assert_eq!(c.x, 13);
        assert_eq!(c.y, 7);
        assert!(!c.about_to_wrap);
    }

    #[test]
    fn test_move_relative_clamped() {
        let mut c = Cursor::new();
        c.x = 78; c.y = 23;
        c.move_relative(10, 10, 80, 24);
        assert_eq!(c.x, 79);
        assert_eq!(c.y, 23);
    }

    #[test]
    fn test_save_restore_state() {
        let mut c = Cursor::new();
        c.x = 20; c.y = 10; c.about_to_wrap = true;

        c.save_state(0x1234, 0xFF, true, false, true, 100, 200);

        c.x = 0; c.y = 0; c.about_to_wrap = false;

        let restored = c.restore_state();
        assert_eq!(c.x, 20);
        assert_eq!(c.y, 10);
        assert!(c.about_to_wrap);
        assert_eq!(restored.x, 20);
        assert_eq!(restored.y, 10);
        assert_eq!(restored.style, 0x1234);
        assert_eq!(restored.decset_flags, 0xFF);
        assert!(restored.use_line_drawing_g0);
        assert!(!restored.use_line_drawing_g1);
        assert!(restored.use_line_drawing_uses_g0);
        assert_eq!(restored.fore_color, 100);
        assert_eq!(restored.back_color, 200);
    }

    #[test]
    fn test_should_be_visible() {
        let c = Cursor::new();

        // 无闪烁：始终可见
        assert!(c.should_be_visible(true));
        assert!(!c.should_be_visible(false));

        // 闪烁启用
        let mut c2 = c;
        c2.blinking_enabled = true;
        c2.blink_state = true;
        assert!(c2.should_be_visible(true));

        c2.blink_state = false;
        assert!(!c2.should_be_visible(true));
    }

    #[test]
    fn test_cursor_styles() {
        let mut c = Cursor::new();
        assert_eq!(c.style, 0); // block

        c.style = 1; // underline
        assert_eq!(c.style, 1);

        c.style = 2; // bar
        assert_eq!(c.style, 2);
    }
}
