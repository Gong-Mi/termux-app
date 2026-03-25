use crate::vte_parser::Params;

/// Sixel 颜色寄存器格式
#[derive(Debug, Clone)]
pub struct SixelColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Sixel 解码器状态
#[derive(Debug, Clone, PartialEq)]
pub enum SixelState {
    /// 地面状态，等待 DCS 序列开始
    Ground,
    /// 参数解析状态
    Param,
    /// Sixel 数据解析状态
    Data,
    /// 颜色参数解析状态
    ColorParam,
}

/// Sixel 图形解码器
pub struct SixelDecoder {
    /// 当前状态
    pub state: SixelState,
    /// 解析的参数
    pub params: Vec<i32>,
    /// 当前参数索引
    pub param_index: usize,
    /// 像素数据（每行）- 每个 u8 代表一个 sixel 行（6 像素）
    pub pixel_data: Vec<Vec<u8>>,
    /// 当前颜色索引
    pub current_color: usize,
    /// 图像宽度（sixel 单位）
    pub width: usize,
    /// 图像高度（sixel 单位）
    pub height: usize,
    /// 起始 X 坐标
    pub start_x: i32,
    /// 起始 Y 坐标
    pub start_y: i32,
    /// 是否透明背景
    pub transparent: bool,
    /// 颜色寄存器（最多 256 色）
    pub color_registers: Vec<Option<SixelColor>>,
    /// 当前行位置
    pub current_row: usize,
    /// 当前列位置
    pub current_col: usize,
    /// 纵横比参数
    pub aspect_ratio: (u32, u32),
    /// 图形原点模式
    pub origin_mode: bool,
    /// 颜色选择参数收集
    color_params: Vec<i32>,
}

impl SixelDecoder {
    pub fn new() -> Self {
        Self {
            state: SixelState::Ground,
            params: Vec::with_capacity(4),
            param_index: 0,
            pixel_data: Vec::new(),
            current_color: 0,
            width: 0,
            height: 0,
            start_x: 0,
            start_y: 0,
            transparent: false,
            color_registers: vec![None; 256],
            current_row: 0,
            current_col: 0,
            aspect_ratio: (1, 1),
            origin_mode: false,
            color_params: Vec::new(),
        }
    }

    /// 重置解码器状态
    pub fn reset(&mut self) {
        self.state = SixelState::Ground;
        self.params.clear();
        self.param_index = 0;
        self.pixel_data.clear();
        self.current_color = 0;
        self.width = 0;
        self.height = 0;
        self.current_row = 0;
        self.current_col = 0;
        self.origin_mode = false;
        self.color_params.clear();
    }

    /// 开始解析 DCS Sixel 序列
    pub fn start(&mut self, params: &Params) {
        self.reset();
        self.state = SixelState::Param;

        for param in params.iter() {
            self.params.push(param.first().copied().unwrap_or(-1));
        }

        if self.params.len() >= 1 && self.params[0] > 0 {
            self.width = self.params[0] as usize;
        }
        if self.params.len() >= 2 && self.params[1] > 0 {
            self.height = self.params[1] as usize;
        }
        if self.params.len() >= 3 {
            self.transparent = self.params[2] != 0;
        }
        if self.params.len() >= 5 {
            self.aspect_ratio = (self.params[3] as u32, self.params[4] as u32);
        }
        if self.params.len() >= 6 {
            self.origin_mode = self.params[5] != 0;
        }

        let sixel_rows = if self.height > 0 {
            (self.height + 5) / 6
        } else {
            100
        };
        self.pixel_data = vec![vec![0u8; self.width.max(1)]; sixel_rows];
    }

    pub fn process_data(&mut self, data: &[u8]) {
        self.state = SixelState::Data;
        if self.pixel_data.is_empty() {
            let default_width = self.width.max(100);
            let default_height = 100;
            let sixel_rows = (default_height + 5) / 6;
            self.pixel_data = vec![vec![0u8; default_width]; sixel_rows];
            if self.width == 0 {
                self.width = default_width;
            }
        }

        let mut i = 0;
        while i < data.len() {
            let byte = data[i];
            match byte {
                b'#' => {
                    // 颜色选择命令，需要收集后续参数
                    // 格式：# Pc ; Ps ; Pu ; Px ; Pm ; Pd ; Pr ; Pg ; Pb
                    // Pc = 颜色索引 (0-255)
                    // Ps = 颜色空间 (0=HLS, 1=RGB)
                    // 后续参数取决于颜色空间
                    self.color_params.clear();
                    let mut param_value: i32 = -1;
                    
                    i += 1;  // 跳过 '#'
                    while i < data.len() {
                        let b = data[i];
                        if b >= b'0' && b <= b'9' {
                            if param_value < 0 {
                                param_value = 0;
                            }
                            param_value = param_value * 10 + (b - b'0') as i32;
                            i += 1;
                        } else if b == b';' {
                            if param_value >= 0 {
                                self.color_params.push(param_value);
                            }
                            param_value = -1;
                            i += 1;
                        } else {
                            // 参数结束（遇到非数字非分号字符）
                            if param_value >= 0 {
                                self.color_params.push(param_value);
                            }
                            // 处理颜色选择
                            self.apply_color_select();
                            break;
                        }
                    }
                    
                    // 如果到数据末尾还有未处理的参数
                    if param_value >= 0 && i >= data.len() {
                        self.color_params.push(param_value);
                        self.apply_color_select();
                    }
                    continue;  // 跳过下面的 i += 1
                }
                48..=63 => {
                    let sixel_value = (byte - 48) as u8;
                    for bit in 0..6 {
                        let pixel_row = self.current_row + bit as usize;
                        if pixel_row < self.pixel_data.len() {
                            let mask = 1u8 << bit;
                            if (sixel_value & mask) != 0 {
                                if self.current_col >= self.pixel_data[pixel_row].len() {
                                    for row_data in &mut self.pixel_data {
                                        row_data.resize(self.current_col + 1, 0);
                                    }
                                    self.width = self.pixel_data[0].len();
                                }
                                // 使用颜色寄存器中的颜色
                                let color_index = self.current_color;
                                // 存储颜色索引，渲染时从颜色寄存器查找
                                self.pixel_data[pixel_row][self.current_col] = color_index as u8;
                            }
                        }
                    }
                    self.current_col += 1;
                }
                b'!' => {
                    // 清空图形并换行
                    self.current_row += 6;
                    self.current_col = 0;
                    while self.current_row + 6 > self.pixel_data.len() {
                        self.pixel_data.push(vec![0u8; self.width.max(1)]);
                    }
                }
                b'$' => { self.current_col = 0; }
                b'*' => {
                    // 重复最后一个 sixel 字符
                    if i > 0 {
                        let repeat_count = self.parse_repeat_count(data, &mut i);
                        for _ in 1..repeat_count {
                            // 重复绘制
                            if self.current_col < self.pixel_data.get(0).map(|r| r.len()).unwrap_or(0) {
                                let last_col = self.current_col.saturating_sub(1);
                                for bit in 0..6 {
                                    let pixel_row = self.current_row + bit as usize;
                                    if pixel_row < self.pixel_data.len() {
                                        self.pixel_data[pixel_row][self.current_col] = 
                                            self.pixel_data[pixel_row][last_col];
                                    }
                                }
                                self.current_col += 1;
                            }
                        }
                    }
                }
                b'~' => {
                    if self.current_row < self.pixel_data.len()
                        && self.current_col < self.pixel_data[self.current_row].len()
                    {
                        self.pixel_data[self.current_row][self.current_col] = 0;
                    }
                }
                b'\r' => { self.current_col = 0; }
                b'\n' => {
                    self.current_row += 6;
                    self.current_col = 0;
                }
                0x08 => {
                    if self.current_col > 0 { self.current_col -= 1; }
                }
                0x0C => {
                    for row in &mut self.pixel_data { row.fill(0); }
                    self.current_row = 0;
                    self.current_col = 0;
                }
                _ => {}
            }
            i += 1;
        }
        self.height = self.pixel_data.len() * 6;
    }

    /// 应用颜色选择
    fn apply_color_select(&mut self) {
        if !self.color_params.is_empty() {
            let color_index = self.color_params[0] as usize % 256;

            // 解析 RGB 值
            // 格式：# Pc ; Ps ; P1 ; P2 ; P3
            // Ps=0: HLS (H, L, S)
            // Ps=1: RGB (R, G, B) - 值 0-100 百分比
            let (r, g, b) = if self.color_params.len() >= 4 {
                let color_space = self.color_params[1] as u32;
                let p1 = self.color_params[2] as u32;
                let p2 = self.color_params[3] as u32;
                let p3 = self.color_params.get(4).copied().unwrap_or(0) as u32;
                
                if color_space == 1 {
                    // RGB 空间：P1=R, P2=G, P3=B (0-100 百分比)
                    ((p1 * 255 / 100) as u8, (p2 * 255 / 100) as u8, (p3 * 255 / 100) as u8)
                } else {
                    // HLS 空间：P1=H(0-360), P2=L(0-100), P3=S(0-100)
                    hls_to_rgb(p1, p2, p3)
                }
            } else if self.color_params.len() == 1 {
                // 只有颜色索引，使用默认颜色
                index_to_default_color(color_index)
            } else {
                // 部分参数，使用灰色
                let gray = (self.color_params.get(1).copied().unwrap_or(50) as u32).min(100);
                ((gray * 255 / 100) as u8, (gray * 255 / 100) as u8, (gray * 255 / 100) as u8)
            };

            // 设置颜色寄存器
            self.color_registers[color_index] = Some(SixelColor { r, g, b });
            self.current_color = color_index;
        } else {
            self.current_color = 0;
        }
        self.color_params.clear();
    }

    /// 解析重复计数（* 命令）
    fn parse_repeat_count(&self, data: &[u8], pos: &mut usize) -> usize {
        let mut count = 0;
        *pos += 1;
        while *pos < data.len() {
            let b = data[*pos];
            if b >= b'0' && b <= b'9' {
                count = count * 10 + (b - b'0') as usize;
                *pos += 1;
            } else {
                break;
            }
        }
        if count == 0 { count = 1; }
        count
    }

    pub fn finish(&mut self) {
        self.state = SixelState::Ground;
    }

    /// 获取渲染后的图像数据（RGBA 格式）
    pub fn get_image_data(&self) -> Vec<u8> {
        let mut rgba_data = Vec::new();
        
        for row in &self.pixel_data {
            for &pixel_index in row {
                // 从颜色寄存器获取颜色
                let (r, g, b, a) = if let Some(color) = &self.color_registers[pixel_index as usize] {
                    (color.r, color.g, color.b, 255)
                } else {
                    // 使用默认颜色
                    let (r, g, b) = index_to_default_color(pixel_index as usize);
                    (r, g, b, 255)
                };
                
                rgba_data.push(r);
                rgba_data.push(g);
                rgba_data.push(b);
                rgba_data.push(a);
            }
        }
        
        rgba_data
    }

    /// 获取颜色寄存器
    pub fn get_color_registers(&self) -> &Vec<Option<SixelColor>> {
        &self.color_registers
    }

    /// 设置颜色寄存器
    pub fn set_color(&mut self, index: usize, r: u8, g: u8, b: u8) {
        if index < 256 {
            self.color_registers[index] = Some(SixelColor { r, g, b });
        }
    }
}

impl Default for SixelDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// HLS 转 RGB 辅助函数
/// H: 0-360 (色相)
/// L: 0-100 (亮度)
/// S: 0-100 (饱和度)
pub fn hls_to_rgb(h: u32, l: u32, s: u32) -> (u8, u8, u8) {
    // 标准化
    let h_norm = (h % 360) as f32 / 360.0;
    let l_norm = l as f32 / 100.0;
    let s_norm = s as f32 / 100.0;
    
    if s_norm == 0.0 {
        // 无饱和度，灰色
        let gray = (l_norm * 255.0) as u8;
        return (gray, gray, gray);
    }
    
    let q = if l_norm < 0.5 {
        l_norm * (1.0 + s_norm)
    } else {
        l_norm + s_norm - l_norm * s_norm
    };
    let p = 2.0 * l_norm - q;
    
    let hue_to_rgb = |p: f32, q: f32, mut t: f32| -> f32 {
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
        if t < 1.0 / 2.0 { return q; }
        if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
        p
    };
    
    let r = (hue_to_rgb(p, q, h_norm + 1.0 / 3.0) * 255.0) as u8;
    let g = (hue_to_rgb(p, q, h_norm) * 255.0) as u8;
    let b = (hue_to_rgb(p, q, h_norm - 1.0 / 3.0) * 255.0) as u8;
    
    (r, g, b)
}

/// 根据颜色索引返回默认颜色（X11 颜色表）
pub fn index_to_default_color(index: usize) -> (u8, u8, u8) {
    // 简化的默认颜色表（前 16 色）
    const DEFAULT_COLORS: [(u8, u8, u8); 16] = [
        (0, 0, 0),       // 0: 黑色
        (170, 0, 0),     // 1: 红色
        (0, 170, 0),     // 2: 绿色
        (170, 170, 0),   // 3: 黄色
        (0, 0, 170),     // 4: 蓝色
        (170, 0, 170),   // 5: 品红
        (0, 170, 170),   // 6: 青色
        (170, 170, 170), // 7: 白色
        (85, 85, 85),    // 8: 亮黑
        (255, 85, 85),   // 9: 亮红
        (85, 255, 85),   // 10: 亮绿
        (255, 255, 85),  // 11: 亮黄
        (85, 85, 255),   // 12: 亮蓝
        (255, 85, 255),  // 13: 亮品红
        (85, 255, 255),  // 14: 亮青
        (255, 255, 255), // 15: 亮白
    ];
    
    if index < 16 {
        DEFAULT_COLORS[index]
    } else {
        // 其他索引使用灰色渐变
        let gray = ((index % 24) * 10 + 20) as u8;
        (gray, gray, gray)
    }
}
