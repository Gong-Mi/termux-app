use std::cmp::{max, min};
use crate::engine::ScreenState;
use crate::vte_parser::Params;

pub fn handle_csi(state: &mut ScreenState, params: &Params, intermediates: &[u8], action: char) {
    let is_private = intermediates.contains(&b'?');
    let is_bang = intermediates.contains(&b'!');

    match action {
        '@' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.about_to_wrap = false;
            state.insert_characters(n);
        }
        'A' => {
            let dist = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.y = max(state.top_margin, state.cursor.y - dist);
            state.cursor.about_to_wrap = false;
        }
        'B' => {
            let dist = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.y = min(state.bottom_margin - 1, state.cursor.y + dist);
            state.cursor.about_to_wrap = false;
        }
        'C' | 'a' => {
            let dist = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor_horizontal_relative(dist);
        }
        'D' => {
            let dist = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.x = max(state.left_margin, state.cursor.x - dist);
            state.cursor.about_to_wrap = false;
        }
        'E' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor_next_line(n);
        }
        'F' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor_previous_line(n);
        }
        'G' | '`' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor_horizontal_absolute(n);
        }
        'H' | 'f' => {
            let mut iter = params.iter();
            let row = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            let col = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            if state.origin_mode() {
                state.cursor.y = max(state.top_margin, min(state.bottom_margin - 1, state.top_margin + row - 1));
            } else {
                state.cursor.y = max(0, min(state.rows - 1, row - 1));
            }
            state.cursor.x = max(state.left_margin, min(state.right_margin - 1, col - 1));
            state.cursor.about_to_wrap = false;
        }
        'I' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            for _ in 0..n { state.cursor_forward_tab(); }
        }
        'J' => {
            let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0) as i32;
            state.cursor.about_to_wrap = false;
            state.erase_in_display(mode);
        }
        'K' => {
            let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0) as i32;
            state.cursor.about_to_wrap = false;
            state.erase_in_line(mode);
        }
        'L' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.about_to_wrap = false;
            state.insert_lines(n);
        }
        'M' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.about_to_wrap = false;
            state.delete_lines(n);
        }
        'P' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.about_to_wrap = false;
            state.delete_characters(n);
        }
        'S' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.about_to_wrap = false;
            state.scroll_up_lines(n);
        }
        'T' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.about_to_wrap = false;
            state.scroll_down_lines(n);
        }
        'X' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor.about_to_wrap = false;
            state.erase_characters(n);
        }
        'Z' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor_backward_tab(n);
        }
        'b' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            if let Some(c) = state.last_printed_char {
                state.repeat_character(n, c);
            }
        }
        'c' => {
            state.report_terminal_response("\x1b[?6c");
        }
        'd' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor_vertical_absolute(n);
        }
        'e' => {
            let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            state.cursor_vertical_relative(n);
        }
        'g' => {
            let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0) as i32;
            state.clear_tab_stop(mode);
        }
        'h' => {
            if is_private { state.handle_decset(params, true); }
            else { state.handle_set_mode(params, true); }
        }
        'l' => {
            if is_private { state.handle_decset(params, false); }
            else { state.handle_set_mode(params, false); }
        }
        'm' => { state.handle_sgr(params); }
        'n' => {
            let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0) as i32;
            match mode {
                5 => state.report_terminal_response("\x1b[0n"),
                6 => {
                    let r = state.cursor.y + 1;
                    let c = state.cursor.x + 1;
                    state.report_terminal_response(&format!("\x1b[{};{}R", r, c));
                }
                _ => {}
            }
        }
        'p' => { if is_bang { state.decstr_soft_reset(); } }
        'r' => {
            let mut iter = params.iter();
            let top = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
            let bottom = iter.next().and_then(|p| p.first()).copied().unwrap_or(state.rows as i32) as i32;
            state.set_margins(top, bottom);
        }
        's' => {
            if state.leftright_margin_mode() {
                let mut iter = params.iter();
                let left = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                let right = iter.next().and_then(|p| p.first()).copied().unwrap_or(state.cols as i32) as i32;
                state.set_left_right_margins(left, right);
            } else {
                state.save_cursor();
            }
        }
        'u' => { state.restore_cursor(); }
        _ => {}
    }
}
