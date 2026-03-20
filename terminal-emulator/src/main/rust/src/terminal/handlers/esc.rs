use crate::engine::ScreenState;

pub fn handle_esc(state: &mut ScreenState, intermediates: &[u8], byte: u8) {
    match (intermediates, byte) {
        (&[b'#'], b'8') => { state.decaln_screen_align(); }
        (&[b'('], b'0') => {
            state.use_line_drawing_g0 = true;
            state.use_line_drawing_uses_g0 = true;
        }
        (&[b'('], b'B') => { state.use_line_drawing_g0 = false; }
        (&[b')'], b'0') => {
            state.use_line_drawing_g1 = true;
            state.use_line_drawing_uses_g0 = false;
        }
        (&[b')'], b'B') => { state.use_line_drawing_g1 = false; }
        (&[], b'6') => {
            if state.cursor.x > state.left_margin {
                state.cursor.x -= 1;
            } else {
                state.back_index_scroll();
            }
        }
        (&[], b'7') => { state.save_cursor(); }
        (&[], b'8') => { state.restore_cursor(); }
        (&[], b'9') => {
            if state.cursor.x < state.right_margin - 1 {
                state.cursor.x += 1;
            } else {
                state.forward_index_scroll();
            }
        }
        (&[], b'c') => { state.reset_to_initial_state(); }
        (&[], b'D') => {
            if state.cursor.y < state.bottom_margin - 1 {
                state.cursor.y += 1;
            } else {
                state.scroll_up();
            }
        }
        (&[], b'E') => {
            if state.cursor.y < state.bottom_margin - 1 {
                state.cursor.y += 1;
                state.cursor.x = state.left_margin;
            } else {
                state.scroll_up();
                state.cursor.x = state.left_margin;
            }
        }
        (&[], b'F') => {
            state.cursor.x = state.left_margin;
            state.cursor.y = state.bottom_margin - 1;
        }
        (&[], b'H') => {
            if state.cursor.x >= 0 && (state.cursor.x as usize) < state.tab_stops.len() {
                state.tab_stops[state.cursor.x as usize] = true;
            }
        }
        (&[], b'M') => {
            if state.cursor.y > state.top_margin {
                state.cursor.y -= 1;
            } else {
                state.reverse_index_scroll();
            }
        }
        (&[], b'=') => { state.cursor.saved_state.decset_flags |= 1 << 5; /* DECKPAM */ }
        (&[], b'>') => { state.cursor.saved_state.decset_flags &= !(1 << 5); /* DECKPNM */ }
        _ => {}
    }
}
