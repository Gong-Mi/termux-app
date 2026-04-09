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
        result |= (1 << 9) | ((fore_color & 0x00ffffff) << 40);
    } else {
        result |= (fore_color & 0x1FF) << 40;
    }
    if (back_color & 0xff000000) == 0xff000000 {
        result |= (1 << 10) | ((back_color & 0x00ffffff) << 16);
    } else {
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
        // 默认样式：前景 256, 背景 257, 无效果
        assert_eq!(decode_effect(STYLE_NORMAL), 0);
        assert_eq!(decode_fore_color(STYLE_NORMAL), COLOR_INDEX_FOREGROUND as u64);
        assert_eq!(decode_back_color(STYLE_NORMAL), COLOR_INDEX_BACKGROUND as u64);
    }

    #[test]
    fn test_encode_decode_index_colors() {
        let s = encode_style(5, 12, EFFECT_BOLD);
        assert_eq!(decode_effect(s), EFFECT_BOLD);
        assert_eq!(decode_fore_color(s), 5);
        assert_eq!(decode_back_color(s), 12);
        // 真彩色标志位未设置
        assert_eq!(s & STYLE_TRUECOLOR_FG, 0);
        assert_eq!(s & STYLE_TRUECOLOR_BG, 0);
    }

    #[test]
    fn test_encode_decode_truecolor() {
        let fg = 0xff123456u64;
        let bg = 0xffaabbccu64;
        let s = encode_style(fg, bg, EFFECT_UNDERLINE);
        // Truecolor flags (bits 9, 10) are set within the effect range
        let expected_effect = EFFECT_UNDERLINE | STYLE_TRUECOLOR_FG | STYLE_TRUECOLOR_BG;
        assert_eq!(decode_effect(s), expected_effect);
        assert_eq!(decode_fore_color(s), fg);
        assert_eq!(decode_back_color(s), bg);
        // 真彩色标志位已设置
        assert_ne!(s & STYLE_TRUECOLOR_FG, 0);
        assert_ne!(s & STYLE_TRUECOLOR_BG, 0);
    }

    #[test]
    fn test_mixed_index_and_truecolor() {
        // 索引前景 + 真彩色背景
        let bg = 0xff112233u64;
        let s = encode_style(42, bg, EFFECT_DIM);
        let expected_effect = EFFECT_DIM | STYLE_TRUECOLOR_BG;
        assert_eq!(decode_effect(s), expected_effect);
        assert_eq!(decode_fore_color(s), 42);
        assert_eq!(decode_back_color(s), bg);
        assert_eq!(s & STYLE_TRUECOLOR_FG, 0);
        assert_ne!(s & STYLE_TRUECOLOR_BG, 0);
    }

    #[test]
    fn test_effect_flags_without_truecolor() {
        // Test effect flags without triggering truecolor
        let combined = EFFECT_BOLD | EFFECT_ITALIC | EFFECT_UNDERLINE | EFFECT_BLINK | EFFECT_REVERSE;
        let s = encode_style(5, 7, combined);
        assert_eq!(decode_effect(s), combined);
        // 验证各标志位
        assert_ne!(s & EFFECT_BOLD, 0);
        assert_ne!(s & EFFECT_ITALIC, 0);
        assert_ne!(s & EFFECT_UNDERLINE, 0);
        assert_ne!(s & EFFECT_BLINK, 0);
        assert_ne!(s & EFFECT_REVERSE, 0);
        assert_eq!(s & EFFECT_INVISIBLE, 0);
        // Index colors don't set truecolor flags
        assert_eq!(s & STYLE_TRUECOLOR_FG, 0);
        assert_eq!(s & STYLE_TRUECOLOR_BG, 0);
    }

    #[test]
    fn test_all_effect_flags() {
        let all_effects = EFFECT_BOLD | EFFECT_ITALIC | EFFECT_UNDERLINE | EFFECT_BLINK
            | EFFECT_REVERSE | EFFECT_INVISIBLE | EFFECT_STRIKETHROUGH | EFFECT_PROTECTED | EFFECT_DIM;
        let s = encode_style(7, 7, all_effects);
        assert_eq!(decode_effect(s), all_effects);
    }

    #[test]
    fn test_color_index_cursor() {
        let s = encode_style(COLOR_INDEX_CURSOR as u64, 0, 0);
        assert_eq!(decode_fore_color(s), COLOR_INDEX_CURSOR as u64);
    }

    #[test]
    fn test_effect_mask_bounds() {
        // 确保 effect 只占用低 11 位
        let s = encode_style(0, 0, 0xFFFF_FFFF);
        assert_eq!(decode_effect(s), 0x7FF); // 只保留低 11 位
    }

    #[test]
    fn test_color_0_is_black() {
        let s = encode_style(0, 0, 0);
        assert_eq!(decode_fore_color(s), 0);
        assert_eq!(decode_back_color(s), 0);
    }

    #[test]
    fn test_color_255_is_white_index() {
        let s = encode_style(255, 255, 0);
        assert_eq!(decode_fore_color(s), 255);
        assert_eq!(decode_back_color(s), 255);
    }
}
