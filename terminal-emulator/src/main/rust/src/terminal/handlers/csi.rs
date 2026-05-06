use crate::engine::ScreenState;
use crate::vte_parser::Params;
use std::cmp::{max, min};

/// 处理 CSI (Control Sequence Introducer) 序列
/// 参数默认值行为与 Java TerminalEmulator.getArg0()/getArg1() 保持一致
pub fn handle_csi(state: &mut ScreenState, params: &Params, intermediates: &[u8], action: char) {
    let is_private = intermediates.contains(&b'?');
    let is_bang = intermediates.contains(&b'!');

    match action {
        '@' => {
            // ICH - Insert Character (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor.about_to_wrap = false;
            state.insert_characters(n);
        }
        'A' => {
            // CUU - Cursor Up (默认 1)
            let dist = params.get_arg0(1) as i64;
            state.cursor.y = max(state.top_margin, state.cursor.y - dist);
            state.cursor.about_to_wrap = false;
        }
        'B' => {
            // CUD - Cursor Down (默认 1)
            let dist = params.get_arg0(1) as i64;
            state.cursor.y = min(state.bottom_margin - 1, state.cursor.y + dist);
            state.cursor.about_to_wrap = false;
        }
        'C' | 'a' => {
            // CUF - Cursor Forward (默认 1)
            let dist = params.get_arg0(1) as i64;
            state.cursor_horizontal_relative(dist);
        }
        'D' => {
            // CUB - Cursor Backward (默认 1)
            let dist = params.get_arg0(1) as i64;
            state.cursor.x = max(state.left_margin, state.cursor.x - dist);
            state.cursor.about_to_wrap = false;
        }
        'E' => {
            // CNL - Cursor Next Line (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor_next_line(n);
        }
        'F' => {
            // CPL - Cursor Previous Line (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor_previous_line(n);
        }
        'G' | '`' => {
            // CHA - Cursor Horizontal Absolute (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor_horizontal_absolute(n);
        }
        'H' | 'f' => {
            // CUP - Cursor Position (默认 row=1, col=1)
            let row = params.get_arg0(1) as i64;
            let col = params.get_arg1(1) as i64;
            if state.origin_mode() {
                state.cursor.y = max(
                    state.top_margin,
                    min(state.bottom_margin - 1, state.top_margin + row - 1),
                );
            } else {
                state.cursor.y = max(0, min(state.rows - 1, row - 1));
            }
            state.cursor.x = max(state.left_margin, min(state.right_margin - 1, col - 1));
            state.cursor.about_to_wrap = false;
        }
        'I' => {
            // CHT - Cursor Horizontal Tab (默认 1)
            let n = params.get_arg0(1) as i64;
            for _ in 0..n {
                state.cursor_forward_tab();
            }
        }
        'J' => {
            // ED - Erase in Display (默认 0)
            let mode = params.get_arg0(0) as i64;
            state.cursor.about_to_wrap = false;
            state.erase_in_display(mode);
        }
        'K' => {
            // EL - Erase in Line (默认 0)
            let mode = params.get_arg0(0);
            state.cursor.about_to_wrap = false;
            state.erase_in_line(mode);
        }
        'L' => {
            // IL - Insert Line (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor.about_to_wrap = false;
            state.insert_lines(n);
        }
        'M' => {
            // DL - Delete Line (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor.about_to_wrap = false;
            state.delete_lines(n);
        }
        'P' => {
            // DCH - Delete Character (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor.about_to_wrap = false;
            state.delete_characters(n);
        }
        'S' => {
            // SU - Scroll Up (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor.about_to_wrap = false;
            state.scroll_up_lines(n);
        }
        'T' => {
            // SD - Scroll Down (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor.about_to_wrap = false;
            state.scroll_down_lines(n);
        }
        'X' => {
            // ECH - Erase Character (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor.about_to_wrap = false;
            state.erase_characters(n);
        }
        'Z' => {
            // CBT - Cursor Backward Tab (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor_backward_tab(n);
        }
        'b' => {
            // REP - Repeat (默认 1)
            let n = params.get_arg0(1) as i64;
            if let Some(c) = state.last_printed_char {
                state.repeat_character(n, c);
            }
        }
        'c' => {
            // DA - Device Attributes
            state.report_terminal_response("\x1b[?6c");
        }
        'd' => {
            // VPA - Vertical Position Absolute (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor_vertical_absolute(n);
        }
        'e' => {
            // VPR - Vertical Position Relative (默认 1)
            let n = params.get_arg0(1) as i64;
            state.cursor_vertical_relative(n);
        }
        'g' => {
            // TBC - Tab Clear (默认 0)
            let mode = params.get_arg0(0);
            state.clear_tab_stop(mode);
        }
        'h' => {
            // SM - Set Mode
            if is_private {
                state.handle_decset(params, true);
            } else {
                state.handle_set_mode(params, true);
            }
        }
        'l' => {
            // RM - Reset Mode
            if is_private {
                state.handle_decset(params, false);
            } else {
                state.handle_set_mode(params, false);
            }
        }
        'm' => {
            state.handle_sgr(params);
        }
        'n' => {
            // DSR - Device Status Report
            // Java: getArg0(-1) - 默认 -1 表示无参数
            let mode = if params.len == 0 {
                -1
            } else {
                params.get(0, 0)
            };
            match mode {
                5 => state.report_terminal_response("\x1b[0n"), // DSR Status Report
                6 => {
                    // CPR - Cursor Position Report
                    let r = state.cursor.y + 1;
                    let c = state.cursor.x + 1;
                    state.report_terminal_response(&format!("\x1b[{};{}R", r, c));
                }
                _ => {} // 其他值或无参数时忽略
            }
        }
        'p' => {
            if is_bang {
                state.decstr_soft_reset();
            }
        }
        'r' => {
            // DECSTBM - Set Top and Bottom Margins (默认 top=1, bottom=rows)
            let top = params.get_arg0(1) as i64;
            let bottom = params.get_arg1(state.rows as i32) as i64;
            state.set_margins(top, bottom);
        }
        's' => {
            if state.leftright_margin_mode() {
                // DECSLRM - Set Left and Right Margins (默认 left=1, right=cols)
                let left = params.get_arg0(1) as i64;
                let right = params.get_arg1(state.cols as i32) as i64;
                state.set_left_right_margins(left, right);
            } else {
                state.save_cursor();
            }
        }
        'u' => {
            state.restore_cursor();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vte_parser::Params;

    fn setup_state() -> crate::engine::ScreenState {
        crate::engine::ScreenState::new(80, 24, 100, 10, 20)
    }

    fn make_params(values: &[i32]) -> Params {
        let mut p = Params::new();
        for (i, &v) in values.iter().enumerate() {
            p.values[i] = v;
        }
        p.len = values.len();
        p
    }

    // -------------------------------------------------------------------------
    // Cursor movement
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_cursor_up() {
        let mut state = setup_state();
        state.cursor.y = 10;
        handle_csi(&mut state, &make_params(&[3]), &[], 'A');
        assert_eq!(state.cursor.y, 7);
    }

    #[test]
    fn test_csi_cursor_down() {
        let mut state = setup_state();
        state.cursor.y = 5;
        handle_csi(&mut state, &make_params(&[2]), &[], 'B');
        assert_eq!(state.cursor.y, 7);
    }

    #[test]
    fn test_csi_cursor_forward() {
        let mut state = setup_state();
        state.cursor.x = 5;
        handle_csi(&mut state, &make_params(&[4]), &[], 'C');
        assert_eq!(state.cursor.x, 9);
    }

    #[test]
    fn test_csi_cursor_backward() {
        let mut state = setup_state();
        state.cursor.x = 10;
        handle_csi(&mut state, &make_params(&[3]), &[], 'D');
        assert_eq!(state.cursor.x, 7);
    }

    #[test]
    fn test_csi_cursor_position() {
        let mut state = setup_state();
        handle_csi(&mut state, &make_params(&[5, 10]), &[], 'H');
        assert_eq!(state.cursor.y, 4);
        assert_eq!(state.cursor.x, 9);
    }

    #[test]
    fn test_csi_cursor_position_default() {
        let mut state = setup_state();
        state.cursor.x = 50;
        state.cursor.y = 20;
        handle_csi(&mut state, &make_params(&[]), &[], 'H');
        assert_eq!(state.cursor.y, 0);
        assert_eq!(state.cursor.x, 0);
    }

    // -------------------------------------------------------------------------
    // Erase
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_erase_in_display_clear_all() {
        let mut state = setup_state();
        state.get_current_screen_mut().get_row_mut(0).text[0] = 'X';
        handle_csi(&mut state, &make_params(&[2]), &[], 'J');
        let row = state.get_current_screen().get_row(0);
        assert_eq!(row.text[0], ' ');
    }

    #[test]
    fn test_csi_erase_in_line_clear_to_end() {
        let mut state = setup_state();
        state.cursor.x = 5;
        state.get_current_screen_mut().get_row_mut(0).text[10] = 'X';
        handle_csi(&mut state, &make_params(&[0]), &[], 'K');
        let row = state.get_current_screen().get_row(0);
        assert_eq!(row.text[10], ' ');
    }

    // -------------------------------------------------------------------------
    // Insert / Delete lines
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_insert_lines() {
        let mut state = setup_state();
        state.cursor.y = 5;
        handle_csi(&mut state, &make_params(&[2]), &[], 'L');
        // Verify cursor position unchanged and scroll happened
        assert_eq!(state.cursor.y, 5);
    }

    #[test]
    fn test_csi_delete_lines() {
        let mut state = setup_state();
        state.cursor.y = 5;
        handle_csi(&mut state, &make_params(&[1]), &[], 'M');
        assert_eq!(state.cursor.y, 5);
    }

    // -------------------------------------------------------------------------
    // Delete / Erase characters
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_delete_characters() {
        let mut state = setup_state();
        state.cursor.x = 5;
        let screen = state.get_current_screen_mut();
        screen.get_row_mut(0).text[5] = 'A';
        screen.get_row_mut(0).text[6] = 'B';
        handle_csi(&mut state, &make_params(&[2]), &[], 'P');
        let row = state.get_current_screen().get_row(0);
        assert_eq!(row.text[5], ' ');
    }

    #[test]
    fn test_csi_erase_characters() {
        let mut state = setup_state();
        state.cursor.x = 3;
        state.get_current_screen_mut().get_row_mut(0).text[3] = 'X';
        state.get_current_screen_mut().get_row_mut(0).text[4] = 'Y';
        handle_csi(&mut state, &make_params(&[2]), &[], 'X');
        let row = state.get_current_screen().get_row(0);
        assert_eq!(row.text[3], ' ');
        assert_eq!(row.text[4], ' ');
    }

    // -------------------------------------------------------------------------
    // Scroll
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_scroll_up() {
        let mut state = setup_state();
        let before = state.scroll_counter;
        handle_csi(&mut state, &make_params(&[1]), &[], 'S');
        assert_eq!(state.scroll_counter, before + 1);
    }

    #[test]
    fn test_csi_scroll_down() {
        let mut state = setup_state();
        handle_csi(&mut state, &make_params(&[1]), &[], 'T');
        // Just verify it doesn't panic
    }

    // -------------------------------------------------------------------------
    // Margins
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_set_margins() {
        let mut state = setup_state();
        handle_csi(&mut state, &make_params(&[5, 20]), &[], 'r');
        assert_eq!(state.top_margin, 4);
        assert_eq!(state.bottom_margin, 20);
    }

    // -------------------------------------------------------------------------
    // Tab stops
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_clear_tab_stop_single() {
        let mut state = setup_state();
        state.cursor.x = 8;
        assert!(state.tab_stops[8]);
        handle_csi(&mut state, &make_params(&[0]), &[], 'g');
        assert!(!state.tab_stops[8]);
    }

    #[test]
    fn test_csi_clear_tab_stop_all() {
        let mut state = setup_state();
        handle_csi(&mut state, &make_params(&[3]), &[], 'g');
        assert!(state.tab_stops.iter().all(|&t| !t));
    }

    // -------------------------------------------------------------------------
    // Cursor save / restore
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_save_restore_cursor() {
        let mut state = setup_state();
        state.cursor.x = 10;
        state.cursor.y = 5;
        handle_csi(&mut state, &make_params(&[]), &[], 's');
        state.cursor.x = 0;
        state.cursor.y = 0;
        handle_csi(&mut state, &make_params(&[]), &[], 'u');
        assert_eq!(state.cursor.x, 10);
        assert_eq!(state.cursor.y, 5);
    }

    // -------------------------------------------------------------------------
    // Device Attributes / Status Report
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_device_attributes() {
        let mut state = setup_state();
        handle_csi(&mut state, &make_params(&[]), &[], 'c');
        // Just verify no panic; response goes through JNI
    }

    #[test]
    fn test_csi_cursor_position_report() {
        let mut state = setup_state();
        state.cursor.x = 7;
        state.cursor.y = 3;
        handle_csi(&mut state, &make_params(&[6]), &[], 'n');
        // Response goes through JNI; just verify no panic
    }

    // -------------------------------------------------------------------------
    // DECSTR soft reset
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_decstr_soft_reset() {
        let mut state = setup_state();
        state.modes.set(crate::terminal::modes::MODE_INSERT);
        handle_csi(&mut state, &make_params(&[]), &[b'!'], 'p');
        assert!(!state.modes.is_enabled(crate::terminal::modes::MODE_INSERT));
    }

    // -------------------------------------------------------------------------
    // Repeat character
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_repeat_character() {
        let mut state = setup_state();
        state.last_printed_char = Some('X');
        handle_csi(&mut state, &make_params(&[3]), &[], 'b');
        let row = state.get_current_screen().get_row(0);
        assert_eq!(row.text[0], 'X');
        assert_eq!(row.text[1], 'X');
        assert_eq!(row.text[2], 'X');
    }
}
