// 从 colors.rs 导入颜色索引常量
use crate::terminal::colors::{COLOR_INDEX_FOREGROUND, COLOR_INDEX_BACKGROUND};

/// STYLE_NORMAL: 前景 256, 背景 257, 无效果
pub const STYLE_NORMAL: u64 = encode_style(COLOR_INDEX_FOREGROUND as u64, COLOR_INDEX_BACKGROUND as u64, 0);
pub const STYLE_MASK_EFFECT: u64 = 0x7FF;           // 位 0-10 (11 位效果标志)
pub const STYLE_MASK_BG: u64 = 0xFFFFFF << 16;      // 位 16-39 (24 位背景色)
pub const STYLE_MASK_FG: u64 = 0xFFFFFF << 40;      // 位 40-63 (24 位前景色)

// 真彩色标志位
pub const STYLE_TRUECOLOR_FG: u64 = 1 << 9; // 位 9 - 前景色使用 24 位真彩色
pub const STYLE_TRUECOLOR_BG: u64 = 1 << 10; // 位 10 - 背景色使用 24 位真彩色

// 效果标志
pub const EFFECT_BOLD: u64 = 1 << 0;
pub const EFFECT_ITALIC: u64 = 1 << 1;
pub const EFFECT_UNDERLINE: u64 = 1 << 2;
pub const EFFECT_BLINK: u64 = 1 << 3;
pub const EFFECT_REVERSE: u64 = 1 << 4;
pub const EFFECT_INVISIBLE: u64 = 1 << 5;
pub const EFFECT_STRIKETHROUGH: u64 = 1 << 6;
pub const EFFECT_PROTECTED: u64 = 1 << 7;
pub const EFFECT_DIM: u64 = 1 << 8;

#[inline]
pub const fn encode_style(fore_color: u64, back_color: u64, effect: u64) -> u64 {
    let mut result = effect & 0x7FF;
    if (fore_color & 0xff000000) == 0xff000000 {
        result |= STYLE_TRUECOLOR_FG | ((fore_color & 0x00ffffff) << 40);
    } else {
        result &= !STYLE_TRUECOLOR_FG; // 确保清除真彩色标志
        result |= (fore_color & 0x1FF) << 40;
    }
    if (back_color & 0xff000000) == 0xff000000 {
        result |= STYLE_TRUECOLOR_BG | ((back_color & 0x00ffffff) << 16);
    } else {
        result &= !STYLE_TRUECOLOR_BG; // 确保清除真彩色标志
        result |= (back_color & 0x1FF) << 16;
    }
    result
}

pub fn decode_fore_color(style: u64) -> u64 {
    if (style & STYLE_TRUECOLOR_FG) != 0 {
        0xff000000 | ((style & STYLE_MASK_FG) >> 40)
    } else {
        (style & STYLE_MASK_FG) >> 40
    }
}

pub fn decode_back_color(style: u64) -> u64 {
    if (style & STYLE_TRUECOLOR_BG) != 0 {
        0xff000000 | ((style & STYLE_MASK_BG) >> 16)
    } else {
        (style & STYLE_MASK_BG) >> 16
    }
}

pub fn decode_effect(style: u64) -> u64 {
    style & STYLE_MASK_EFFECT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::colors::COLOR_INDEX_CURSOR;

    #[test]
    fn test_style_normal() {
        assert_eq!(decode_effect(STYLE_NORMAL), 0);
        assert_eq!(decode_fore_color(STYLE_NORMAL), COLOR_INDEX_FOREGROUND as u64);
        assert_eq!(decode_back_color(STYLE_NORMAL), COLOR_INDEX_BACKGROUND as u64);
    }

    #[test]
    fn test_invisible_color_bug_reproduction() {
        // 模拟一个潜在的 Bug：如果有人错误地在 effect 中包含了 STYLE_TRUECOLOR_FG 位
        let index_color = 256u64;
        let effect_with_err_flag = EFFECT_DIM | STYLE_TRUECOLOR_FG;
        
        let s = encode_style(index_color, 0, effect_with_err_flag);
        let decoded_fg = decode_fore_color(s);
        
        // 如果 encode_style 足够强壮，它应该强制覆盖 effect 里的真彩色标志位
        assert_eq!(decoded_fg, index_color, "Color index should not be corrupted by effect flags");
        assert_eq!(s & STYLE_TRUECOLOR_FG, 0, "TrueColor flag must be cleared for index colors");
    }

    #[test]
    fn test_all_effect_flags() {
        let all_effects = EFFECT_BOLD | EFFECT_ITALIC | EFFECT_UNDERLINE | EFFECT_BLINK
            | EFFECT_REVERSE | EFFECT_INVISIBLE | EFFECT_STRIKETHROUGH | EFFECT_PROTECTED | EFFECT_DIM;
        let s = encode_style(7, 7, all_effects);
        assert_eq!(decode_effect(s), all_effects);
    }
}
