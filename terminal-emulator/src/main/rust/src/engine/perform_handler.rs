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
