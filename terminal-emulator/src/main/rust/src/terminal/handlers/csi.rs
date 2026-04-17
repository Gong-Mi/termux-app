use std::cmp::{max, min};
use crate::engine::ScreenState;
use crate::vte_parser::Params;

/// 处理 CSI (Control Sequence Introducer) 序列
/// 参数默认值行为与 Java TerminalEmulator.getArg0()/getArg1() 保持一致
pub fn handle_csi(state: &mut ScreenState, params: &Params, intermediates: &[u8], action: char) {
    let is_private = intermediates.contains(&b'?');
    let is_gt = intermediates.contains(&b'>');
    let is_bang = intermediates.contains(&b'!');

    // 辅助逻辑：清理 wrap 标志（大多数 CSI 指令都会清理它）
    let mut clear_wrap = true;

    match action {
        '@' => {
            // ICH - Insert Character (默认 1)
            let n = params.get_arg0(1);
            state.insert_characters(n);
        }
        'A' => {
            // CUU - Cursor Up (默认 1)
            let dist = params.get_arg0(1);
            state.cursor.y = max(state.top_margin, state.cursor.y - dist);
        }
        'B' => {
            // CUD - Cursor Down (默认 1)
            let dist = params.get_arg0(1);
            state.cursor.y = min(state.bottom_margin - 1, state.cursor.y + dist);
        }
        'C' | 'a' => {
            // CUF - Cursor Forward (默认 1)
            let dist = params.get_arg0(1);
            state.cursor_horizontal_relative(dist);
        }
        'D' => {
            // CUB - Cursor Backward (默认 1)
            let dist = params.get_arg0(1);
            state.cursor.x = max(state.left_margin, state.cursor.x - dist);
        }
        'E' => {
            // CNL - Cursor Next Line (默认 1)
            let n = params.get_arg0(1);
            state.cursor_next_line(n);
        }
        'F' => {
            // CPL - Cursor Previous Line (默认 1)
            let n = params.get_arg0(1);
            state.cursor_previous_line(n);
        }
        'G' | '`' => {
            // CHA - Cursor Horizontal Absolute (默认 1)
            let n = params.get_arg0(1);
            state.cursor_horizontal_absolute(n);
        }
        'H' | 'f' => {
            // CUP - Cursor Position (默认 row=1, col=1)
            let row = params.get_arg0(1);
            let col = params.get_arg1(1);
            if state.origin_mode() {
                state.cursor.y = max(state.top_margin, min(state.bottom_margin - 1, state.top_margin + row - 1));
            } else {
                state.cursor.y = max(0, min(state.rows - 1, row - 1));
            }
            state.cursor.x = max(state.left_margin, min(state.right_margin - 1, col - 1));
        }
        'I' => {
            // CHT - Cursor Horizontal Tab (默认 1)
            let n = params.get_arg0(1);
            for _ in 0..n { state.cursor_forward_tab(); }
        }
        'J' => {
            if is_private {
                // DECSED - Selective Erase in Display (暂时按 ED 处理)
                let mode = params.get_arg0(0);
                state.erase_in_display(mode);
            } else {
                // ED - Erase in Display (默认 0)
                let mode = params.get_arg0(0);
                state.erase_in_display(mode);
            }
        }
        'K' => {
            if is_private {
                // DECSEL - Selective Erase in Line
                let mode = params.get_arg0(0);
                state.erase_in_line(mode);
            } else {
                // EL - Erase in Line (默认 0)
                let mode = params.get_arg0(0);
                state.erase_in_line(mode);
            }
        }
        'L' => {
            // IL - Insert Line (默认 1)
            let n = params.get_arg0(1);
            state.insert_lines(n);
        }
        'M' => {
            // DL - Delete Line (默认 1)
            let n = params.get_arg0(1);
            state.delete_lines(n);
        }
        'P' => {
            // DCH - Delete Character (默认 1)
            let n = params.get_arg0(1);
            state.delete_characters(n);
        }
        'S' => {
            // SU - Scroll Up (默认 1)
            let n = params.get_arg0(1);
            state.scroll_up_lines(n);
        }
        'T' => {
            if is_gt {
                // Ignore CSI > T
            } else {
                // SD - Scroll Down (默认 1)
                let n = params.get_arg0(1);
                state.scroll_down_lines(n);
            }
        }
        'X' => {
            // ECH - Erase Character (默认 1)
            let n = params.get_arg0(1);
            state.erase_characters(n);
        }
        'Z' => {
            // CBT - Cursor Backward Tab (默认 1)
            let n = params.get_arg0(1);
            state.cursor_backward_tab(n);
        }
        'b' => {
            // REP - Repeat (默认 1)
            let n = params.get_arg0(1);
            if let Some(c) = state.last_printed_char {
                state.repeat_character(n, c);
            }
        }
        'c' => {
            clear_wrap = false;
            if is_gt {
                // DA2 - Secondary Device Attributes
                state.report_terminal_response("\x1b[>41;320;0c");
            } else {
                // DA1 - Primary Device Attributes
                state.report_terminal_response("\x1b[?64;1;2;6;15;22c");
            }
        }
        'd' => {
            // VPA - Vertical Position Absolute (默认 1)
            let n = params.get_arg0(1);
            state.cursor_vertical_absolute(n);
        }
        'e' => {
            // VPR - Vertical Position Relative (默认 1)
            let n = params.get_arg0(1);
            state.cursor_vertical_relative(n);
        }
        'g' => {
            // TBC - Tab Clear (默认 0)
            let mode = params.get_arg0(0);
            state.clear_tab_stop(mode);
        }
        'h' => {
            clear_wrap = false;
            if is_private { state.handle_decset(params, true); }
            else { state.handle_set_mode(params, true); }
        }
        'l' => {
            clear_wrap = false;
            if is_private { state.handle_decset(params, false); }
            else { state.handle_set_mode(params, false); }
        }
        'm' => {
            clear_wrap = false;
            if is_gt {
                // xterm resource / keyboard mode, ignore safely
            } else {
                state.handle_sgr(params);
            }
        }
        'n' => {
            clear_wrap = false;
            if is_private {
                // DEC-specific DSR
                match params.get_arg0(-1) {
                    6 => { // DECXCPR - Extended Cursor Position
                        let r = state.cursor.y + 1;
                        let c = state.cursor.x + 1;
                        state.report_terminal_response(&format!("\x1b[?{};{};1R", r, c));
                    }
                    _ => {}
                }
            } else {
                // Standard DSR
                let mode = if params.len == 0 { -1 } else { params.get(0, 0) };
                match mode {
                    5 => state.report_terminal_response("\x1b[0n"),  // DSR Status Report
                    6 => {  // CPR - Cursor Position Report
                        let r = state.cursor.y + 1;
                        let c = state.cursor.x + 1;
                        state.report_terminal_response(&format!("\x1b[{};{}R", r, c));
                    }
                    _ => {}
                }
            }
        }
        'p' => { 
            if is_bang { 
                state.decstr_soft_reset(); 
            } else if is_gt {
                // xterm resource modification, ignore safely
            }
        }
        'q' => {
            if is_gt {
                // Secondary DA / Terminal Name query
                state.report_terminal_response("\x1bP>|Termux-Rust(0.1)\x1b\\");
            } else {
                // DECSCUSR - Set Cursor Style (1-6)
                let style = params.get_arg0(1);
                state.set_cursor_style(style);
            }
        }
        'r' => {
            // DECSTBM - Set Top and Bottom Margins (默认 top=1, bottom=rows)
            let top = params.get_arg0(1);
            let bottom = params.get_arg1(state.rows as i32);
            state.set_margins(top, bottom);
        }
        's' => {
            if state.leftright_margin_mode() {
                // DECSLRM - Set Left and Right Margins (默认 left=1, right=cols)
                let left = params.get_arg0(1);
                let right = params.get_arg1(state.cols as i32);
                state.set_left_right_margins(left, right);
            } else {
                state.save_cursor();
            }
        }
        'u' => { 
            if is_private {
                // Kitty Keyboard Protocol query (CSI ? u), ignore
            } else {
                state.restore_cursor(); 
            }
        }
        _ => { clear_wrap = false; }
    }

    if clear_wrap {
        state.cursor.about_to_wrap = false;
    }
}
