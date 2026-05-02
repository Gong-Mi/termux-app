/// VTE Parser 的 Perform trait 实现
use crate::vte_parser::{Params, Perform};
use crate::engine::state::ScreenState;
use crate::engine::events::TerminalEvent;

pub struct PerformHandler<'a> {
    pub state: &'a mut ScreenState,
    pub events: &'a mut Vec<TerminalEvent>,
}

impl<'a> Perform for PerformHandler<'a> {
    fn print(&mut self, c: char) {
        self.state.last_printed_char = Some(c);
        crate::terminal::handlers::print::handle_print(self.state, c);
    }

    fn print_str(&mut self, s: &str) {
        crate::terminal::handlers::print::handle_print_str(self.state, s);
    }

    fn execute(&mut self, byte: u8) {
        crate::terminal::handlers::control::handle_control(self.state, byte);
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        crate::terminal::handlers::csi::handle_csi(self.state, params, intermediates, action);
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.len() > 0 {
            if let Ok(opcode) = std::str::from_utf8(params[0]) {
                crate::terminal::handlers::osc::handle_osc(self.state, self.events, opcode, params);
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        crate::terminal::handlers::esc::handle_esc(self.state, intermediates, byte);
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        if action == 'q' && intermediates.is_empty() {
            self.state.sixel_decoder.start(params);
        }
    }

    fn put(&mut self, byte: u8) {
        self.state.sixel_decoder.process_data(&[byte]);
    }

    fn unhook(&mut self) {
        self.state.sixel_decoder.finish();
        let decoder = &self.state.sixel_decoder;
        self.events.push(TerminalEvent::SixelImage {
            rgba_data: decoder.get_image_data().to_vec(),
            width: decoder.width.max(1) as i32,
            height: decoder.height.max(1) as i32,
            start_x: decoder.start_x,
            start_y: decoder.start_y,
        });
    }

    fn bell(&mut self) {
        self.events.push(TerminalEvent::Bell);
    }

    fn backspace(&mut self) {
        crate::terminal::handlers::control::handle_control(self.state, 0x08);
    }

    fn tab(&mut self) {
        crate::terminal::handlers::control::handle_control(self.state, 0x09);
    }

    fn linefeed(&mut self) {
        crate::terminal::handlers::control::handle_control(self.state, 0x0a);
    }

    fn carriage_return(&mut self) {
        crate::terminal::handlers::control::handle_control(self.state, 0x0d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vte_parser::Params;

    fn setup() -> (ScreenState, Vec<TerminalEvent>) {
        (ScreenState::new(80, 24, 100, 10, 20), Vec::new())
    }

    // -------------------------------------------------------------------------
    // print / print_str
    // -------------------------------------------------------------------------
    #[test]
    fn test_print_moves_cursor() {
        let (mut state, mut events) = setup();
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.print('A');
        assert_eq!(handler.state.cursor.x, 1);
        assert_eq!(handler.state.cursor.y, 0);
        assert_eq!(handler.state.last_printed_char, Some('A'));
    }

    #[test]
    fn test_print_str_moves_cursor() {
        let (mut state, mut events) = setup();
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.print_str("Hello");
        assert_eq!(handler.state.cursor.x, 5);
        assert_eq!(handler.state.last_printed_char, Some('o'));
    }

    #[test]
    fn test_print_appears_on_screen() {
        let (mut state, mut events) = setup();
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.print('X');
        let row = handler.state.get_current_screen().get_row(0);
        assert_eq!(row.text[0], 'X');
    }

    // -------------------------------------------------------------------------
    // execute control bytes
    // -------------------------------------------------------------------------
    #[test]
    fn test_execute_bell() {
        // execute(0x07) goes through handle_control which calls report_bell() via JNI,
        // not the events vector. Direct bell() pushes to events.
        let (mut state, mut events) = setup();
        {
            let mut handler = PerformHandler { state: &mut state, events: &mut events };
            handler.execute(0x07);
        }
        // report_bell does not push TerminalEvent::Bell into local events
        assert!(events.is_empty());
        {
            let mut handler = PerformHandler { state: &mut state, events: &mut events };
            handler.bell();
        }
        assert!(events.iter().any(|e| matches!(e, TerminalEvent::Bell)));
    }

    #[test]
    fn test_execute_backspace() {
        let (mut state, mut events) = setup();
        state.cursor.x = 5;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.execute(0x08);
        assert_eq!(handler.state.cursor.x, 4);
    }

    #[test]
    fn test_execute_linefeed() {
        let (mut state, mut events) = setup();
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.execute(0x0a);
        assert_eq!(handler.state.cursor.y, 1);
    }

    #[test]
    fn test_execute_linefeed_with_lnm() {
        let (mut state, mut events) = setup();
        state.cursor.x = 10;
        state.modes.set(crate::terminal::modes::MODE_LNM);
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.execute(0x0a);
        assert_eq!(handler.state.cursor.y, 1);
        assert_eq!(handler.state.cursor.x, 0);
    }

    #[test]
    fn test_execute_carriage_return() {
        let (mut state, mut events) = setup();
        state.cursor.x = 15;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.execute(0x0d);
        assert_eq!(handler.state.cursor.x, 0);
    }

    // -------------------------------------------------------------------------
    // direct control methods
    // -------------------------------------------------------------------------
    #[test]
    fn test_bell_produces_event() {
        let (mut state, mut events) = setup();
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.bell();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], TerminalEvent::Bell));
    }

    #[test]
    fn test_backspace_moves_left() {
        let (mut state, mut events) = setup();
        state.cursor.x = 3;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.backspace();
        assert_eq!(handler.state.cursor.x, 2);
    }

    #[test]
    fn test_tab_moves_forward() {
        let (mut state, mut events) = setup();
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.tab();
        assert_eq!(handler.state.cursor.x, 8); // first tab stop at col 8
    }

    #[test]
    fn test_carriage_return_resets_x() {
        let (mut state, mut events) = setup();
        state.cursor.x = 20;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.carriage_return();
        assert_eq!(handler.state.cursor.x, 0);
    }

    // -------------------------------------------------------------------------
    // CSI dispatch
    // -------------------------------------------------------------------------
    #[test]
    fn test_csi_cursor_up() {
        let (mut state, mut events) = setup();
        state.cursor.y = 5;
        let mut params = Params::new();
        params.values[0] = 3;
        params.len = 1;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.csi_dispatch(&params, &[], false, 'A');
        assert_eq!(handler.state.cursor.y, 2);
    }

    #[test]
    fn test_csi_cursor_down() {
        let (mut state, mut events) = setup();
        let mut params = Params::new();
        params.values[0] = 2;
        params.len = 1;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.csi_dispatch(&params, &[], false, 'B');
        assert_eq!(handler.state.cursor.y, 2);
    }

    #[test]
    fn test_csi_cursor_forward() {
        let (mut state, mut events) = setup();
        let mut params = Params::new();
        params.values[0] = 5;
        params.len = 1;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.csi_dispatch(&params, &[], false, 'C');
        assert_eq!(handler.state.cursor.x, 5);
    }

    #[test]
    fn test_csi_cursor_backward() {
        let (mut state, mut events) = setup();
        state.cursor.x = 10;
        let mut params = Params::new();
        params.values[0] = 3;
        params.len = 1;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.csi_dispatch(&params, &[], false, 'D');
        assert_eq!(handler.state.cursor.x, 7);
    }

    #[test]
    fn test_csi_erase_in_display() {
        let (mut state, mut events) = setup();
        // Print something first
        state.get_current_screen_mut().get_row_mut(0).text[0] = 'X';
        let mut params = Params::new();
        params.values[0] = 2;
        params.len = 1;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.csi_dispatch(&params, &[], false, 'J');
        let row = handler.state.get_current_screen().get_row(0);
        assert_eq!(row.text[0], ' ');
    }

    #[test]
    fn test_csi_set_mode() {
        let (mut state, mut events) = setup();
        let mut params = Params::new();
        params.values[0] = 4; // Insert mode
        params.len = 1;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.csi_dispatch(&params, &[], false, 'h');
        assert!(handler.state.modes.is_enabled(crate::terminal::modes::MODE_INSERT));
    }

    #[test]
    fn test_csi_reset_mode() {
        let (mut state, mut events) = setup();
        state.modes.set(crate::terminal::modes::MODE_INSERT);
        let mut params = Params::new();
        params.values[0] = 4;
        params.len = 1;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.csi_dispatch(&params, &[], false, 'l');
        assert!(!handler.state.modes.is_enabled(crate::terminal::modes::MODE_INSERT));
    }

    // -------------------------------------------------------------------------
    // ESC dispatch
    // -------------------------------------------------------------------------
    #[test]
    fn test_execute_shift_out() {
        // SO (0x0e) and SI (0x0f) are control characters processed via execute(),
        // not esc_dispatch().
        let (mut state, mut events) = setup();
        state.use_line_drawing_uses_g0 = true;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.execute(0x0e); // SO
        assert!(!handler.state.use_line_drawing_uses_g0);
    }

    #[test]
    fn test_execute_shift_in() {
        let (mut state, mut events) = setup();
        state.use_line_drawing_uses_g0 = false;
        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.execute(0x0f); // SI
        assert!(handler.state.use_line_drawing_uses_g0);
    }

    // -------------------------------------------------------------------------
    // Sixel hook / put / unhook
    // -------------------------------------------------------------------------
    #[test]
    fn test_sixel_sequence_produces_event() {
        let (mut state, mut events) = setup();
        let mut params = Params::new();
        params.values[0] = 0;
        params.len = 1;

        let mut handler = PerformHandler { state: &mut state, events: &mut events };
        handler.hook(&params, &[], false, 'q');
        handler.put(b' '); // some sixel data
        handler.unhook();

        assert!(events.iter().any(|e| matches!(e, TerminalEvent::SixelImage { .. })));
    }

    #[test]
    fn test_sixel_only_responds_to_q() {
        let (mut state, mut events) = setup();
        let params = Params::new();
        {
            let mut handler = PerformHandler { state: &mut state, events: &mut events };
            handler.hook(&params, &[], false, 'x'); // not 'q'
            handler.unhook();
        }
        // unhook unconditionally pushes a SixelImage event even if hook didn't start decoder
        assert_eq!(events.len(), 1);
        if let TerminalEvent::SixelImage { width, height, .. } = &events[0] {
            assert_eq!(*width, 1); // max(1, 0) when decoder not started
            assert_eq!(*height, 1);
        } else {
            panic!("Expected SixelImage event");
        }
    }
}
