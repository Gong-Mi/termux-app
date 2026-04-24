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
    /// 地面状态，解析 Sixel 数据或命令开始
    Data,
    /// 重复次数解析状态 (!)
    RepeatCount,
    /// 重复字符等待状态
    RepeatChar,
    /// 颜色参数解析状态 (#)
    ColorParam,
}

/// Sixel 图形解码器
pub struct SixelDecoder {
    /// 当前状态
    pub state: SixelState,
    /// 参数解析暂存
    pub params: Vec<i32>,
    /// 当前参数值
    pub current_param: i32,
    /// 像素数据（每行）- 存储颜色索引
    pub pixel_data: Vec<Vec<u8>>,
    /// 当前颜色索引
    pub current_color: usize,
    /// 图像宽度（像素）
    pub width: usize,
    /// 图像高度（像素）
    pub height: usize,
    /// 起始 X 坐标
    pub start_x: i32,
    /// 起始 Y 坐标
    pub start_y: i32,
    /// 是否透明背景
    pub transparent: bool,
    /// 颜色寄存器（最多 256 色）
    pub color_registers: Vec<Option<SixelColor>>,
    /// 当前行位置 (pixel)
    pub current_row: usize,
    /// 当前列位置 (pixel)
    pub current_col: usize,
    /// 纵横比参数
    pub aspect_ratio: (u32, u32),
    /// 图形原点模式
    pub origin_mode: bool,
    /// 重复计数暂存
    repeat_count: usize,
}

impl SixelDecoder {
    pub fn new() -> Self {
        Self {
            state: SixelState::Data,
            params: Vec::with_capacity(8),
            current_param: -1,
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
            repeat_count: 0,
        }
    }

    /// 重置解码器状态
    pub fn reset(&mut self) {
        self.state = SixelState::Data;
        self.params.clear();
        self.current_param = -1;
        self.pixel_data.clear();
        self.current_color = 0;
        self.width = 0;
        self.height = 0;
        self.current_row = 0;
        self.current_col = 0;
        self.repeat_count = 0;
    }

    /// 开始解析 DCS Sixel 序列
    pub fn start(&mut self, params: &Params) {
        self.reset();
        
        // 解析 DCS 参数：[Pq; Pi; Pa]
        // Pq: 纵横比 (0, 1, 7, 8, 9)
        // Pi: 背景透明 (1=不透明, 0/2=透明)
        // Pa: 水平网格大小
        let pq = params.get(0, 0);
        let pi = params.get(1, 0);
        
        self.transparent = pi != 1;
        self.aspect_ratio = match pq {
            0 | 1 => (2, 1), // 2:1
            2 => (5, 1),
            3 | 4 => (3, 1),
            5 | 6 => (2, 1),
            7 | 8 | 9 => (1, 1),
            _ => (2, 1),
        };

        // 预分配一些行
        self.pixel_data = vec![vec![0u8; 1]; 6];
    }

    pub fn process_data(&mut self, data: &[u8]) {
        for &byte in data {
            match self.state {
                SixelState::Data => {
                    match byte {
                        b'#' => {
                            self.params.clear();
                            self.current_param = -1;
                            self.state = SixelState::ColorParam;
                        }
                        b'!' => {
                            self.repeat_count = 0;
                            self.state = SixelState::RepeatCount;
                        }
                        63..=126 => {
                            // 正常的 Sixel 数据
                            self.render_sixel(byte - 63, 1);
                        }
                        b'$' => {
                            self.current_col = 0;
                        }
                        b'-' => {
                            self.current_row += 6;
                            self.current_col = 0;
                            self.ensure_height(self.current_row + 6);
                        }
                        b'\r' => { self.current_col = 0; }
                        b'\n' => {
                            self.current_row += 6;
                            self.current_col = 0;
                            self.ensure_height(self.current_row + 6);
                        }
                        _ => {} // 忽略其他字符
                    }
                }
                SixelState::RepeatCount => {
                    if byte.is_ascii_digit() {
                        self.repeat_count = self.repeat_count * 10 + (byte - b'0') as usize;
                    } else {
                        if self.repeat_count == 0 { self.repeat_count = 1; }
                        self.state = SixelState::RepeatChar;
                        // 重新处理当前字节（作为重复的目标字符）
                        if (63..=126).contains(&byte) {
                            self.render_sixel(byte - 63, self.repeat_count);
                            self.state = SixelState::Data;
                        }
                    }
                }
                SixelState::RepeatChar => {
                    if (63..=126).contains(&byte) {
                        self.render_sixel(byte - 63, self.repeat_count);
                    }
                    self.state = SixelState::Data;
                }
                SixelState::ColorParam => {
                    if byte.is_ascii_digit() {
                        if self.current_param < 0 { self.current_param = 0; }
                        self.current_param = self.current_param * 10 + (byte - b'0') as i32;
                    } else if byte == b';' {
                        self.params.push(if self.current_param < 0 { 0 } else { self.current_param });
                        self.current_param = -1;
                    } else {
                        // 颜色参数结束
                        if self.current_param >= 0 {
                            self.params.push(self.current_param);
                        }
                        self.apply_color_select();
                        self.state = SixelState::Data;
                        // 重新处理导致退出的字节
                        if (63..=126).contains(&byte) {
                            self.render_sixel(byte - 63, 1);
                        } else if byte == b'$' {
                            self.current_col = 0;
                        } else if byte == b'-' {
                            self.current_row += 6;
                            self.current_col = 0;
                            self.ensure_height(self.current_row + 6);
                        }
                    }
                }
            }
        }
        self.height = self.pixel_data.len();
        self.width = self.pixel_data.get(0).map(|r| r.len()).unwrap_or(0);
    }

    fn render_sixel(&mut self, sixel_value: u8, count: usize) {
        if count == 0 { return; }
        
        let target_col = self.current_col + count;
        
        // 确保高度
        self.ensure_height(self.current_row + 6);
        
        // 确保宽度
        for row in self.current_row..self.current_row + 6 {
            if self.pixel_data[row].len() < target_col {
                self.pixel_data[row].resize(target_col, 0);
            }
        }

        for _ in 0..count {
            for bit in 0..6 {
                if (sixel_value & (1 << bit)) != 0 {
                    self.pixel_data[self.current_row + bit][self.current_col] = (self.current_color + 1) as u8;
                }
            }
            self.current_col += 1;
        }
        
        if self.current_col > self.width {
            self.width = self.current_col;
        }
    }

    fn ensure_height(&mut self, height: usize) {
        while self.pixel_data.len() < height {
            self.pixel_data.push(vec![0u8; self.width.max(1)]);
        }
    }

    /// 应用颜色选择
    fn apply_color_select(&mut self) {
        if self.params.is_empty() { return; }
        
        let color_index = self.params[0] as usize % 256;

        if self.params.len() >= 4 {
            let color_space = self.params[1];
            let p1 = self.params[2] as u32;
            let p2 = self.params[3] as u32;
            let p3 = self.params.get(4).copied().unwrap_or(0) as u32;
            
            let (r, g, b) = if color_space == 2 {
                // RGB 空间：值 0-100 百分比
                ((p1 * 255 / 100).min(255) as u8, (p2 * 255 / 100).min(255) as u8, (p3 * 255 / 100).min(255) as u8)
            } else if color_space == 1 {
                // HLS 空间
                hls_to_rgb(p1, p2, p3)
            } else {
                (128, 128, 128)
            };
            self.color_registers[color_index] = Some(SixelColor { r, g, b });
        }
        
        self.current_color = color_index;
    }

    pub fn finish(&mut self) {
        self.state = SixelState::Data;
    }

    /// 获取渲染后的图像数据（RGBA 格式）- 具备硬件加速
    pub fn get_image_data(&self) -> Vec<u8> {
        let total_pixels: usize = self.pixel_data.iter().map(|r| r.len()).sum();
        let mut rgba_data = Vec::with_capacity(total_pixels * 4);

        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("sve2") {
                unsafe { self.get_image_data_sve2(&mut rgba_data); }
                return rgba_data;
            }
            if std::arch::is_aarch64_feature_detected!("neon") {
                unsafe { self.get_image_data_neon(&mut rgba_data); }
                return rgba_data;
            }
        }

        // 回退到通用版本
        self.get_image_data_generic(&mut rgba_data);
        rgba_data
    }

    /// 通用版本：兼容所有架构
    fn get_image_data_generic(&self, rgba_data: &mut Vec<u8>) {
        for row in &self.pixel_data {
            for &pixel_index in row {
                let (r, g, b, a) = self.lookup_color(pixel_index as usize);
                rgba_data.extend_from_slice(&[r, g, b, a]);
            }
        }
    }

    /// NEON 优化版本：利用 128 位向量化
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn get_image_data_neon(&self, rgba_data: &mut Vec<u8>) {
        // 预计算颜色表以加速查找
        let color_table = self.build_fast_color_table();
        
        for row in &self.pixel_data {
            let mut chunks = row.chunks_exact(16);
            for chunk in &mut chunks {
                // 编译器现在可以安全地利用 NEON 指令进行向量化
                for &idx in chunk {
                    let c = color_table[idx as usize];
                    rgba_data.extend_from_slice(&c);
                }
            }
            // 处理剩余像素
            for &idx in chunks.remainder() {
                rgba_data.extend_from_slice(&color_table[idx as usize]);
            }
        }
    }

    /// SVE2 优化版本：利用可变长向量指令
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "sve2")]
    unsafe fn get_image_data_sve2(&self, rgba_data: &mut Vec<u8>) {
        let color_table = self.build_fast_color_table();
        
        for row in &self.pixel_data {
            // 在 SVE2 环境下，编译器会自动识别并应用更宽的向量指令（如 256/512 bit）
            for &idx in row {
                let c = color_table[idx as usize];
                rgba_data.extend_from_slice(&c);
            }
        }
    }

    /// 辅助：构建 256 色的快速查找表 [R, G, B, A]
    fn build_fast_color_table(&self) -> [[u8; 4]; 256] {
        let mut table = [[0u8; 4]; 256];
        for i in 0..256 {
            let (r, g, b) = if let Some(color) = &self.color_registers[i] {
                (color.r, color.g, color.b)
            } else {
                index_to_default_color(i)
            };
            table[i] = [r, g, b, 255];
        }
        table
    }

    /// 辅助：颜色查找逻辑封装
    #[inline(always)]
    fn lookup_color(&self, index: usize) -> (u8, u8, u8, u8) {
        if let Some(color) = &self.color_registers[index % 256] {
            (color.r, color.g, color.b, 255)
        } else {
            let (r, g, b) = index_to_default_color(index % 256);
            (r, g, b, 255)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sixel_basic_decoding() {
        let mut decoder = SixelDecoder::new();
        // Sixel data: ~ (63+63=126) which is all 6 bits set
        decoder.process_data(b"~");
        assert_eq!(decoder.width, 1);
        assert_eq!(decoder.height, 6);
        // color index 0 maps to pixel value 1
        assert_eq!(decoder.pixel_data[0][0], 1);
        assert_eq!(decoder.pixel_data[5][0], 1);
    }

    #[test]
    fn test_sixel_rle_decoding() {
        let mut decoder = SixelDecoder::new();
        // ! 5 ~ -> repeat ~ 5 times
        decoder.process_data(b"!5~");
        assert_eq!(decoder.width, 5);
        for i in 0..5 {
            assert_eq!(decoder.pixel_data[0][i], 1);
        }
    }

    #[test]
    fn test_sixel_newline() {
        let mut decoder = SixelDecoder::new();
        // ~ - ~ -> one pixel, next line (6 pixels down), one pixel
        decoder.process_data(b"~-~");
        assert_eq!(decoder.height, 12);
        assert_eq!(decoder.pixel_data[0][0], 1);
        assert_eq!(decoder.pixel_data[6][0], 1);
    }

    #[test]
    fn test_sixel_color_select() {
        let mut decoder = SixelDecoder::new();
        // #1;2;100;0;0 -> color 1 = RGB 100%,0%,0% (Red)
        decoder.process_data(b"#1;2;100;0;0#1~");
        assert_eq!(decoder.current_color, 1);
        let color = decoder.color_registers[1].as_ref().unwrap();
        assert_eq!(color.r, 255);
        assert_eq!(color.g, 0);
        assert_eq!(color.b, 0);
        assert_eq!(decoder.pixel_data[0][0], 2); // index+1
    }

    #[test]
    fn test_sixel_fragmented_data() {
        let mut decoder = SixelDecoder::new();
        // Data split across chunks
        decoder.process_data(b"!");
        decoder.process_data(b"1");
        decoder.process_data(b"0");
        decoder.process_data(b"~");
        assert_eq!(decoder.width, 10);
        assert_eq!(decoder.pixel_data[0][9], 1);
    }
}
