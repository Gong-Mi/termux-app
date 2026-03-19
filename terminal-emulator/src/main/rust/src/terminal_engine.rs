//! 终端引擎模块

use vte::{Params, Parser, Perform};

// 单元格结构
#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub char: char,
    pub fg_color: Option<(u8, u8, u8)>,
    pub bg_color: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub underline: bool,
    pub italic: bool,
    pub reverse: bool,
}

// 终端引擎
pub struct TerminalEngine {
    pub cols: i32,
    pub rows: i32,
    pub parser: Parser,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub cells: Vec<Vec<Cell>>,
    
    // 样式状态
    pub fg_color: Option<(u8, u8, u8)>,
    pub bg_color: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub underline: bool,
    pub italic: bool,
    pub reverse: bool,
    
    // 模式状态
    pub application_cursor_keys: bool,
}

fn default_cell() -> Cell {
    Cell::default()
}

impl TerminalEngine {
    pub fn new(cols: i32, rows: i32) -> Self {
        let cells = vec![vec![default_cell(); cols as usize]; rows as usize];
        
        Self {
            cols,
            rows,
            parser: Parser::new(),
            cursor_x: 0,
            cursor_y: 0,
            cells,
            fg_color: None,
            bg_color: None,
            bold: false,
            underline: false,
            italic: false,
            reverse: false,
            application_cursor_keys: false,
        }
    }

    pub fn resize(&mut self, new_cols: i32, new_rows: i32) {
        self.cols = new_cols;
        self.rows = new_rows;
        self.cells = vec![vec![default_cell(); new_cols as usize]; new_rows as usize];
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    pub fn get_cell(&self, col: usize, row: usize) -> &Cell {
        if row < self.cells.len() && col < self.cells[row].len() {
            &self.cells[row][col]
        } else {
            static DEFAULT: Option<Cell> = None;
            DEFAULT.as_ref().unwrap()
        }
    }

    pub fn parse_bytes(&mut self, bytes: &[u8]) {
        // 使用原始指针绕过借用检查器
        unsafe {
            let this = self as *mut TerminalEngine;
            (*this).parser.advance(&mut *this, bytes);
        }
    }

    fn put_char(&mut self, c: char) {
        let col = self.cursor_x as usize;
        let row = self.cursor_y as usize;

        if row < self.cells.len() && col < self.cells[row].len() {
            self.cells[row][col] = Cell {
                char: c,
                fg_color: self.fg_color,
                bg_color: self.bg_color,
                bold: self.bold,
                underline: self.underline,
                italic: self.italic,
                reverse: self.reverse,
            };

            // 光标右移
            self.cursor_x += 1;
            if self.cursor_x >= self.cols {
                self.cursor_x = 0;
                self.cursor_y += 1;
                if self.cursor_y >= self.rows {
                    self.scroll_up();
                    self.cursor_y = self.rows - 1;
                }
            }
        }
    }

    fn scroll_up(&mut self) {
        self.cells.remove(0);
        self.cells.push(vec![default_cell(); self.cols as usize]);
    }
}

// VTE Perform 实现
impl Perform for TerminalEngine {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | b'\x0b' | b'\x0c' => {
                self.cursor_y += 1;
                if self.cursor_y >= self.rows {
                    self.scroll_up();
                    self.cursor_y = self.rows - 1;
                }
            }
            b'\r' => {
                self.cursor_x = 0;
            }
            b'\x08' => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            b'\t' => {
                // 前进到下一个制表位（每 8 列）
                self.cursor_x = ((self.cursor_x / 8) + 1) * 8;
                if self.cursor_x >= self.cols {
                    self.cursor_x = self.cols - 1;
                }
            }
            b'\x07' => {
                // Bell - 忽略
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        // DCS 序列 - 简化处理
    }

    fn put(&mut self, _byte: u8) {
        // DCS 数据 - 简化处理
    }

    fn unhook(&mut self) {
        // DCS 结束 - 简化处理
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        // OSC 序列 - 简化处理
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        match action {
            'A' => {
                // CUU - 光标上移
                let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                self.cursor_y = (self.cursor_y - n).max(0);
            }
            'B' => {
                // CUD - 光标下移
                let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                self.cursor_y = (self.cursor_y + n).min(self.rows - 1);
            }
            'C' => {
                // CUF - 光标右移
                let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                self.cursor_x = (self.cursor_x + n).min(self.cols - 1);
            }
            'D' => {
                // CUB - 光标左移
                let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                self.cursor_x = (self.cursor_x - n).max(0);
            }
            'H' | 'f' => {
                // CUP - 光标定位
                let mut iter = params.iter();
                let row = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                let col = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                self.cursor_y = (row - 1).max(0).min(self.rows - 1);
                self.cursor_x = (col - 1).max(0).min(self.cols - 1);
            }
            'J' => {
                // ED - 清屏
                let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0);
                match mode {
                    0 => {
                        // 从光标到屏幕末尾
                        for row in self.cursor_y as usize..self.rows as usize {
                            for col in (if row == self.cursor_y as usize { self.cursor_x as usize } else { 0 })..self.cols as usize {
                                self.cells[row][col] = default_cell();
                            }
                        }
                    }
                    2 => {
                        // 整个屏幕
                        for row in 0..self.rows as usize {
                            for col in 0..self.cols as usize {
                                self.cells[row][col] = default_cell();
                            }
                        }
                        self.cursor_x = 0;
                        self.cursor_y = 0;
                    }
                    _ => {}
                }
            }
            'K' => {
                // EL - 清行
                let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0);
                let row = self.cursor_y as usize;
                match mode {
                    0 => {
                        // 从光标到行末尾
                        for col in self.cursor_x as usize..self.cols as usize {
                            self.cells[row][col] = default_cell();
                        }
                    }
                    2 => {
                        // 整行
                        for col in 0..self.cols as usize {
                            self.cells[row][col] = default_cell();
                        }
                    }
                    _ => {}
                }
            }
            'h' => {
                // DECSET - 设置模式
                for param in params.iter().flat_map(|p| p.iter()) {
                    match param {
                        1 => self.application_cursor_keys = true,
                        _ => {}
                    }
                }
            }
            'l' => {
                // DECRST - 重置模式
                for param in params.iter().flat_map(|p| p.iter()) {
                    match param {
                        1 => self.application_cursor_keys = false,
                        _ => {}
                    }
                }
            }
            'm' => {
                // SGR - 设置样式
                for param in params.iter().flat_map(|p| p.iter()) {
                    match param {
                        0 => {
                            self.fg_color = None;
                            self.bg_color = None;
                            self.bold = false;
                            self.underline = false;
                            self.italic = false;
                            self.reverse = false;
                        }
                        1 => self.bold = true,
                        3 => self.italic = true,
                        4 => self.underline = true,
                        7 => self.reverse = true,
                        30..=37 => {
                            // 标准前景色
                            let colors = [(128, 0, 0), (0, 128, 0), (128, 128, 0), (0, 0, 128), (128, 0, 128), (0, 128, 128), (192, 192, 192), (128, 128, 128)];
                            self.fg_color = Some(colors[(param - 30) as usize]);
                        }
                        38 => {
                            // 256 色/真彩色前景
                            let mut iter = params.iter().flat_map(|p| p.iter());
                            let _ = iter.next(); // 跳过 38
                            if let Some(2) = iter.next() {
                                // RGB
                                let r = iter.next().copied().unwrap_or(0) as u8;
                                let g = iter.next().copied().unwrap_or(0) as u8;
                                let b = iter.next().copied().unwrap_or(0) as u8;
                                self.fg_color = Some((r, g, b));
                            }
                        }
                        39 => self.fg_color = None,
                        40..=47 => {
                            // 标准背景色
                            let colors = [(128, 0, 0), (0, 128, 0), (128, 128, 0), (0, 0, 128), (128, 0, 128), (0, 128, 128), (192, 192, 192), (128, 128, 128)];
                            self.bg_color = Some(colors[(param - 40) as usize]);
                        }
                        48 => {
                            // 256 色/真彩色背景
                            let mut iter = params.iter().flat_map(|p| p.iter());
                            let _ = iter.next(); // 跳过 48
                            if let Some(2) = iter.next() {
                                // RGB
                                let r = iter.next().copied().unwrap_or(0) as u8;
                                let g = iter.next().copied().unwrap_or(0) as u8;
                                let b = iter.next().copied().unwrap_or(0) as u8;
                                self.bg_color = Some((r, g, b));
                            }
                        }
                        49 => self.bg_color = None,
                        90..=97 => {
                            // 亮色前景
                            let colors = [(255, 0, 0), (0, 255, 0), (255, 255, 0), (0, 0, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255), (0, 0, 0)];
                            self.fg_color = Some(colors[(param - 90) as usize]);
                        }
                        100..=107 => {
                            // 亮色背景
                            let colors = [(255, 0, 0), (0, 255, 0), (255, 255, 0), (0, 0, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255), (0, 0, 0)];
                            self.bg_color = Some(colors[(param - 100) as usize]);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        // ESC 序列 - 简化处理
    }
}
