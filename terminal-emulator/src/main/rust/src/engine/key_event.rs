/// 按键事件处理 - 生成转义序列
use crate::engine::state::ScreenState;
use crate::terminal::modes::*;

impl ScreenState {
    /// 处理按键事件 - 实现 KeyHandler.getCode() 的逻辑
    /// 返回生成的转义序列，由 Java 写入 PTY
    pub fn send_key_event(&mut self, key_code: i32, char_str: Option<String>, meta_state: i32) -> Option<String> {
        const KEYCODE_DPAD_UP: i32 = 19;
        const KEYCODE_DPAD_DOWN: i32 = 20;
        const KEYCODE_DPAD_LEFT: i32 = 21;
        const KEYCODE_DPAD_RIGHT: i32 = 22;
        const KEYCODE_DPAD_CENTER: i32 = 23;
        const KEYCODE_ENTER: i32 = 66;
        const KEYCODE_TAB: i32 = 61;
        const KEYCODE_DEL: i32 = 67;
        const KEYCODE_FORWARD_DEL: i32 = 112;
        const KEYCODE_INSERT: i32 = 124;
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
        const KEYCODE_ESCAPE: i32 = 111;
        const KEYCODE_NUMPAD_ENTER: i32 = 160;

        const KEYMOD_SHIFT: i32 = 0x20000000u32 as i32;
        const KEYMOD_ALT: i32 = 0x80000000u32 as i32;
        const KEYMOD_CTRL: i32 = 0x40000000u32 as i32;

        let shift_down = (meta_state & KEYMOD_SHIFT) != 0;
        let alt_down = (meta_state & KEYMOD_ALT) != 0;
        let ctrl_down = (meta_state & KEYMOD_CTRL) != 0;

        if let Some(ref s) = char_str {
            if !s.is_empty() {
                if alt_down && s.chars().count() == 1 {
                    return Some(format!("\x1b{}", s));
                }
                return Some(s.clone());
            }
        }

        let mut key_mode = 0;
        if shift_down { key_mode |= 1; }
        if alt_down { key_mode |= 2; }
        if ctrl_down { key_mode |= 4; }

        let cursor_app = self.application_cursor_keys;
        let keypad_application = self.modes.is_enabled(DECSET_BIT_APPLICATION_KEYPAD);

        if self.kitty_keyboard_mode {
            let modifier = key_mode + 1;
            let mut cp: Option<u32> = None;
            
            match key_code {
                29..=54 => cp = Some((key_code - 29) as u32 + 'a' as u32),
                7..=16 => cp = Some((key_code - 7) as u32 + '0' as u32),
                62 => cp = Some(' ' as u32),
                55 => cp = Some(',' as u32),
                56 => cp = Some('.' as u32),
                69 => cp = Some('-' as u32),
                70 => cp = Some('=' as u32),
                71 => cp = Some('[' as u32),
                72 => cp = Some(']' as u32),
                73 => cp = Some('\\' as u32),
                74 => cp = Some(';' as u32),
                75 => cp = Some('\'' as u32),
                76 => cp = Some('/' as u32),
                68 => cp = Some('`' as u32),
                KEYCODE_ENTER | KEYCODE_DPAD_CENTER => cp = Some(13),
                KEYCODE_ESCAPE => cp = Some(27),
                KEYCODE_TAB => cp = Some(9),
                KEYCODE_DEL => cp = Some(127),
                _ => {}
            };
            
            if let Some(c) = cp {
                if key_mode > 0 || c < 32 || c == 127 {
                    return Some(format!("\x1b[{};{}u", c, modifier));
                }
            }
        }

        match key_code {
            KEYCODE_DPAD_CENTER | KEYCODE_ENTER => {
                if alt_down { Some("\x1b\r".to_string()) } else { Some("\r".to_string()) }
            },
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
                    Some(if cursor_app { "\x1bOD".to_string() } else { "\x1b[D".to_string() })
                } else {
                    Some(self.transform_for_modifiers("\x1b[1", key_mode, 'D'))
                }
            },
            KEYCODE_DPAD_RIGHT => {
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOC".to_string() } else { "\x1b[C".to_string() })
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
            KEYCODE_TAB => {
                if shift_down { Some("\x1b[Z".to_string()) } else { Some("\t".to_string()) }
            },
            KEYCODE_ESCAPE => Some("\x1b".to_string()),
            KEYCODE_DEL => {
                let prefix = if alt_down { "\x1b" } else { "" };
                Some(format!("{}{}", prefix, if ctrl_down { "\x08" } else { "\x7f" }))
            },
            KEYCODE_FORWARD_DEL => Some(self.transform_for_modifiers("\x1b[3", key_mode, '~')),
            KEYCODE_INSERT => Some(self.transform_for_modifiers("\x1b[2", key_mode, '~')),
            KEYCODE_F1 => {
                if key_mode == 0 { Some("\x1bOP".to_string()) }
                else { Some(self.transform_for_modifiers("\x1b[1", key_mode, 'P')) }
            },
            KEYCODE_F2 => {
                if key_mode == 0 { Some("\x1bOQ".to_string()) }
                else { Some(self.transform_for_modifiers("\x1b[1", key_mode, 'Q')) }
            },
            KEYCODE_F3 => {
                if key_mode == 0 { Some("\x1bOR".to_string()) }
                else { Some(self.transform_for_modifiers("\x1b[1", key_mode, 'R')) }
            },
            KEYCODE_F4 => {
                if key_mode == 0 { Some("\x1bOS".to_string()) }
                else { Some(self.transform_for_modifiers("\x1b[1", key_mode, 'S')) }
            },
            KEYCODE_F5 => Some(self.transform_for_modifiers("\x1b[15", key_mode, '~')),
            KEYCODE_F6 => Some(self.transform_for_modifiers("\x1b[17", key_mode, '~')),
            KEYCODE_F7 => Some(self.transform_for_modifiers("\x1b[18", key_mode, '~')),
            KEYCODE_F8 => Some(self.transform_for_modifiers("\x1b[19", key_mode, '~')),
            KEYCODE_F9 => Some(self.transform_for_modifiers("\x1b[20", key_mode, '~')),
            KEYCODE_F10 => Some(self.transform_for_modifiers("\x1b[21", key_mode, '~')),
            KEYCODE_F11 => Some(self.transform_for_modifiers("\x1b[23", key_mode, '~')),
            KEYCODE_F12 => Some(self.transform_for_modifiers("\x1b[24", key_mode, '~')),
            
            KEYCODE_NUMPAD_ENTER => {
                if keypad_application {
                    Some(self.transform_for_modifiers("\x1bO", key_mode, 'M'))
                } else {
                    Some("\n".to_string())
                }
            },
            _ => None,
        }
    }

    /// 根据修饰符转换转义序列
    /// 1 = none, 2 = shift, 3 = alt, 4 = shift+alt, 5 = ctrl, 6 = ctrl+shift, 7 = ctrl+alt, 8 = ctrl+shift+alt
    fn transform_for_modifiers(&self, base: &str, key_mode: i32, suffix: char) -> String {
        if key_mode == 0 {
            return format!("{}{}", base, suffix);
        }
        let modifier = key_mode + 1;
        format!("{};{}{}", base, modifier, suffix)
    }
}
