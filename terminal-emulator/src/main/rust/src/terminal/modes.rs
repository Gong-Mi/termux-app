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
