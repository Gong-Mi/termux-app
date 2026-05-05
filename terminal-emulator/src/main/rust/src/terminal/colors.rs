use skia_safe::Color4f;

/// 默认颜色方案（与 Java TerminalColorScheme.DEFAULT_COLORSCHEME 一致）
pub const DEFAULT_COLORSCHEME: [u32; 259] = [
    // 16 原始颜色（前 8 个是暗色）
    0xff000000, 0xffcd0000, 0xff00cd00, 0xffcdcd00, 0xff6495ed, 0xffcd00cd, 0xff00cdcd, 0xffe5e5e5,
    // 后 8 个是亮色
    0xff7f7f7f, 0xffff0000, 0xff00ff00, 0xffffff00, 0xff5c5cff, 0xffff00ff, 0xff00ffff, 0xffffffff,
    // 216 色立方体
    0xff000000, 0xff00005f, 0xff000087, 0xff0000af, 0xff0000d7, 0xff0000ff, 0xff005f00, 0xff005f5f,
    0xff005f87, 0xff005faf, 0xff005fd7, 0xff005fff, 0xff008700, 0xff00875f, 0xff008787, 0xff0087af,
    0xff0087d7, 0xff0087ff, 0xff00af00, 0xff00af5f, 0xff00af87, 0xff00afaf, 0xff00afd7, 0xff00afff,
    0xff00d700, 0xff00d75f, 0xff00d787, 0xff00d7af, 0xff00d7d7, 0xff00d7ff, 0xff00ff00, 0xff00ff5f,
    0xff00ff87, 0xff00ffaf, 0xff00ffd7, 0xff00ffff, 0xff5f0000, 0xff5f005f, 0xff5f0087, 0xff5f00af,
    0xff5f00d7, 0xff5f00ff, 0xff5f5f00, 0xff5f5f5f, 0xff5f5f87, 0xff5f5faf, 0xff5fd700, 0xff5fd75f,
    0xff5f8700, 0xff5f875f, 0xff5f8787, 0xff5f87af, 0xff5f87d7, 0xff5f87ff, 0xff5faf00, 0xff5faf5f,
    0xff5faf87, 0xff5fafaf, 0xff5fafd7, 0xff5fafff, 0xff5fd700, 0xff5fd75f, 0xff5fd787, 0xff5fd7af,
    0xff5fd7d7, 0xff5fd7ff, 0xff5fff00, 0xff5fff5f, 0xff5fff87, 0xff5fffaf, 0xff5fffd7, 0xff5fffff,
    0xff870000, 0xff87005f, 0xff870087, 0xff8700af, 0xff8700d7, 0xff8700ff, 0xff875f00, 0xff875f5f,
    0xff875f87, 0xff875faf, 0xff875fd7, 0xff875fff, 0xff878700, 0xff87875f, 0xff878787, 0xff8787af,
    0xff8787d7, 0xff8787ff, 0xff87af00, 0xff87af5f, 0xff87af87, 0xff87afaf, 0xff87afd7, 0xff87afff,
    0xff87d700, 0xff87d75f, 0xff87d787, 0xff87d7af, 0xff87d7d7, 0xff87d7ff, 0xff87ff00, 0xff87ff5f,
    0xff87ff87, 0xff87ffaf, 0xff87ffd7, 0xff87ffff, 0xffaf0000, 0xffaf005f, 0xffaf0087, 0xffaf00af,
    0xffaf00d7, 0xffaf00ff, 0xffaf5f00, 0xffaf5f5f, 0xffaf5f87, 0xffaf5faf, 0xffaf5fd7, 0xffaf5fff,
    0xffaf8700, 0xffaf875f, 0xffaf8787, 0xffaf87af, 0xffaf87d7, 0xffaf87ff, 0xffafaf00, 0xffafaf5f,
    0xffafaf87, 0xffafafaf, 0xffafafd7, 0xffafafff, 0xffafd700, 0xffafd75f, 0xffafd787, 0xffafd7af,
    0xffafd7d7, 0xffafd7ff, 0xffafff00, 0xffafff5f, 0xffafff87, 0xffafffaf, 0xffafffd7, 0xffafffff,
    0xffd70000, 0xffd7005f, 0xffd70087, 0xffd700af, 0xffd700d7, 0xffd700ff, 0xffd75f00, 0xffd75f5f,
    0xffd75f87, 0xffd75faf, 0xffd75fd7, 0xffd75fff, 0xffd78700, 0xffd7875f, 0xffd78787, 0xffd787af,
    0xffd787d7, 0xffd787ff, 0xffd7af00, 0xffd7af5f, 0xffd7af87, 0xffd7afaf, 0xffd7afd7, 0xffd7afff,
    0xffd7d700, 0xffd7d75f, 0xffd7d787, 0xffd7d7af, 0xffd7d7d7, 0xffd7d7ff, 0xffd7ff00, 0xffd7ff5f,
    0xffd7ff87, 0xffd7ffaf, 0xffd7ffd7, 0xffd7ffff, 0xffff0000, 0xffff005f, 0xffff0087, 0xffff00af,
    0xffff00d7, 0xffff00ff, 0xffff5f00, 0xffff5f5f, 0xffff5f87, 0xffff5faf, 0xffff5fd7, 0xffff5fff,
    0xffff8700, 0xffff875f, 0xffff8787, 0xffff87af, 0xffff87d7, 0xffff87ff, 0xffffaf00, 0xffffaf5f,
    0xffffaf87, 0xffffafaf, 0xffffafd7, 0xffffafff, 0xffffd700, 0xffffd75f, 0xffffd787, 0xffffd7af,
    0xffffd7d7, 0xffffd7ff, 0xffffff00, 0xffffff5f, 0xffffff87, 0xffffffaf, 0xffffffd7, 0xffffffff,
    // 24 级灰度
    0xff080808, 0xff121212, 0xff1c1c1c, 0xff262626, 0xff303030, 0xff3a3a3a, 0xff444444, 0xff4e4e4e,
    0xff585858, 0xff626262, 0xff6c6c6c, 0xff767676, 0xff808080, 0xff8a8a8a, 0xff949494, 0xff9e9e9e,
    0xffa8a8a8, 0xffb2b2b2, 0xffbcbcbc, 0xffc6c6c6, 0xffd0d0d0, 0xffdadada, 0xffe4e4e4, 0xffeeeeee,
    // 特殊颜色索引
    0xffffffff, // 256: COLOR_INDEX_FOREGROUND
    0xff000000, // 257: COLOR_INDEX_BACKGROUND
    0xffffffff, // 258: COLOR_INDEX_CURSOR
];

/// 颜色索引常量（与 Java TextStyle 保持一致）
/// 注意：这些是 usize 类型，因为用于数组索引
pub const COLOR_INDEX_FOREGROUND: usize = 256;
pub const COLOR_INDEX_BACKGROUND: usize = 257;
pub const COLOR_INDEX_CURSOR: usize = 258;
/// 标准颜色索引总数（256 色 + 3 特殊 = 259）
pub const NUM_INDEXED_COLORS: usize = 259;

/// 感知亮度计算的系数（来自 Java TerminalColors.getPerceivedBrightnessOfColor）
/// https://www.nbdtech.com/Blog/archive/2008/04/27/Calculating-the-Perceived-Brightness-of-a-Color.aspx
/// http://alienryderflex.com/hsp.html
const BRIGHTNESS_R_COEF: f64 = 0.241;
const BRIGHTNESS_G_COEF: f64 = 0.691;
const BRIGHTNESS_B_COEF: f64 = 0.068;

/// 光标颜色自动设置的亮度阈值（与 Java 一致）
const CURSOR_BRIGHTNESS_THRESHOLD: u8 = 130;

pub struct TerminalColors {
    pub current_colors: [u32; 259],
    pub current_colors_4f: [Color4f; 259],
}

impl TerminalColors {
    pub fn new() -> Self {
        let mut colors_4f = [Color4f::new(0.0, 0.0, 0.0, 0.0); 259];
        for i in 0..259 {
            colors_4f[i] = Color4f::from(skia_safe::Color::new(DEFAULT_COLORSCHEME[i]));
        }
        Self { 
            current_colors: DEFAULT_COLORSCHEME,
            current_colors_4f: colors_4f,
        }
    }

    /// 解析颜色字符串并返回 (u32, Color4f)
    pub fn parse_color_full(color_str: &str) -> Option<(u32, Color4f)> {
        let color_str = color_str.trim_end_matches(|c| c == '\x07' || c == '\x1b' || c == '\\').trim();
        
        if color_str.starts_with('#') {
            // #RGB, #RRGGBB, #RRRGGGBBB, #RRRRGGGGBBBB
            let hex = &color_str[1..];
            match hex.len() {
                3 => {
                    let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                    let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                    let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                    let c32 = 0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                    Some((c32, Color4f::from(skia_safe::Color::new(c32))))
                }
                6 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    let c32 = 0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                    Some((c32, Color4f::from(skia_safe::Color::new(c32))))
                }
                9 => {
                    let r = u16::from_str_radix(&hex[0..3], 16).ok()?;
                    let g = u16::from_str_radix(&hex[3..6], 16).ok()?;
                    let b = u16::from_str_radix(&hex[6..9], 16).ok()?;
                    let rf = r as f32 / 4095.0;
                    let gf = g as f32 / 4095.0;
                    let bf = b as f32 / 4095.0;
                    let c32 = 0xff000000 | (((rf * 255.0) as u32) << 16) | (((gf * 255.0) as u32) << 8) | ((bf * 255.0) as u32);
                    Some((c32, Color4f::new(rf, gf, bf, 1.0)))
                }
                12 => {
                    let r = u16::from_str_radix(&hex[0..4], 16).ok()?;
                    let g = u16::from_str_radix(&hex[4..8], 16).ok()?;
                    let b = u16::from_str_radix(&hex[8..12], 16).ok()?;
                    let rf = r as f32 / 65535.0;
                    let gf = g as f32 / 65535.0;
                    let bf = b as f32 / 65535.0;
                    let c32 = 0xff000000 | (((rf * 255.0) as u32) << 16) | (((gf * 255.0) as u32) << 8) | ((bf * 255.0) as u32);
                    Some((c32, Color4f::new(rf, gf, bf, 1.0)))
                }
                _ => None,
            }
        } else if color_str.starts_with("rgb:") {
            let rgb_part = &color_str[4..];
            let parts: Vec<&str> = rgb_part.split('/').collect();
            if parts.len() != 3 { return None; }
            
            let rf = parse_rgb_component_f32(parts[0])?;
            let gf = parse_rgb_component_f32(parts[1])?;
            let bf = parse_rgb_component_f32(parts[2])?;
            let c32 = 0xff000000 | (((rf * 255.0) as u32) << 16) | (((gf * 255.0) as u32) << 8) | ((bf * 255.0) as u32);
            Some((c32, Color4f::new(rf, gf, bf, 1.0)))
        } else {
            None
        }
    }

    pub fn parse_color(color_str: &str) -> Option<u32> {
        Self::parse_color_full(color_str).map(|(c32, _)| c32)
    }

    /// 计算颜色的感知亮度 (0-255)
    pub fn get_perceived_brightness(color: u32) -> u8 {
        let r = ((color >> 16) & 0xff) as f64;
        let g = ((color >> 8) & 0xff) as f64;
        let b = (color & 0xff) as f64;
        let brightness = (r * r * BRIGHTNESS_R_COEF 
                        + g * g * BRIGHTNESS_G_COEF 
                        + b * b * BRIGHTNESS_B_COEF).sqrt();
        brightness as u8
    }

    pub fn set_cursor_color_for_background(&mut self) {
        let bg_color = self.current_colors[COLOR_INDEX_BACKGROUND];
        let brightness = Self::get_perceived_brightness(bg_color);
        if brightness < CURSOR_BRIGHTNESS_THRESHOLD {
            self.current_colors[COLOR_INDEX_CURSOR] = 0xffffffff;
            self.current_colors_4f[COLOR_INDEX_CURSOR] = Color4f::new(1.0, 1.0, 1.0, 1.0);
        } else {
            self.current_colors[COLOR_INDEX_CURSOR] = 0xff000000;
            self.current_colors_4f[COLOR_INDEX_CURSOR] = Color4f::new(0.0, 0.0, 0.0, 1.0);
        }
    }

    pub fn update_with_properties(&mut self, props: &std::collections::HashMap<String, String>) -> Result<(), String> {
        self.reset();
        let mut cursor_prop_exists = false;
        for (key, value) in props {
            let color_index = if key == "foreground" {
                COLOR_INDEX_FOREGROUND
            } else if key == "background" {
                COLOR_INDEX_BACKGROUND
            } else if key == "cursor" {
                cursor_prop_exists = true;
                COLOR_INDEX_CURSOR
            } else if key.starts_with("color") {
                let index_str = key.strip_prefix("color").unwrap();
                let idx = index_str.parse::<usize>().map_err(|_| format!("Invalid color index: {}", key))?;
                if idx >= COLOR_INDEX_FOREGROUND { return Err(format!("Index out of range: {}", idx)); }
                idx
            } else { continue; };
            if let Some((c32, c4f)) = Self::parse_color_full(value) {
                self.current_colors[color_index] = c32;
                self.current_colors_4f[color_index] = c4f;
            } else {
                return Err(format!("Invalid color value for '{}': '{}'", key, value));
            }
        }
        if !cursor_prop_exists {
            self.set_cursor_color_for_background();
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.current_colors = DEFAULT_COLORSCHEME;
        for i in 0..259 {
            self.current_colors_4f[i] = Color4f::from(skia_safe::Color::new(DEFAULT_COLORSCHEME[i]));
        }
    }

    pub fn reset_index(&mut self, index: usize) {
        if index < 259 {
            self.current_colors[index] = DEFAULT_COLORSCHEME[index];
            self.current_colors_4f[index] = Color4f::from(skia_safe::Color::new(DEFAULT_COLORSCHEME[index]));
        }
    }

    pub fn try_parse_color(&mut self, index: usize, color_str: &str) -> bool {
        if let Some((c32, c4f)) = Self::parse_color_full(color_str) {
            if index < 259 {
                self.current_colors[index] = c32;
                self.current_colors_4f[index] = c4f;
                return true;
            }
        }
        false
    }

    pub fn generate_color_report(&self, index: usize) -> String {
        if index >= 259 { return String::new(); }
        let color = self.current_colors[index];
        let r = (((color >> 16) & 0xff) as u16 * 65535) / 255;
        let g = (((color >> 8) & 0xff) as u16 * 65535) / 255;
        let b = ((color & 0xff) as u16 * 65535) / 255;
        format!("rgb:{:04x}/{:04x}/{:04x}", r, g, b)
    }
}

fn parse_rgb_component_f32(s: &str) -> Option<f32> {
    let len = s.len();
    if len == 0 || len > 4 { return None; }
    let value = u16::from_str_radix(s, 16).ok()?;
    match len {
        1 => Some((value * 17) as f32 / 255.0),
        2 => Some(value as f32 / 255.0),
        3 => Some(value as f32 / 4095.0),
        4 => Some(value as f32 / 65535.0),
        _ => None,
    }
}

impl Default for TerminalColors {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_colors() {
        assert_eq!(TerminalColors::parse_color("#000"), Some(0xff000000));
        assert_eq!(TerminalColors::parse_color("#ffffff"), Some(0xffffffff));
        
        let (_, c4f) = TerminalColors::parse_color_full("#RRRRGGGGBBBB".replace('R',"f").replace('G',"0").replace('B',"f").as_str()).unwrap();
        assert_eq!(c4f.r, 1.0);
        assert_eq!(c4f.g, 0.0);
        assert_eq!(c4f.b, 1.0);
    }
}
