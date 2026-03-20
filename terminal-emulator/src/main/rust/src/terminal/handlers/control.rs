use std::cmp::{max, min};
use crate::engine::ScreenState;
use crate::terminal::modes;

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
            if state.cursor.y < state.bottom_margin - 1 {
                state.cursor.y += 1;
            } else {
                state.scroll_up();
            }
            state.cursor.about_to_wrap = false;
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
