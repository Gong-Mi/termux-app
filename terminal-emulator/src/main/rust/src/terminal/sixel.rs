use crate::vte_parser::Params;

/// Sixel 解码器状态
#[derive(Debug, Clone, PartialEq)]
pub enum SixelState {
    /// 地面状态，等待 DCS 序列开始
    Ground,
    /// 参数解析状态
    Param,
    /// Sixel 数据解析状态
    Data,
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
    pub color_registers: Vec<Option<(u8, u8, u8)>>,
    /// 当前行位置
    pub current_row: usize,
    /// 当前列位置
    pub current_col: usize,
    /// 纵横比参数
    pub aspect_ratio: (u32, u32),
    /// 图形原点模式
    pub origin_mode: bool,
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

        for &byte in data {
            match byte {
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
                                self.pixel_data[pixel_row][self.current_col] = self.current_color as u8;
                            }
                        }
                    }
                    self.current_col += 1;
                }
                b'!' => {
                    self.current_row += 6;
                    self.current_col = 0;
                    while self.current_row + 6 > self.pixel_data.len() {
                        self.pixel_data.push(vec![0u8; self.width.max(1)]);
                    }
                }
                b'#' => {}
                b'$' => { self.current_col = 0; }
                b'*' => {}
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
        }
        self.height = self.pixel_data.len() * 6;
    }

    pub fn finish(&mut self) {
        self.state = SixelState::Ground;
    }

    pub fn get_image_data(&self) -> Vec<u8> {
        let mut rgba_data = Vec::new();
        for row in &self.pixel_data {
            for &pixel in row {
                rgba_data.push(pixel); rgba_data.push(pixel); rgba_data.push(pixel); rgba_data.push(255);
            }
        }
        rgba_data
    }
}

impl Default for SixelDecoder {
    fn default() -> Self {
        Self::new()
    }
}
