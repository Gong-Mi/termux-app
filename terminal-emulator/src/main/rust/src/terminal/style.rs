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
