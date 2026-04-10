/// DECSET/DECRST 模式处理
use crate::vte_parser::Params;
use crate::terminal::modes::*;
use crate::engine::state::ScreenState;

impl ScreenState {
    pub fn handle_decset(&mut self, params: &Params, set: bool) {
        for param in params.iter() {
            for &p in param.iter() {
                match p {
                    1 => {
                        if set { self.modes.set(DECSET_BIT_APPLICATION_CURSOR_KEYS) } else { self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS) }
                        self.application_cursor_keys = set;
                    },
                    3 => { /* DECCOLM - 132列模式，忽略 */ },
                    5 => {
                        if set { self.modes.set(DECSET_BIT_REVERSE_VIDEO) } else { self.modes.reset(DECSET_BIT_REVERSE_VIDEO) }
                    },
                    6 => {
                        if set { self.modes.set(DECSET_BIT_ORIGIN_MODE) } else { self.modes.reset(DECSET_BIT_ORIGIN_MODE) }
                    },
                    7 => {
                        if set { self.modes.set(DECSET_BIT_AUTOWRAP) } else { self.modes.reset(DECSET_BIT_AUTOWRAP) }
                    },
                    12 => { /* 光标闪烁，未完全实现 */ },
                    25 => { self.cursor_enabled = set; },
                    40 => { /* 132列模式切换，忽略 */ },
                    45 => { /* 反向换行，忽略 */ },
                    66 => {
                        if set { self.modes.set(DECSET_BIT_APPLICATION_KEYPAD) } else { self.modes.reset(DECSET_BIT_APPLICATION_KEYPAD) }
                    },
                    69 => {
                        if set { self.modes.set(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) } else { self.modes.reset(DECSET_BIT_LEFTRIGHT_MARGIN_MODE) }
                    },
                    1000 => {
                        if set {
                            self.modes.set(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE);
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT);
                            self.mouse_tracking = true;
                            self.mouse_button_event = false;
                        } else {
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE);
                            self.mouse_tracking = false;
                        }
                    },
                    1002 => {
                        if set {
                            self.modes.set(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT);
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE);
                            self.mouse_button_event = true;
                            self.mouse_tracking = false;
                        } else {
                            self.modes.reset(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT);
                            self.mouse_button_event = false;
                        }
                    },
                    1003 => { /* 鼠标追踪所有事件，未实现 */ },
                    1004 => {
                        if set { self.modes.set(DECSET_BIT_SEND_FOCUS_EVENTS) } else { self.modes.reset(DECSET_BIT_SEND_FOCUS_EVENTS) }
                        self.send_focus_events = set;
                    },
                    1006 => {
                        if set { self.modes.set(DECSET_BIT_MOUSE_PROTOCOL_SGR) } else { self.modes.reset(DECSET_BIT_MOUSE_PROTOCOL_SGR) }
                        self.sgr_mouse = set;
                    },
                    1034 => { /* 8位输入模式，忽略 */ },
                    1047 => {
                        if set {
                            self.use_alternate_buffer = true;
                            self.erase_in_display(2);
                        } else {
                            self.use_alternate_buffer = false;
                        }
                    },
                    1048 => {
                        if set { self.save_cursor(); } else { self.restore_cursor(); }
                    },
                    1049 => {
                        if set {
                            self.save_cursor();
                            self.use_alternate_buffer = true;
                            self.erase_in_display(2);
                        } else {
                            self.use_alternate_buffer = false;
                            self.restore_cursor();
                        }
                    },
                    2004 => {
                        if set { self.modes.set(DECSET_BIT_BRACKETED_PASTE_MODE) } else { self.modes.reset(DECSET_BIT_BRACKETED_PASTE_MODE) }
                        self.bracketed_paste = set;
                    },
                    _ => { /* 未知DECSET模式，忽略 */ }
                }
            }
        }
    }
}
