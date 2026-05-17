/// 按键事件处理 - 生成转义序列
use crate::engine::state::ScreenState;
use crate::terminal::key_handler;
use crate::terminal::modes::*;

impl ScreenState {
    /// 处理按键事件 - 实现 KeyHandler.getCode() 的逻辑
    /// 返回生成的转义序列，由 Java 写入 PTY
    pub fn send_key_event(
        &mut self,
        key_code: i32,
        char_str: Option<String>,
        meta_state: i32,
    ) -> Option<String> {
        let cursor_app = self.application_cursor_keys;
        let keypad_application = self.modes.is_enabled(DECSET_BIT_APPLICATION_KEYPAD);

        // 优先使用 key_handler 处理特殊功能键
        let code =
            key_handler::get_code(key_code, meta_state as u32, cursor_app, keypad_application);

        if code.is_some() {
            return code;
        }

        // 如果 key_handler 没处理，且有字符输入，则返回 None 让 Java 层处理为普通字符输入
        // 或者在这里处理 Alt+Char 逻辑
        if let Some(s) = char_str {
            if !s.is_empty() {
                const KEYMOD_ALT: i32 = 0x80000000u32 as i32;
                if (meta_state & KEYMOD_ALT) != 0 {
                    return Some(format!("\x1b{}", s));
                }
            }
        }

        None
    }
}
