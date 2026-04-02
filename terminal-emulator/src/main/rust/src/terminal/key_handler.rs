//! KeyHandler - 键盘按键处理
//! 
//! 将 Android KeyEvent 转换为终端转义序列
//! 替代 Java: KeyHandler.java (373 行)

// 键位修饰符标志
pub const KEYMOD_ALT: u32 = 0x80000000;
pub const KEYMOD_CTRL: u32 = 0x40000000;
pub const KEYMOD_SHIFT: u32 = 0x20000000;
pub const KEYMOD_NUM_LOCK: u32 = 0x10000000;

// Android KeyEvent 键值常量
pub const KEYCODE_BACK: i32 = 4;
pub const KEYCODE_BREAK: i32 = 121;
pub const KEYCODE_DEL: i32 = 67;
pub const KEYCODE_DPAD_CENTER: i32 = 23;
pub const KEYCODE_DPAD_DOWN: i32 = 20;
pub const KEYCODE_DPAD_LEFT: i32 = 21;
pub const KEYCODE_DPAD_RIGHT: i32 = 22;
pub const KEYCODE_DPAD_UP: i32 = 19;
pub const KEYCODE_ENTER: i32 = 66;
pub const KEYCODE_ESCAPE: i32 = 111;
pub const KEYCODE_F1: i32 = 131;
pub const KEYCODE_F10: i32 = 140;
pub const KEYCODE_F11: i32 = 141;
pub const KEYCODE_F12: i32 = 142;
pub const KEYCODE_F2: i32 = 132;
pub const KEYCODE_F3: i32 = 133;
pub const KEYCODE_F4: i32 = 134;
pub const KEYCODE_F5: i32 = 135;
pub const KEYCODE_F6: i32 = 136;
pub const KEYCODE_F7: i32 = 137;
pub const KEYCODE_F8: i32 = 138;
pub const KEYCODE_F9: i32 = 139;
pub const KEYCODE_FORWARD_DEL: i32 = 112;
pub const KEYCODE_INSERT: i32 = 124;
pub const KEYCODE_MOVE_END: i32 = 123;
pub const KEYCODE_MOVE_HOME: i32 = 122;
pub const KEYCODE_NUMPAD_0: i32 = 144;
pub const KEYCODE_NUMPAD_1: i32 = 145;
pub const KEYCODE_NUMPAD_2: i32 = 146;
pub const KEYCODE_NUMPAD_3: i32 = 147;
pub const KEYCODE_NUMPAD_4: i32 = 148;
pub const KEYCODE_NUMPAD_5: i32 = 149;
pub const KEYCODE_NUMPAD_6: i32 = 150;
pub const KEYCODE_NUMPAD_7: i32 = 151;
pub const KEYCODE_NUMPAD_8: i32 = 152;
pub const KEYCODE_NUMPAD_9: i32 = 153;
pub const KEYCODE_NUMPAD_ADD: i32 = 157;
pub const KEYCODE_NUMPAD_COMMA: i32 = 159;
pub const KEYCODE_NUMPAD_DIVIDE: i32 = 154;
pub const KEYCODE_NUMPAD_DOT: i32 = 158;
pub const KEYCODE_NUMPAD_ENTER: i32 = 160;
pub const KEYCODE_NUMPAD_EQUALS: i32 = 161;
pub const KEYCODE_NUMPAD_MULTIPLY: i32 = 155;
pub const KEYCODE_NUMPAD_SUBTRACT: i32 = 156;
pub const KEYCODE_NUM_LOCK: i32 = 143;
pub const KEYCODE_PAGE_DOWN: i32 = 93;
pub const KEYCODE_PAGE_UP: i32 = 92;
pub const KEYCODE_SPACE: i32 = 62;
pub const KEYCODE_SYSRQ: i32 = 120;
pub const KEYCODE_TAB: i32 = 61;

use std::collections::HashMap;
use std::sync::OnceLock;

// Termcap 到 KeyCode 的映射表
static TERMCAP_TO_KEYCODE: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();

fn init_termcap_map() -> HashMap<&'static str, u32> {
    let mut map = HashMap::new();
    
    // terminfo: http://pubs.opengroup.org/onlinepubs/7990989799/xcurses/terminfo.html
    // termcap: http://man7.org/linux/man-pages/man5/termcap.5.html
    map.insert("%i", KEYMOD_SHIFT | KEYCODE_DPAD_RIGHT as u32);
    map.insert("#2", KEYMOD_SHIFT | KEYCODE_MOVE_HOME as u32); // Shifted home
    map.insert("#4", KEYMOD_SHIFT | KEYCODE_DPAD_LEFT as u32);
    map.insert("*7", KEYMOD_SHIFT | KEYCODE_MOVE_END as u32); // Shifted end key

    map.insert("k1", KEYCODE_F1 as u32);
    map.insert("k2", KEYCODE_F2 as u32);
    map.insert("k3", KEYCODE_F3 as u32);
    map.insert("k4", KEYCODE_F4 as u32);
    map.insert("k5", KEYCODE_F5 as u32);
    map.insert("k6", KEYCODE_F6 as u32);
    map.insert("k7", KEYCODE_F7 as u32);
    map.insert("k8", KEYCODE_F8 as u32);
    map.insert("k9", KEYCODE_F9 as u32);
    map.insert("k;", KEYCODE_F10 as u32);
    map.insert("F1", KEYCODE_F11 as u32);
    map.insert("F2", KEYCODE_F12 as u32);
    map.insert("F3", KEYMOD_SHIFT | KEYCODE_F1 as u32);
    map.insert("F4", KEYMOD_SHIFT | KEYCODE_F2 as u32);
    map.insert("F5", KEYMOD_SHIFT | KEYCODE_F3 as u32);
    map.insert("F6", KEYMOD_SHIFT | KEYCODE_F4 as u32);
    map.insert("F7", KEYMOD_SHIFT | KEYCODE_F5 as u32);
    map.insert("F8", KEYMOD_SHIFT | KEYCODE_F6 as u32);
    map.insert("F9", KEYMOD_SHIFT | KEYCODE_F7 as u32);
    map.insert("FA", KEYMOD_SHIFT | KEYCODE_F8 as u32);
    map.insert("FB", KEYMOD_SHIFT | KEYCODE_F9 as u32);
    map.insert("FC", KEYMOD_SHIFT | KEYCODE_F10 as u32);
    map.insert("FD", KEYMOD_SHIFT | KEYCODE_F11 as u32);
    map.insert("FE", KEYMOD_SHIFT | KEYCODE_F12 as u32);

    map.insert("kb", KEYCODE_DEL as u32); // backspace key
    map.insert("kd", KEYCODE_DPAD_DOWN as u32);
    map.insert("kh", KEYCODE_MOVE_HOME as u32);
    map.insert("kl", KEYCODE_DPAD_LEFT as u32);
    map.insert("kr", KEYCODE_DPAD_RIGHT as u32);
    map.insert("ku", KEYCODE_DPAD_UP as u32);

    // K1=Upper left of keypad
    map.insert("K1", KEYCODE_MOVE_HOME as u32);
    map.insert("K3", KEYCODE_PAGE_UP as u32);
    map.insert("K4", KEYCODE_MOVE_END as u32);
    map.insert("K5", KEYCODE_PAGE_DOWN as u32);

    map.insert("kB", KEYMOD_SHIFT | KEYCODE_TAB as u32); // Back-tab
    map.insert("kD", KEYCODE_FORWARD_DEL as u32);
    map.insert("kDN", KEYMOD_SHIFT | KEYCODE_DPAD_DOWN as u32);
    map.insert("kF", KEYMOD_SHIFT | KEYCODE_DPAD_DOWN as u32);
    map.insert("kI", KEYCODE_INSERT as u32);
    map.insert("kN", KEYCODE_PAGE_UP as u32);
    map.insert("kP", KEYCODE_PAGE_DOWN as u32);
    map.insert("kR", KEYMOD_SHIFT | KEYCODE_DPAD_UP as u32);
    map.insert("kUP", KEYMOD_SHIFT | KEYCODE_DPAD_UP as u32);

    map.insert("@7", KEYCODE_MOVE_END as u32);
    map.insert("@8", KEYCODE_NUMPAD_ENTER as u32);
    
    map
}

fn get_termcap_map() -> &'static HashMap<&'static str, u32> {
    TERMCAP_TO_KEYCODE.get_or_init(init_termcap_map)
}

/// 从 termcap 获取转义序列
pub fn get_code_from_termcap(
    termcap: &str,
    cursor_keys_application: bool,
    keypad_application: bool,
) -> Option<String> {
    let map = get_termcap_map();
    let &key_code_and_mod = map.get(termcap)?;
    
    let mut key_code = key_code_and_mod;
    let mut key_mod = 0u32;
    
    // 提取修饰符
    if (key_code & KEYMOD_SHIFT) != 0 {
        key_mod |= KEYMOD_SHIFT;
        key_code &= !KEYMOD_SHIFT;
    }
    if (key_code & KEYMOD_CTRL) != 0 {
        key_mod |= KEYMOD_CTRL;
        key_code &= !KEYMOD_CTRL;
    }
    if (key_code & KEYMOD_ALT) != 0 {
        key_mod |= KEYMOD_ALT;
        key_code &= !KEYMOD_ALT;
    }
    if (key_code & KEYMOD_NUM_LOCK) != 0 {
        key_mod |= KEYMOD_NUM_LOCK;
        key_code &= !KEYMOD_NUM_LOCK;
    }
    
    get_code(key_code as i32, key_mod, cursor_keys_application, keypad_application)
}

/// 根据键值和修饰符生成转义序列
pub fn get_code(
    key_code: i32,
    key_mode: u32,
    cursor_app: bool,
    keypad_application: bool,
) -> Option<String> {
    let num_lock_on = (key_mode & KEYMOD_NUM_LOCK) != 0;
    let key_mode = key_mode & !KEYMOD_NUM_LOCK;
    
    match key_code {
        KEYCODE_DPAD_CENTER => Some("\r".to_string()),
        
        KEYCODE_DPAD_UP => {
            if key_mode == 0 {
                Some(if cursor_app { "\x1bOA".to_string() } else { "\x1b[A".to_string() })
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'A'))
            }
        }
        KEYCODE_DPAD_DOWN => {
            if key_mode == 0 {
                Some(if cursor_app { "\x1bOB".to_string() } else { "\x1b[B".to_string() })
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'B'))
            }
        }
        KEYCODE_DPAD_RIGHT => {
            if key_mode == 0 {
                Some(if cursor_app { "\x1bOC".to_string() } else { "\x1b[C".to_string() })
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'C'))
            }
        }
        KEYCODE_DPAD_LEFT => {
            if key_mode == 0 {
                Some(if cursor_app { "\x1bOD".to_string() } else { "\x1b[D".to_string() })
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'D'))
            }
        }
        
        KEYCODE_MOVE_HOME => {
            if key_mode == 0 {
                Some(if cursor_app { "\x1bOH".to_string() } else { "\x1b[H".to_string() })
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'H'))
            }
        }
        KEYCODE_MOVE_END => {
            if key_mode == 0 {
                Some(if cursor_app { "\x1bOF".to_string() } else { "\x1b[F".to_string() })
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'F'))
            }
        }
        
        // F1-F4: vt100 兼容模式
        KEYCODE_F1 => {
            if key_mode == 0 {
                Some("\x1bOP".to_string())
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'P'))
            }
        }
        KEYCODE_F2 => {
            if key_mode == 0 {
                Some("\x1bOQ".to_string())
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'Q'))
            }
        }
        KEYCODE_F3 => {
            if key_mode == 0 {
                Some("\x1bOR".to_string())
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'R'))
            }
        }
        KEYCODE_F4 => {
            if key_mode == 0 {
                Some("\x1bOS".to_string())
            } else {
                Some(transform_for_modifiers("\x1b[1", key_mode, 'S'))
            }
        }
        KEYCODE_F5 => Some(transform_for_modifiers("\x1b[15", key_mode, '~')),
        KEYCODE_F6 => Some(transform_for_modifiers("\x1b[17", key_mode, '~')),
        KEYCODE_F7 => Some(transform_for_modifiers("\x1b[18", key_mode, '~')),
        KEYCODE_F8 => Some(transform_for_modifiers("\x1b[19", key_mode, '~')),
        KEYCODE_F9 => Some(transform_for_modifiers("\x1b[20", key_mode, '~')),
        KEYCODE_F10 => Some(transform_for_modifiers("\x1b[21", key_mode, '~')),
        KEYCODE_F11 => Some(transform_for_modifiers("\x1b[23", key_mode, '~')),
        KEYCODE_F12 => Some(transform_for_modifiers("\x1b[24", key_mode, '~')),
        
        KEYCODE_SYSRQ => Some("\x1b[32~".to_string()), // Sys Request / Print
        KEYCODE_BREAK => Some("\x1b[34~".to_string()), // Pause/Break
        
        KEYCODE_ESCAPE | KEYCODE_BACK => Some("\x1b".to_string()),
        
        KEYCODE_INSERT => Some(transform_for_modifiers("\x1b[2", key_mode, '~')),
        KEYCODE_FORWARD_DEL => Some(transform_for_modifiers("\x1b[3", key_mode, '~')),
        
        KEYCODE_PAGE_UP => Some(transform_for_modifiers("\x1b[5", key_mode, '~')),
        KEYCODE_PAGE_DOWN => Some(transform_for_modifiers("\x1b[6", key_mode, '~')),
        
        KEYCODE_DEL => {
            let prefix = if (key_mode & KEYMOD_ALT) == 0 { "" } else { "\x1b" };
            // xterm 和 gnome-terminal 的行为
            Some(format!("{}{}", prefix, if (key_mode & KEYMOD_CTRL) == 0 { "\x7f" } else { "\x08" }))
        }
        
        KEYCODE_NUM_LOCK => {
            if keypad_application {
                Some("\x1bOP".to_string())
            } else {
                None
            }
        }
        
        KEYCODE_SPACE => {
            // 如果没有 Ctrl，返回 None 让正常输入处理
            if (key_mode & KEYMOD_CTRL) == 0 {
                None
            } else {
                Some("\0".to_string())
            }
        }
        
        KEYCODE_TAB => {
            // Shifted 是 back-tab
            if (key_mode & KEYMOD_SHIFT) == 0 {
                Some("\t".to_string())
            } else {
                Some("\x1b[Z".to_string())
            }
        }
        
        KEYCODE_ENTER => {
            if (key_mode & KEYMOD_ALT) == 0 {
                Some("\r".to_string())
            } else {
                Some("\x1b\r".to_string())
            }
        }
        
        KEYCODE_NUMPAD_ENTER => {
            if keypad_application {
                Some(transform_for_modifiers("\x1bO", key_mode, 'M'))
            } else {
                Some("\n".to_string())
            }
        }
        KEYCODE_NUMPAD_MULTIPLY => {
            if keypad_application {
                Some(transform_for_modifiers("\x1bO", key_mode, 'j'))
            } else {
                Some("*".to_string())
            }
        }
        KEYCODE_NUMPAD_ADD => {
            if keypad_application {
                Some(transform_for_modifiers("\x1bO", key_mode, 'k'))
            } else {
                Some("+".to_string())
            }
        }
        KEYCODE_NUMPAD_COMMA => Some(",".to_string()),
        KEYCODE_NUMPAD_DOT => {
            if num_lock_on {
                if keypad_application {
                    Some("\x1bOn".to_string())
                } else {
                    Some(".".to_string())
                }
            } else {
                // DELETE
                Some(transform_for_modifiers("\x1b[3", key_mode, '~'))
            }
        }
        KEYCODE_NUMPAD_SUBTRACT => {
            if keypad_application {
                Some(transform_for_modifiers("\x1bO", key_mode, 'm'))
            } else {
                Some("-".to_string())
            }
        }
        KEYCODE_NUMPAD_DIVIDE => {
            if keypad_application {
                Some(transform_for_modifiers("\x1bO", key_mode, 'o'))
            } else {
                Some("/".to_string())
            }
        }
        KEYCODE_NUMPAD_0 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 'p'))
                } else {
                    Some("0".to_string())
                }
            } else {
                // INSERT
                Some(transform_for_modifiers("\x1b[2", key_mode, '~'))
            }
        }
        KEYCODE_NUMPAD_1 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 'q'))
                } else {
                    Some("1".to_string())
                }
            } else {
                // END
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOF".to_string() } else { "\x1b[F".to_string() })
                } else {
                    Some(transform_for_modifiers("\x1b[1", key_mode, 'F'))
                }
            }
        }
        KEYCODE_NUMPAD_2 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 'r'))
                } else {
                    Some("2".to_string())
                }
            } else {
                // DOWN
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOB".to_string() } else { "\x1b[B".to_string() })
                } else {
                    Some(transform_for_modifiers("\x1b[1", key_mode, 'B'))
                }
            }
        }
        KEYCODE_NUMPAD_3 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 's'))
                } else {
                    Some("3".to_string())
                }
            } else {
                // PGDN
                Some("\x1b[6~".to_string())
            }
        }
        KEYCODE_NUMPAD_4 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 't'))
                } else {
                    Some("4".to_string())
                }
            } else {
                // LEFT
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOD".to_string() } else { "\x1b[D".to_string() })
                } else {
                    Some(transform_for_modifiers("\x1b[1", key_mode, 'D'))
                }
            }
        }
        KEYCODE_NUMPAD_5 => {
            if keypad_application {
                Some(transform_for_modifiers("\x1bO", key_mode, 'u'))
            } else {
                Some("5".to_string())
            }
        }
        KEYCODE_NUMPAD_6 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 'v'))
                } else {
                    Some("6".to_string())
                }
            } else {
                // RIGHT
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOC".to_string() } else { "\x1b[C".to_string() })
                } else {
                    Some(transform_for_modifiers("\x1b[1", key_mode, 'C'))
                }
            }
        }
        KEYCODE_NUMPAD_7 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 'w'))
                } else {
                    Some("7".to_string())
                }
            } else {
                // HOME
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOH".to_string() } else { "\x1b[H".to_string() })
                } else {
                    Some(transform_for_modifiers("\x1b[1", key_mode, 'H'))
                }
            }
        }
        KEYCODE_NUMPAD_8 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 'x'))
                } else {
                    Some("8".to_string())
                }
            } else {
                // UP
                if key_mode == 0 {
                    Some(if cursor_app { "\x1bOA".to_string() } else { "\x1b[A".to_string() })
                } else {
                    Some(transform_for_modifiers("\x1b[1", key_mode, 'A'))
                }
            }
        }
        KEYCODE_NUMPAD_9 => {
            if num_lock_on {
                if keypad_application {
                    Some(transform_for_modifiers("\x1bO", key_mode, 'y'))
                } else {
                    Some("9".to_string())
                }
            } else {
                // PGUP
                Some("\x1b[5~".to_string())
            }
        }
        KEYCODE_NUMPAD_EQUALS => {
            if keypad_application {
                Some(transform_for_modifiers("\x1bO", key_mode, 'X'))
            } else {
                Some("=".to_string())
            }
        }
        
        _ => None,
    }
}

/// 根据修饰符转换转义序列
///
/// 例如：\x1b[1 + ; + modifier + lastChar
/// Ctrl+Shift+↑ = \x1b[1;6A
fn transform_for_modifiers(start: &str, keymod: u32, last_char: char) -> String {
    // 使用位运算计算 modifier 值
    // 1 = none, 2 = shift, 3 = alt, 4 = shift+alt, 5 = ctrl, 6 = ctrl+shift, 7 = ctrl+alt, 8 = ctrl+shift+alt
    let mut modifier = 1u32;
    if (keymod & KEYMOD_SHIFT) != 0 { modifier += 1; }
    if (keymod & KEYMOD_ALT) != 0 { modifier += 2; }
    if (keymod & KEYMOD_CTRL) != 0 { modifier += 4; }
    
    if modifier == 1 {
        format!("{}{}", start, last_char)
    } else {
        format!("{};{}{}", start, modifier, last_char)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_arrow_keys() {
        // 无修饰符的方向键
        assert_eq!(get_code(KEYCODE_DPAD_UP, 0, false, false), Some("\x1b[A".to_string()));
        assert_eq!(get_code(KEYCODE_DPAD_DOWN, 0, false, false), Some("\x1b[B".to_string()));
        assert_eq!(get_code(KEYCODE_DPAD_RIGHT, 0, false, false), Some("\x1b[C".to_string()));
        assert_eq!(get_code(KEYCODE_DPAD_LEFT, 0, false, false), Some("\x1b[D".to_string()));
    }
    
    #[test]
    fn test_cursor_application_mode() {
        // 应用光标模式
        assert_eq!(get_code(KEYCODE_DPAD_UP, 0, true, false), Some("\x1bOA".to_string()));
        assert_eq!(get_code(KEYCODE_DPAD_DOWN, 0, true, false), Some("\x1bOB".to_string()));
    }
    
    #[test]
    fn test_function_keys() {
        // F1-F4 基础
        assert_eq!(get_code(KEYCODE_F1, 0, false, false), Some("\x1bOP".to_string()));
        assert_eq!(get_code(KEYCODE_F2, 0, false, false), Some("\x1bOQ".to_string()));
        assert_eq!(get_code(KEYCODE_F3, 0, false, false), Some("\x1bOR".to_string()));
        assert_eq!(get_code(KEYCODE_F4, 0, false, false), Some("\x1bOS".to_string()));
        
        // F5-F12
        assert_eq!(get_code(KEYCODE_F5, 0, false, false), Some("\x1b[15~".to_string()));
        assert_eq!(get_code(KEYCODE_F12, 0, false, false), Some("\x1b[24~".to_string()));
    }
    
    #[test]
    fn test_modifiers() {
        // Shift + ↑
        assert_eq!(get_code(KEYCODE_DPAD_UP, KEYMOD_SHIFT, false, false), Some("\x1b[1;2A".to_string()));
        // Ctrl + ↑
        assert_eq!(get_code(KEYCODE_DPAD_UP, KEYMOD_CTRL, false, false), Some("\x1b[1;5A".to_string()));
        // Alt + ↑
        assert_eq!(get_code(KEYCODE_DPAD_UP, KEYMOD_ALT, false, false), Some("\x1b[1;3A".to_string()));
        // Ctrl + Shift + ↑ (modifier 6)
        assert_eq!(get_code(KEYCODE_DPAD_UP, KEYMOD_CTRL | KEYMOD_SHIFT, false, false), Some("\x1b[1;6A".to_string()));
        // Alt + Shift + ↑ (modifier 4)
        assert_eq!(get_code(KEYCODE_DPAD_UP, KEYMOD_ALT | KEYMOD_SHIFT, false, false), Some("\x1b[1;4A".to_string()));
        // Alt + Ctrl + ↑ (modifier 7)
        assert_eq!(get_code(KEYCODE_DPAD_UP, KEYMOD_ALT | KEYMOD_CTRL, false, false), Some("\x1b[1;7A".to_string()));
        // Alt + Ctrl + Shift + ↑ (modifier 8)
        assert_eq!(get_code(KEYCODE_DPAD_UP, KEYMOD_ALT | KEYMOD_CTRL | KEYMOD_SHIFT, false, false), Some("\x1b[1;8A".to_string()));
    }
    
    #[test]
    fn test_special_keys() {
        assert_eq!(get_code(KEYCODE_DEL, 0, false, false), Some("\x7f".to_string()));
        assert_eq!(get_code(KEYCODE_DEL, KEYMOD_CTRL, false, false), Some("\x08".to_string()));
        assert_eq!(get_code(KEYCODE_INSERT, 0, false, false), Some("\x1b[2~".to_string()));
        assert_eq!(get_code(KEYCODE_FORWARD_DEL, 0, false, false), Some("\x1b[3~".to_string()));
        assert_eq!(get_code(KEYCODE_PAGE_UP, 0, false, false), Some("\x1b[5~".to_string()));
        assert_eq!(get_code(KEYCODE_PAGE_DOWN, 0, false, false), Some("\x1b[6~".to_string()));
    }
    
    #[test]
    fn test_termcap() {
        assert_eq!(get_code_from_termcap("k1", false, false), Some("\x1bOP".to_string()));
        assert_eq!(get_code_from_termcap("kd", false, false), Some("\x1b[B".to_string()));
        assert_eq!(get_code_from_termcap("kb", false, false), Some("\x7f".to_string()));
    }
}
