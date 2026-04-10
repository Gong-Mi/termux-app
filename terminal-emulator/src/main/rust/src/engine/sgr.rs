/// SGR (Select Graphic Rendition) 颜色处理
use crate::vte_parser::Params;
use crate::terminal::style::*;
use crate::terminal::colors::*;
use crate::engine::state::ScreenState;

impl ScreenState {
    pub fn handle_sgr(&mut self, params: &Params) {
        if params.len == 0 {
            self.current_style = STYLE_NORMAL;
            self.fore_color = COLOR_INDEX_FOREGROUND as u64;
            self.back_color = COLOR_INDEX_BACKGROUND as u64;
            self.effect = 0;
            return;
        }

        let mut i = 0;
        while i < params.len {
            let p = params.values[i];
            match p {
                0 => {
                    self.fore_color = COLOR_INDEX_FOREGROUND as u64;
                    self.back_color = COLOR_INDEX_BACKGROUND as u64;
                    self.effect = 0;
                }
                1 => self.effect |= EFFECT_BOLD,
                2 => self.effect |= EFFECT_DIM,
                3 => self.effect |= EFFECT_ITALIC,
                4 => self.effect |= EFFECT_UNDERLINE,
                5 => self.effect |= EFFECT_BLINK,
                7 => self.effect |= EFFECT_REVERSE,
                8 => self.effect |= EFFECT_INVISIBLE,
                9 => self.effect |= EFFECT_STRIKETHROUGH,
                21 => self.effect |= EFFECT_UNDERLINE,
                22 => { self.effect &= !EFFECT_BOLD; self.effect &= !EFFECT_DIM; }
                23 => self.effect &= !EFFECT_ITALIC,
                24 => self.effect &= !EFFECT_UNDERLINE,
                25 => self.effect &= !EFFECT_BLINK,
                27 => self.effect &= !EFFECT_REVERSE,
                28 => self.effect &= !EFFECT_INVISIBLE,
                29 => self.effect &= !EFFECT_STRIKETHROUGH,
                30..=37 => self.fore_color = (p - 30) as u64,
                38 => {
                    if i + 2 < params.len && params.values[i + 1] == 5 {
                        self.fore_color = params.values[i + 2] as u64;
                        i += 2;
                    } else if i + 4 < params.len && params.values[i + 1] == 2 {
                        let r = params.values[i + 2] as u32;
                        let g = params.values[i + 3] as u32;
                        let b = params.values[i + 4] as u32;
                        self.fore_color = (0xff000000 | (r << 16) | (g << 8) | b) as u64;
                        i += 4;
                    }
                }
                39 => self.fore_color = COLOR_INDEX_FOREGROUND as u64,
                40..=47 => self.back_color = (p - 40) as u64,
                48 => {
                    if i + 2 < params.len && params.values[i + 1] == 5 {
                        self.back_color = params.values[i + 2] as u64;
                        i += 2;
                    } else if i + 4 < params.len && params.values[i + 1] == 2 {
                        let r = params.values[i + 2] as u32;
                        let g = params.values[i + 3] as u32;
                        let b = params.values[i + 4] as u32;
                        self.back_color = (0xff000000 | (r << 16) | (g << 8) | b) as u64;
                        i += 4;
                    }
                }
                49 => self.back_color = COLOR_INDEX_BACKGROUND as u64,
                58 => {
                    if i + 2 < params.len && params.values[i + 1] == 5 {
                        self.underline_color = params.values[i + 2] as u64;
                        i += 2;
                    } else if i + 4 < params.len && params.values[i + 1] == 2 {
                        let r = params.values[i + 2] as u32;
                        let g = params.values[i + 3] as u32;
                        let b = params.values[i + 4] as u32;
                        self.underline_color = (0xff000000 | (r << 16) | (g << 8) | b) as u64;
                        i += 4;
                    }
                }
                59 => self.underline_color = COLOR_INDEX_FOREGROUND as u64,
                90..=97 => self.fore_color = (p - 90 + 8) as u64,
                100..=107 => self.back_color = (p - 100 + 8) as u64,
                _ => {}
            }
            i += 1;
        }
        self.current_style = encode_style(self.fore_color, self.back_color, self.effect);
    }
}
