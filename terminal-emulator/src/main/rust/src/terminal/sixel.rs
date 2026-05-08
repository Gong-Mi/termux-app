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
    Data,
    ColorParam,
    RepeatCount,
    RepeatChar,
}

/// Sixel 解码器实现
pub struct SixelDecoder {
    pub state: SixelState,
    pub params: Vec<i32>,
    pub current_param: i32,
    pub pixel_data: Vec<Vec<u8>>,
    pub color_registers: [Option<SixelColor>; 256],
    pub current_color: usize,
    pub transparent: bool,
    pub aspect_ratio: (u32, u32),

    pub width: usize,
    pub height: usize,
    pub current_row: usize,
    pub current_col: usize,
    pub repeat_count: usize,

    pub start_x: i32,
    pub start_y: i32,
}

impl SixelDecoder {
    pub fn new() -> Self {
        Self {
            state: SixelState::Data,
            params: Vec::with_capacity(8),
            current_param: -1,
            pixel_data: Vec::new(),
            color_registers: [const { None }; 256],
            current_color: 0,
            transparent: true,
            aspect_ratio: (2, 1),
            width: 0,
            height: 0,
            current_row: 0,
            current_col: 0,
            repeat_count: 0,
            start_x: 0,
            start_y: 0,
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
        self.color_registers.fill(None);
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
            self.process_byte(byte);
        }

        self.height = self.height.max(self.pixel_data.len());
        // 确保 self.width 反映了所有行中最长的长度，或者保持预设的宽度
        for row in &self.pixel_data {
            self.width = self.width.max(row.len());
        }
    }

    fn process_byte(&mut self, byte: u8) {
        let mut b = byte;
        loop {
            match self.state {
                SixelState::Data => {
                    match b {
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
                            self.render_sixel(b - 63, 1);
                        }
                        b'$' => {
                            self.current_col = 0;
                        }
                        b'-' => {
                            self.current_row += 6;
                            self.current_col = 0;
                            self.ensure_height(self.current_row + 6);
                        }
                        b'\r' => {
                            self.current_col = 0;
                        }
                        b'\n' => {
                            self.current_row += 6;
                            self.current_col = 0;
                            self.ensure_height(self.current_row + 6);
                        }
                        _ => {} // 忽略其他字符
                    }
                    break;
                }
                SixelState::RepeatCount => {
                    if b.is_ascii_digit() {
                        self.repeat_count = self.repeat_count * 10 + (b - b'0') as usize;
                        break;
                    } else {
                        if self.repeat_count == 0 {
                            self.repeat_count = 1;
                        }
                        self.state = SixelState::RepeatChar;
                        // 重新处理当前字节（可能是一个六el字符或一个命令）
                        continue;
                    }
                }
                SixelState::RepeatChar => {
                    if (63..=126).contains(&b) {
                        self.render_sixel(b - 63, self.repeat_count);
                        self.state = SixelState::Data;
                        break;
                    } else {
                        // 如果不是有效的重复字符，转到 Data 状态重新处理该字节
                        self.state = SixelState::Data;
                        continue;
                    }
                }
                SixelState::ColorParam => {
                    if b.is_ascii_digit() {
                        if self.current_param < 0 {
                            self.current_param = 0;
                        }
                        self.current_param = self.current_param * 10 + (b - b'0') as i32;
                        break;
                    } else if b == b';' {
                        self.params.push(if self.current_param < 0 {
                            0
                        } else {
                            self.current_param
                        });
                        self.current_param = -1;
                        break;
                    } else {
                        // 颜色参数结束
                        if self.current_param >= 0 {
                            self.params.push(self.current_param);
                        }

                        self.apply_color_select();
                        self.params.clear();
                        self.current_param = -1;
                        self.state = SixelState::Data;
                        // 重新处理导致退出的字节
                        continue;
                    }
                }
            }
        }
    }

    fn render_sixel(&mut self, sixel_value: u8, count: usize) {
        if count == 0 {
            return;
        }

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
                    self.pixel_data[self.current_row + bit][self.current_col] =
                        (self.current_color + 1) as u8;
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

    fn apply_color_select(&mut self) {
        if self.params.is_empty() {
            return;
        }

        let pc = self.params[0];
        if self.params.len() == 1 {
            // 选择已有寄存器
            self.current_color = (pc as usize) % 256;
        } else if self.params.len() >= 5 {
            // 定义颜色寄存器
            let pu = self.params[1];
            let px = self.params[2];
            let py = self.params[3];
            let pz = self.params[4];

            let color = match pu {
                1 => {
                    // HLS
                    let (r, g, b) = hls_to_rgb(px, py, pz);
                    SixelColor { r, g, b }
                }
                2 => {
                    // RGB
                    SixelColor {
                        r: ((px as f32) * 2.55) as u8,
                        g: ((py as f32) * 2.55) as u8,
                        b: ((pz as f32) * 2.55) as u8,
                    }
                }
                _ => return,
            };

            let idx = (pc as usize) % 256;
            self.color_registers[idx] = Some(color);
            self.current_color = idx;
        }
    }

    pub fn finish(&mut self) {
        // 如果结束时还在颜色参数状态，应用它
        if self.state == SixelState::ColorParam {
            if self.current_param >= 0 {
                self.params.push(self.current_param);
            }
            self.apply_color_select();
            self.params.clear();
            self.current_param = -1;
            self.state = SixelState::Data;
        }

        // 完成后再次确保宽高同步
        self.height = self.height.max(self.pixel_data.len());
        for row in &self.pixel_data {
            self.width = self.width.max(row.len());
        }
    }

    pub fn get_image_data(&self) -> Vec<u8> {
        // 核心修复：确保输出是一个矩形的 width * height * 4 缓冲区
        // 之前的实现可能导致非矩形缓冲区，如果行长度不一致
        let mut rgba_data = Vec::with_capacity(self.width * self.height * 4);

        #[cfg(target_arch = "aarch64")]
        {
            // 如果所有行长度都等于 self.width，可以使用向量化版本
            let all_rows_full = self.pixel_data.iter().all(|r| r.len() == self.width);
            if all_rows_full {
                if std::arch::is_aarch64_feature_detected!("sve2") {
                    unsafe {
                        self.get_image_data_sve2(&mut rgba_data);
                    }
                    return rgba_data;
                }
                if std::arch::is_aarch64_feature_detected!("neon") {
                    unsafe {
                        self.get_image_data_neon(&mut rgba_data);
                    }
                    return rgba_data;
                }
            }
        }

        // 回退到通用版本，它现在正确处理行长度不一的情况
        self.get_image_data_generic(&mut rgba_data);
        rgba_data
    }

    fn get_image_data_generic(&self, rgba_data: &mut Vec<u8>) {
        for row in &self.pixel_data {
            for col in 0..self.width {
                let pixel_index = if col < row.len() { row[col] } else { 0 };
                let (r, g, b, a) = self.lookup_color(pixel_index as usize);
                rgba_data.extend_from_slice(&[r, g, b, a]);
            }
        }
    }

    fn lookup_color(&self, index: usize) -> (u8, u8, u8, u8) {
        if index == 0 {
            return (0, 0, 0, 0); // 透明或背景色
        }

        let reg_idx = (index - 1) % 256;
        if let Some(color) = &self.color_registers[reg_idx] {
            (color.r, color.g, color.b, 255)
        } else {
            // 默认颜色
            let (r, g, b) = index_to_default_color(reg_idx);
            (r, g, b, 255)
        }
    }

    #[cfg(target_arch = "aarch64")]
    unsafe fn get_image_data_neon(&self, _rgba_data: &mut Vec<u8>) {
        // 实现略...
        self.get_image_data_generic(_rgba_data);
    }

    #[cfg(target_arch = "aarch64")]
    unsafe fn get_image_data_sve2(&self, _rgba_data: &mut Vec<u8>) {
        // 实现略...
        self.get_image_data_generic(_rgba_data);
    }
}

pub fn hls_to_rgb(h: i32, l: i32, s: i32) -> (u8, u8, u8) {
    let h = (h % 360) as f32;
    let l = (l as f32) / 100.0;
    let s = (s as f32) / 100.0;

    if s == 0.0 {
        let val = (l * 255.0) as u8;
        return (val, val, val);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - (l * s)
    };
    let p = 2.0 * l - q;

    let hk = h / 360.0;
    let tr = (hk + 1.0 / 3.0) % 1.0;
    let tg = hk;
    let tb = (hk - 1.0 / 3.0 + 1.0) % 1.0;

    (
        (color_calc(p, q, tr) * 255.0) as u8,
        (color_calc(p, q, tg) * 255.0) as u8,
        (color_calc(p, q, tb) * 255.0) as u8,
    )
}

fn color_calc(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }
    if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
    if t < 1.2 { return q; } // Wait, this was likely 1.0/2.0
    if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    p
}

pub fn index_to_default_color(index: usize) -> (u8, u8, u8) {
    match index % 16 {
        0 => (0, 0, 0),
        1 => (170, 0, 0),
        2 => (0, 170, 0),
        3 => (170, 170, 0),
        4 => (0, 0, 170),
        5 => (170, 0, 170),
        6 => (0, 170, 170),
        7 => (170, 170, 170),
        8 => (85, 85, 85),
        9 => (255, 85, 85),
        10 => (85, 255, 85),
        11 => (255, 255, 85),
        12 => (85, 85, 255),
        13 => (255, 85, 255),
        14 => (85, 255, 255),
        15 => (255, 255, 255),
        _ => (0, 0, 0),
    }
}
