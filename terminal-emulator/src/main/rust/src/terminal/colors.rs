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
}

impl TerminalColors {
    pub fn new() -> Self {
        Self { current_colors: DEFAULT_COLORSCHEME }
    }

    /// 解析颜色字符串，支持多种格式：
    /// - #RGB, #RRGGBB, #RRRGGGBBB, #RRRRGGGGBBBB
    /// - rgb:r/g/b (r/g/b 可以是 1-4 位十六进制)
    /// 
    /// 返回格式：0xFFRRGGBB
    pub fn parse_color(color_str: &str) -> Option<u32> {
        let color_str = color_str.trim_end_matches(|c| c == '\x07' || c == '\x1b' || c == '\\').trim();
        
        if color_str.starts_with('#') {
            // #RGB, #RRGGBB, #RRRGGGBBB, #RRRRGGGGBBBB
            let hex = &color_str[1..];
            match hex.len() {
                3 => {
                    // #RGB -> 每位重复
                    let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                    let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                    let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                    Some(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                }
                6 => {
                    // #RRGGBB
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    Some(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                }
                9 => {
                    // #RRRGGGBBB - 3 位每通道，取最高有效位
                    let r = u16::from_str_radix(&hex[0..3], 16).ok()?;
                    let g = u16::from_str_radix(&hex[3..6], 16).ok()?;
                    let b = u16::from_str_radix(&hex[6..9], 16).ok()?;
                    let r = ((r * 255) / 0xFFF) as u8;
                    let g = ((g * 255) / 0xFFF) as u8;
                    let b = ((b * 255) / 0xFFF) as u8;
                    Some(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                }
                12 => {
                    // #RRRRGGGGBBBB - 4 位每通道，取最高有效位
                    let r = u16::from_str_radix(&hex[0..4], 16).ok()?;
                    let g = u16::from_str_radix(&hex[4..8], 16).ok()?;
                    let b = u16::from_str_radix(&hex[8..12], 16).ok()?;
                    let r = ((r * 255) / 0xFFFF) as u8;
                    let g = ((g * 255) / 0xFFFF) as u8;
                    let b = ((b * 255) / 0xFFFF) as u8;
                    Some(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                }
                _ => None,
            }
        } else if color_str.starts_with("rgb:") {
            // rgb:r/g/b 格式，r/g/b 可以是 1-4 位十六进制
            let rgb_part = &color_str[4..];
            let parts: Vec<&str> = rgb_part.split('/').collect();
            if parts.len() != 3 {
                return None;
            }
            
            let r = parse_rgb_component(parts[0])?;
            let g = parse_rgb_component(parts[1])?;
            let b = parse_rgb_component(parts[2])?;
            Some(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
        } else {
            None
        }
    }

    /// 计算颜色的感知亮度 (0-255)
    /// 公式：sqrt(R^2 * 0.241 + G^2 * 0.691 + B^2 * 0.068)
    pub fn get_perceived_brightness(color: u32) -> u8 {
        let r = ((color >> 16) & 0xff) as f64;
        let g = ((color >> 8) & 0xff) as f64;
        let b = (color & 0xff) as f64;
        
        let brightness = (r * r * BRIGHTNESS_R_COEF 
                        + g * g * BRIGHTNESS_G_COEF 
                        + b * b * BRIGHTNESS_B_COEF).sqrt();
        
        brightness as u8
    }

    /// 根据背景颜色自动设置合适的光标颜色
    /// - 暗背景 -> 白色光标
    /// - 亮背景 -> 黑色光标
    pub fn set_cursor_color_for_background(&mut self) {
        let bg_color = self.current_colors[COLOR_INDEX_BACKGROUND];
        let brightness = Self::get_perceived_brightness(bg_color);
        
        if brightness < CURSOR_BRIGHTNESS_THRESHOLD {
            // 暗背景，使用白色光标
            self.current_colors[COLOR_INDEX_CURSOR] = 0xffffffff;
        } else {
            // 亮背景，使用黑色光标
            self.current_colors[COLOR_INDEX_CURSOR] = 0xff000000;
        }
    }

    /// 从 Properties 格式更新颜色配置
    /// 支持的键：foreground, background, cursor, color0-color255
    pub fn update_with_properties(&mut self, props: &std::collections::HashMap<String, String>) -> Result<(), String> {
        // 先重置为默认值
        self.reset();
        
        let mut cursor_prop_exists = false;
        
        for (key, value) in props {
            let color_index;
            
            if key == "foreground" {
                color_index = COLOR_INDEX_FOREGROUND;
            } else if key == "background" {
                color_index = COLOR_INDEX_BACKGROUND;
            } else if key == "cursor" {
                color_index = COLOR_INDEX_CURSOR;
                cursor_prop_exists = true;
            } else if key.starts_with("color") {
                let index_str = key.strip_prefix("color").ok_or(format!("Invalid key: {}", key))?;
                color_index = index_str.parse::<usize>()
                    .map_err(|_| format!("Invalid color index: {}", key))?;
                if color_index >= COLOR_INDEX_FOREGROUND {
                    return Err(format!("Color index out of range: {}", color_index));
                }
            } else {
                return Err(format!("Unknown property: {}", key));
            }
            
            let color_value = Self::parse_color(value)
                .ok_or_else(|| format!("Invalid color value for '{}': '{}'", key, value))?;
            
            self.current_colors[color_index] = color_value;
        }
        
        // 如果没有显式设置光标颜色，根据背景自动设置
        if !cursor_prop_exists {
            self.set_cursor_color_for_background();
        }
        
        Ok(())
    }

    pub fn reset(&mut self) {
        self.current_colors = DEFAULT_COLORSCHEME;
    }

    pub fn reset_index(&mut self, index: usize) {
        if index < 259 {
            self.current_colors[index] = DEFAULT_COLORSCHEME[index];
        }
    }

    pub fn try_parse_color(&mut self, index: usize, color_str: &str) -> bool {
        if let Some(color) = Self::parse_color(color_str) {
            if index < 259 {
                self.current_colors[index] = color;
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

/// 解析 rgb:r/g/b 格式中的单个分量
/// 支持 1-4 位十六进制，缩放到 0-255
fn parse_rgb_component(s: &str) -> Option<u8> {
    let len = s.len();
    if len == 0 || len > 4 {
        return None;
    }

    let value = u16::from_str_radix(s, 16).ok()?;

    // 根据位数缩放到 0-255
    // 1 位：0-F -> 0-255 (乘以 17)
    // 2 位：00-FF -> 0-255 (不变)
    // 3 位：000-FFF -> 0-255 (乘以 255 除以 4095)
    // 4 位：0000-FFFF -> 0-255 (乘以 255 除以 65535)
    let result = match len {
        1 => (value * 17) as u8,
        2 => value as u8,
        3 => ((value as u32 * 255) / 0xFFF) as u8,
        4 => ((value as u32 * 255) / 0xFFFF) as u8,
        _ => return None,
    };

    Some(result)
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
        // #RGB
        assert_eq!(TerminalColors::parse_color("#000"), Some(0xff000000));
        assert_eq!(TerminalColors::parse_color("#fff"), Some(0xffffffff));
        assert_eq!(TerminalColors::parse_color("#f00"), Some(0xffff0000));
        assert_eq!(TerminalColors::parse_color("#0f0"), Some(0xff00ff00));
        assert_eq!(TerminalColors::parse_color("#00f"), Some(0xff0000ff));
        
        // #RRGGBB
        assert_eq!(TerminalColors::parse_color("#000000"), Some(0xff000000));
        assert_eq!(TerminalColors::parse_color("#ffffff"), Some(0xffffffff));
        assert_eq!(TerminalColors::parse_color("#0000FA"), Some(0xff0000fa));
        assert_eq!(TerminalColors::parse_color("#53186f"), Some(0xff53186f));
        
        // #RRRGGGBBB
        assert_eq!(TerminalColors::parse_color("#000000000"), Some(0xff000000));
        assert_eq!(TerminalColors::parse_color("#FFF"), Some(0xffffffff)); // Falls back to 3-char
        
        // Invalid
        assert_eq!(TerminalColors::parse_color("#3456"), None);
        assert_eq!(TerminalColors::parse_color("invalid"), None);
    }

    #[test]
    fn test_parse_rgb_format() {
        // rgb:r/g/b with 1 digit
        assert_eq!(TerminalColors::parse_color("rgb:0/0/0"), Some(0xff000000));
        assert_eq!(TerminalColors::parse_color("rgb:f/f/f"), Some(0xffffffff));
        assert_eq!(TerminalColors::parse_color("rgb:f/0/0"), Some(0xffff0000));
        
        // rgb:r/g/b with 2 digits
        assert_eq!(TerminalColors::parse_color("rgb:00/00/00"), Some(0xff000000));
        assert_eq!(TerminalColors::parse_color("rgb:ff/ff/ff"), Some(0xffffffff));
        assert_eq!(TerminalColors::parse_color("rgb:00/00/FA"), Some(0xff0000fa));
        assert_eq!(TerminalColors::parse_color("rgb:53/18/6f"), Some(0xff53186f));
        
        // rgb:r/g/b with 4 digits
        assert_eq!(TerminalColors::parse_color("rgb:0000/0000/0000"), Some(0xff000000));
        assert_eq!(TerminalColors::parse_color("rgb:ffff/ffff/ffff"), Some(0xffffffff));
        assert_eq!(TerminalColors::parse_color("rgb:ffff/0000/ffff"), Some(0xffff00ff));
        
        // With trailing control chars (OSC termination)
        assert_eq!(TerminalColors::parse_color("rgb:f/0/f\x07"), Some(0xffff00ff));
        assert_eq!(TerminalColors::parse_color("rgb:f/0/f\x1b"), Some(0xffff00ff));
        assert_eq!(TerminalColors::parse_color("rgb:f/0/f\\"), Some(0xffff00ff));
        
        // Invalid
        assert_eq!(TerminalColors::parse_color("rgb:invalid"), None);
        assert_eq!(TerminalColors::parse_color("rgb:1/2"), None);
    }

    #[test]
    fn test_perceived_brightness() {
        // Black = 0 brightness
        assert_eq!(TerminalColors::get_perceived_brightness(0xff000000), 0);
        
        // White = max brightness (~255)
        let white_brightness = TerminalColors::get_perceived_brightness(0xffffffff);
        assert!(white_brightness > 250);
        
        // Green is perceived brighter than red
        let green = TerminalColors::get_perceived_brightness(0xff00ff00);
        let red = TerminalColors::get_perceived_brightness(0xffff0000);
        assert!(green > red);
        
        // Test threshold
        let dark_color = 0xff303030;
        let light_color = 0xffd0d0d0;
        assert!(TerminalColors::get_perceived_brightness(dark_color) < CURSOR_BRIGHTNESS_THRESHOLD);
        assert!(TerminalColors::get_perceived_brightness(light_color) > CURSOR_BRIGHTNESS_THRESHOLD);
    }

    #[test]
    fn test_cursor_color_auto_set() {
        let mut colors = TerminalColors::new();
        
        // Set dark background
        colors.current_colors[COLOR_INDEX_BACKGROUND] = 0xff101010;
        colors.set_cursor_color_for_background();
        assert_eq!(colors.current_colors[COLOR_INDEX_CURSOR], 0xffffffff); // White cursor
        
        // Set light background
        colors.current_colors[COLOR_INDEX_BACKGROUND] = 0xffeeeeee;
        colors.set_cursor_color_for_background();
        assert_eq!(colors.current_colors[COLOR_INDEX_CURSOR], 0xff000000); // Black cursor
    }

    #[test]
    fn test_update_with_properties() {
        let mut colors = TerminalColors::new();
        let mut props = std::collections::HashMap::new();
        
        props.insert("foreground".to_string(), "#ffffff".to_string());
        props.insert("background".to_string(), "#000000".to_string());
        props.insert("color1".to_string(), "#ff0000".to_string());
        
        colors.update_with_properties(&props).unwrap();
        
        assert_eq!(colors.current_colors[COLOR_INDEX_FOREGROUND], 0xffffffff);
        assert_eq!(colors.current_colors[COLOR_INDEX_BACKGROUND], 0xff000000);
        assert_eq!(colors.current_colors[1], 0xffff0000);
        
        // Cursor should be auto-set to white (dark background)
        assert_eq!(colors.current_colors[COLOR_INDEX_CURSOR], 0xffffffff);
    }

    #[test]
    fn test_update_with_properties_cursor_override() {
        let mut colors = TerminalColors::new();
        let mut props = std::collections::HashMap::new();
        
        props.insert("background".to_string(), "#000000".to_string());
        props.insert("cursor".to_string(), "#00ff00".to_string());
        
        colors.update_with_properties(&props).unwrap();
        
        // Cursor should be the specified green, not auto-set
        assert_eq!(colors.current_colors[COLOR_INDEX_CURSOR], 0xff00ff00);
    }

    #[test]
    fn test_reset() {
        let mut colors = TerminalColors::new();
        
        // Modify colors
        colors.current_colors[0] = 0xffffffff;
        colors.current_colors[COLOR_INDEX_FOREGROUND] = 0x00000000;
        
        // Reset
        colors.reset();
        
        // Should be back to defaults
        assert_eq!(colors.current_colors[0], DEFAULT_COLORSCHEME[0]);
        assert_eq!(colors.current_colors[COLOR_INDEX_FOREGROUND], DEFAULT_COLORSCHEME[COLOR_INDEX_FOREGROUND]);
    }
}
