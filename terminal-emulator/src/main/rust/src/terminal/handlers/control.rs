use std::cmp::{max};
use crate::engine::ScreenState;

pub fn handle_control(state: &mut ScreenState, byte: u8) -> bool {
    match byte {
        0x00 => true, // NUL - 忽略
        0x07 => {
            state.report_bell();
            true
        } // BEL - 响铃
        0x08 => {
            state.cursor.x = max(state.left_margin, state.cursor.x - 1);
            state.cursor.about_to_wrap = false;
            true
        } // BS
        0x09 => {
            state.cursor_forward_tab();
            state.cursor.about_to_wrap = false;
            true
        } // HT
        0x0a..=0x0c => {
            // LF, VT, FF
            // 标准 VT100 行为：LF 只下移一行，不回到行首（与 upstream Java 一致）
            // Shell 通常发送 \r\n 来同时完成回车和换行
            // LNM (DECSET 20) 模式未实现，因此 LF 永远不包含隐式 CR
            state.cursor.about_to_wrap = false;
            if state.cursor.y < state.bottom_margin - 1 {
                state.cursor.y += 1;
            } else {
                state.scroll_up();
            }
            true
        }
        0x0d => {
            state.cursor.x = state.left_margin;
            state.cursor.about_to_wrap = false;
            true
        } // CR
        0x0e => {
            // SO (Shift Out) - 切换到 G1 字符集
            state.use_line_drawing_uses_g0 = false;
            true
        }
        0x0f => {
            // SI (Shift In) - 切换到 G0 字符集
            state.use_line_drawing_uses_g0 = true;
            true
        }
        _ => false,
    }
}
