/// DECSET 标志位定义（与 Java DECSET_BIT_* 常量一致）
pub const DECSET_BIT_APPLICATION_CURSOR_KEYS: i32 = 1;
pub const DECSET_BIT_REVERSE_VIDEO: i32 = 1 << 1;
pub const DECSET_BIT_ORIGIN_MODE: i32 = 1 << 2;
pub const DECSET_BIT_AUTOWRAP: i32 = 1 << 3;
pub const DECSET_BIT_CURSOR_ENABLED: i32 = 1 << 4;
pub const DECSET_BIT_APPLICATION_KEYPAD: i32 = 1 << 5;
pub const DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE: i32 = 1 << 6;
pub const DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT: i32 = 1 << 7;
pub const DECSET_BIT_SEND_FOCUS_EVENTS: i32 = 1 << 8;
pub const DECSET_BIT_MOUSE_PROTOCOL_SGR: i32 = 1 << 9;
pub const DECSET_BIT_BRACKETED_PASTE_MODE: i32 = 1 << 10;
pub const DECSET_BIT_LEFTRIGHT_MARGIN_MODE: i32 = 1 << 11;
pub const MODE_INSERT: i32 = 1 << 12;
pub const MODE_LNM: i32 = 1 << 13;

pub struct TerminalModes {
    pub flags: i32,
}

impl TerminalModes {
    pub fn new() -> Self {
        Self { flags: 0 }
    }

    pub fn set(&mut self, flag: i32) {
        self.flags |= flag;
    }

    pub fn reset(&mut self, flag: i32) {
        self.flags &= !flag;
    }

    pub fn is_enabled(&self, flag: i32) -> bool {
        (self.flags & flag) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_new_defaults() {
        let m = TerminalModes::new();
        assert_eq!(m.flags, 0);
    }

    #[test]
    fn test_set_and_reset_single_flag() {
        let mut m = TerminalModes::new();
        assert!(!m.is_enabled(DECSET_BIT_AUTOWRAP));

        m.set(DECSET_BIT_AUTOWRAP);
        assert!(m.is_enabled(DECSET_BIT_AUTOWRAP));

        m.reset(DECSET_BIT_AUTOWRAP);
        assert!(!m.is_enabled(DECSET_BIT_AUTOWRAP));
    }

    #[test]
    fn test_multiple_flags_independent() {
        let mut m = TerminalModes::new();
        m.set(DECSET_BIT_APPLICATION_CURSOR_KEYS);
        m.set(DECSET_BIT_REVERSE_VIDEO);
        m.set(DECSET_BIT_BRACKETED_PASTE_MODE);

        assert!(m.is_enabled(DECSET_BIT_APPLICATION_CURSOR_KEYS));
        assert!(m.is_enabled(DECSET_BIT_REVERSE_VIDEO));
        assert!(m.is_enabled(DECSET_BIT_BRACKETED_PASTE_MODE));
        assert!(!m.is_enabled(DECSET_BIT_AUTOWRAP));

        m.reset(DECSET_BIT_REVERSE_VIDEO);
        assert!(!m.is_enabled(DECSET_BIT_REVERSE_VIDEO));
        // Others unchanged
        assert!(m.is_enabled(DECSET_BIT_APPLICATION_CURSOR_KEYS));
        assert!(m.is_enabled(DECSET_BIT_BRACKETED_PASTE_MODE));
    }

    #[test]
    fn test_all_decset_flags() {
        let mut m = TerminalModes::new();
        let all = DECSET_BIT_APPLICATION_CURSOR_KEYS
            | DECSET_BIT_REVERSE_VIDEO
            | DECSET_BIT_ORIGIN_MODE
            | DECSET_BIT_AUTOWRAP
            | DECSET_BIT_CURSOR_ENABLED
            | DECSET_BIT_APPLICATION_KEYPAD
            | DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE
            | DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT
            | DECSET_BIT_SEND_FOCUS_EVENTS
            | DECSET_BIT_MOUSE_PROTOCOL_SGR
            | DECSET_BIT_BRACKETED_PASTE_MODE
            | DECSET_BIT_LEFTRIGHT_MARGIN_MODE;

        m.set(all);
        assert_eq!(m.flags, all);

        m.reset(all);
        assert_eq!(m.flags, 0);
    }

    #[test]
    fn test_flag_bit_values() {
        assert_eq!(DECSET_BIT_APPLICATION_CURSOR_KEYS, 1);
        assert_eq!(DECSET_BIT_REVERSE_VIDEO, 2);
        assert_eq!(DECSET_BIT_ORIGIN_MODE, 4);
        assert_eq!(DECSET_BIT_AUTOWRAP, 8);
        assert_eq!(DECSET_BIT_CURSOR_ENABLED, 16);
        assert_eq!(DECSET_BIT_APPLICATION_KEYPAD, 32);
        assert_eq!(DECSET_BIT_BRACKETED_PASTE_MODE, 1024);
    }

    #[test]
    fn test_is_enabled_zero_flag() {
        let m = TerminalModes::new();
        assert!(!m.is_enabled(0));
    }

    #[test]
    fn test_set_zero_flag() {
        let mut m = TerminalModes::new();
        m.set(DECSET_BIT_AUTOWRAP);
        m.set(0); // should not change anything
        assert!(m.is_enabled(DECSET_BIT_AUTOWRAP));
    }
}
