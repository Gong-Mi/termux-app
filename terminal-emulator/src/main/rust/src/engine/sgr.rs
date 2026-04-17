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
                    if i + 2 <= params.len && params.values[i + 1] == 5 {
                        self.fore_color = params.values[i + 2] as u64;
                        i += 2;
                    } else if i + 4 <= params.len && params.values[i + 1] == 2 {
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
                    if i + 2 <= params.len && params.values[i + 1] == 5 {
                        self.back_color = params.values[i + 2] as u64;
                        i += 2;
                    } else if i + 4 <= params.len && params.values[i + 1] == 2 {
                        let r = params.values[i + 2] as u32;
                        let g = params.values[i + 3] as u32;
                        let b = params.values[i + 4] as u32;
                        self.back_color = (0xff000000 | (r << 16) | (g << 8) | b) as u64;
                        i += 4;
                    }
                }
                49 => self.back_color = COLOR_INDEX_BACKGROUND as u64,
                58 => {
                    if i + 2 <= params.len && params.values[i + 1] == 5 {
                        self.underline_color = params.values[i + 2] as u64;
                        i += 2;
                    } else if i + 4 <= params.len && params.values[i + 1] == 2 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_params(vals: &[i32]) -> Params {
        let mut p = Params::default();
        for &v in vals {
            p.values[p.len] = v;
            p.len += 1;
        }
        p
    }

    #[test]
    fn test_sgr_simple_colors() {
        let mut state = ScreenState::new(80, 24, 1000, 10, 20);
        // CSI 31;42m -> Red FG, Green BG
        let params = create_params(&[31, 42]);
        state.handle_sgr(&params);
        assert_eq!(state.fore_color, 1); // 31-30
        assert_eq!(state.back_color, 2); // 42-40
    }

    #[test]
    fn test_sgr_truecolor_fg() {
        let mut state = ScreenState::new(80, 24, 1000, 10, 20);
        // CSI 38;2;255;128;64m
        let params = create_params(&[38, 2, 255, 128, 64]);
        state.handle_sgr(&params);
        assert_eq!(state.fore_color, 0xffff8040);
    }

    #[test]
    fn test_sgr_mixed_malformed_consumption() {
        let mut state = ScreenState::new(80, 24, 1000, 10, 20);
        // 模拟一个错误：38 后面没有跟着 2 或 5，而是跟着 1 (Bold)
        // CSI 38;1;32m
        let params = create_params(&[38, 1, 32]);
        state.handle_sgr(&params);
        
        // 期望：38 因为不符合真彩色/256色格式被跳过，1 应该被解析为加粗，32 为绿色
        assert_ne!(state.effect & EFFECT_BOLD, 0, "Bold flag should be set");
        assert_eq!(state.fore_color, 2, "Foreground should be green (32-30)");
    }

    #[test]
    fn test_sgr_parameter_skipping_bug() {
        let mut state = ScreenState::new(80, 24, 1000, 10, 20);
        // CSI 1;38;5;42;4m  -> Bold, 256-color(42), Underline
        // 这是一个标准的多参数序列
        let params = create_params(&[1, 38, 5, 42, 4]);
        state.handle_sgr(&params);
        
        assert_ne!(state.effect & EFFECT_BOLD, 0, "Should be bold");
        assert_eq!(state.fore_color, 42, "Should be color 42");
        assert_ne!(state.effect & EFFECT_UNDERLINE, 0, "Should be underlined");
    }

    #[test]
    fn test_sgr_subparameters_fail() {
        let mut state = ScreenState::new(80, 24, 1000, 10, 20);
        // 模拟子参数情况
        let params = create_params(&[38, 2, 255, 255, 255]);
        state.handle_sgr(&params);
        assert_eq!(state.fore_color, 0xffffffff);
    }
}
