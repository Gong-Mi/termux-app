use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use jni::objects::JValue;
use std::cmp::{max, min};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use vte::{Params, Parser, Perform};

use crate::utils::map_line_drawing;

/// Base64 解码辅助函数
fn base64_decode(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64.decode(input)
}

// =============================================================================
// DirectByteBuffer 零拷贝支持
// =============================================================================

/// 共享内存布局：用于 Rust 和 Java 之间的零拷贝数据共享
/// 内存布局：[text_data (u16 数组)][style_data (u64 数组)]
#[repr(C)]
pub struct SharedScreenBuffer {
    /// 缓冲区版本，用于同步
    pub version: AtomicBool,
    /// 列数
    pub cols: u32,
    /// 行数
    pub rows: u32,
    /// 文本数据起始位置（u16 数组，大小 = cols * rows）
    pub text_data: [u16; 0], // 灵活数组成员
}

impl SharedScreenBuffer {
    /// 计算所需内存大小
    pub fn required_size(cols: usize, rows: usize) -> usize {
        std::mem::size_of::<Self>() + (cols * rows * (2 + 8)) // u16 + u64 per cell
    }

    /// 获取样式数据起始位置
    pub fn style_data_ptr(&self) -> *const u64 {
        let text_ptr = self.text_data.as_ptr();
        let cell_count = self.cols as usize * self.rows as usize;
        unsafe { text_ptr.add(cell_count) as *const u64 }
    }

    /// 获取可变样式数据指针
    pub fn style_data_ptr_mut(&mut self) -> *mut u64 {
        let text_ptr = self.text_data.as_ptr() as *mut u16;
        let cell_count = self.cols as usize * self.rows as usize;
        unsafe { text_ptr.add(cell_count) as *mut u64 }
    }
}

/// 屏幕数据扁平化存储，支持零拷贝
pub struct FlatScreenBuffer {
    /// 共享内存缓冲区（用于 DirectByteBuffer）
    pub shared_buffer: Option<Arc<SharedScreenBuffer>>,
    /// 文本数据（Rust 侧使用）
    pub text_data: Vec<u16>,
    /// 样式数据（Rust 侧使用）
    pub style_data: Vec<u64>,
    /// 列数
    pub cols: usize,
    /// 行数
    pub rows: usize,
}

impl FlatScreenBuffer {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cell_count = cols * rows;
        Self {
            shared_buffer: None,
            text_data: vec![0u16; cell_count],
            style_data: vec![0u64; cell_count],
            cols,
            rows,
        }
    }

    /// 创建共享缓冲区用于 DirectByteBuffer
    pub fn create_shared_buffer(&mut self) -> *mut SharedScreenBuffer {
        // 分配共享内存
        let buffer_size = SharedScreenBuffer::required_size(self.cols, self.rows);
        let layout = std::alloc::Layout::from_size_align(buffer_size, 8).unwrap();
        let ptr = unsafe { std::alloc::alloc(layout) } as *mut SharedScreenBuffer;

        if !ptr.is_null() {
            unsafe {
                std::ptr::write(&mut (*ptr).version, AtomicBool::new(false));
                std::ptr::write(&mut (*ptr).cols, self.cols as u32);
                std::ptr::write(&mut (*ptr).rows, self.rows as u32);
            }
            self.shared_buffer = Some(Arc::new(SharedScreenBuffer {
                version: AtomicBool::new(false),
                cols: self.cols as u32,
                rows: self.rows as u32,
                text_data: [],
            }));
        }

        ptr
    }

    /// 从共享缓冲区同步数据到 Rust 侧
    pub fn sync_from_shared(&mut self, shared_ptr: *const SharedScreenBuffer) {
        if shared_ptr.is_null() {
            return;
        }

        unsafe {
            let shared = &*shared_ptr;
            let cell_count = self.cols * self.rows;

            std::ptr::copy_nonoverlapping(
                shared.text_data.as_ptr(),
                self.text_data.as_mut_ptr(),
                cell_count,
            );
            std::ptr::copy_nonoverlapping(
                shared.style_data_ptr(),
                self.style_data.as_mut_ptr(),
                cell_count,
            );
        }
    }

    /// 同步数据到共享缓冲区
    pub fn sync_to_shared(&self, shared_ptr: *mut SharedScreenBuffer) {
        if shared_ptr.is_null() {
            return;
        }

        unsafe {
            let shared = &mut *shared_ptr;
            let cell_count = self.cols * self.rows;

            std::ptr::copy_nonoverlapping(
                self.text_data.as_ptr(),
                shared.text_data.as_mut_ptr(),
                cell_count,
            );
            std::ptr::copy_nonoverlapping(
                self.style_data.as_ptr(),
                shared.style_data_ptr_mut(),
                cell_count,
            );

            // 更新版本，通知 Java 侧数据已变更
            shared.version.store(true, Ordering::Release);
        }
    }

    /// 获取单元格索引
    #[inline]
    pub fn cell_index(&self, col: usize, row: usize) -> usize {
        row * self.cols + col
    }

    /// 设置单元格文本
    #[inline]
    pub fn set_cell_text(&mut self, col: usize, row: usize, ch: u16) {
        let idx = self.cell_index(col, row);
        if idx < self.text_data.len() {
            self.text_data[idx] = ch;
        }
    }

    /// 设置单元格样式
    #[inline]
    pub fn set_cell_style(&mut self, col: usize, row: usize, style: u64) {
        let idx = self.cell_index(col, row);
        if idx < self.style_data.len() {
            self.style_data[idx] = style;
        }
    }
}

// =============================================================================
// Sixel 图形解码支持 (DCS 序列)
// =============================================================================

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
        // 不重置颜色寄存器，除非收到重置命令
    }

    /// 开始解析 DCS Sixel 序列
    pub fn start(&mut self, params: &Params) {
        self.reset();
        self.state = SixelState::Param;

        // 解析 DCS 参数：Pn1;Pn2;Pn3;Pn4;Pn5
        // Pn1: 图像宽度（可选，像素单位）
        // Pn2: 图像高度（可选，像素单位）
        // Pn3: 透明标志（0 或 1）
        // Pn4: 纵横比参数（格式：Ph;Pv）
        // Pn5: 图形原点模式（0 或 1）
        for param in params.iter() {
            for value in param.iter() {
                self.params.push(*value as i32);
            }
        }

        // 解析参数
        if self.params.len() >= 1 && self.params[0] > 0 {
            self.width = self.params[0] as usize;
        }
        if self.params.len() >= 2 && self.params[1] > 0 {
            self.height = self.params[1] as usize;
        }
        if self.params.len() >= 3 {
            self.transparent = self.params[2] != 0;
        }
        // Pn4 纵横比：Pn4a:Pn4b 格式，需要特殊处理
        if self.params.len() >= 5 {
            self.aspect_ratio = (self.params[3] as u32, self.params[4] as u32);
        }
        if self.params.len() >= 6 {
            self.origin_mode = self.params[5] != 0;
        }

        // 初始化像素数据缓冲区
        // 每个 sixel 行包含 6 个垂直像素
        let sixel_rows = if self.height > 0 {
            (self.height + 5) / 6
        } else {
            100 // 默认高度
        };
        self.pixel_data = vec![vec![0u8; self.width.max(1)]; sixel_rows];
    }

    /// 处理 Sixel 数据字符
    pub fn process_data(&mut self, data: &[u8]) {
        self.state = SixelState::Data;

        // 如果 pixel_data 为空，初始化默认缓冲区
        if self.pixel_data.is_empty() {
            let default_width = self.width.max(100);
            let default_height = 100; // 默认 100 像素高
            let sixel_rows = (default_height + 5) / 6;
            self.pixel_data = vec![vec![0u8; default_width]; sixel_rows];
            // 更新宽度为实际初始化值
            if self.width == 0 {
                self.width = default_width;
            }
        }

        for &byte in data {
            match byte {
                // Sixel 数据字符 (0-63)，每个字符代表 6 个垂直像素
                // ASCII 范围：'0' (48) 到 '?' (63)
                48..=63 => {
                    let sixel_value = (byte - 48) as u8;

                    // 将 sixel 值转换为 6 个像素（垂直方向）
                    for bit in 0..6 {
                        let pixel_row = self.current_row + bit as usize;
                        if pixel_row < self.pixel_data.len() {
                            let mask = 1u8 << bit;
                            if (sixel_value & mask) != 0 {
                                // 设置当前颜色
                                self.pixel_data[pixel_row][self.current_col] =
                                    self.current_color as u8;
                            }
                        }
                    }

                    // 移动到下一列
                    self.current_col += 1;
                    if self.current_col >= self.pixel_data[0].len() {
                        // 自动扩展宽度
                        for row in &mut self.pixel_data {
                            row.push(0);
                        }
                        self.width = self.pixel_data[0].len();
                    }
                }
                b'!' => {
                    // 图形结束，换行到下一行
                    self.current_row += 6;
                    self.current_col = 0;

                    // 扩展高度如果需要
                    while self.current_row + 6 > self.pixel_data.len() {
                        self.pixel_data.push(vec![0u8; self.width.max(1)]);
                    }
                }
                b'#' => {
                    // 颜色选择，后面跟颜色索引和参数
                    // 格式：# Pc ; Pu ; Px ; Py ; Pz
                    // Pc: 颜色索引 (0-255)
                    // Pu: 颜色空间 (0=HLS, 1=RGB)
                    // Px, Py, Pz: 颜色值
                    // 简单处理：读取下一个字符作为颜色索引
                }
                b'$' => {
                    // 光标归位到行首
                    self.current_col = 0;
                }
                b'*' => {
                    // 重复计数开始
                    // 格式：* N C，其中 N 是重复次数，C 是 sixel 字符
                    // 下一个字符是重复次数
                }
                b'~' => {
                    // 删除图形（清除）
                    if self.current_row < self.pixel_data.len()
                        && self.current_col < self.pixel_data[self.current_row].len()
                    {
                        self.pixel_data[self.current_row][self.current_col] = 0;
                    }
                }
                b'\r' => {
                    // 回车
                    self.current_col = 0;
                }
                b'\n' => {
                    // 换行
                    self.current_row += 6;
                    self.current_col = 0;
                }
                0x08 => {
                    // 退格
                    if self.current_col > 0 {
                        self.current_col -= 1;
                    }
                }
                0x0C => {
                    // 换页，清屏
                    for row in &mut self.pixel_data {
                        row.fill(0);
                    }
                    self.current_row = 0;
                    self.current_col = 0;
                }
                b' ' => {
                    // 空格，忽略
                }
                _ => {
                    // 其他字符，忽略
                }
            }
        }

        // 更新实际高度
        self.height = self.pixel_data.len() * 6;
    }

    /// 完成解析
    pub fn finish(&mut self) {
        self.state = SixelState::Ground;
    }

    /// 获取解码后的图像数据（RGBA 格式）
    pub fn get_image_data(&self) -> Vec<u8> {
        let mut rgba_data = Vec::new();

        for row in &self.pixel_data {
            for &pixel in row {
                rgba_data.push(pixel); // R
                rgba_data.push(pixel); // G
                rgba_data.push(pixel); // B
                rgba_data.push(255); // A
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

#[derive(Clone)]
pub struct TerminalRow {
    pub text: Vec<char>,
    pub styles: Vec<u64>,
}

impl TerminalRow {
    fn new(cols: usize) -> Self {
        Self {
            text: vec![' '; cols],
            styles: vec![STYLE_NORMAL; cols],
        }
    }

    fn clear(&mut self, start: usize, end: usize, style: u64) {
        let end = min(end, self.text.len());
        if start < end {
            for i in start..end {
                self.text[i] = ' ';
                self.styles[i] = style;
            }
        }
    }
}

/// SGR 样式位字段定义（与 Java TextStyle 格式兼容）
///
/// Java TextStyle 位布局 (64 位 long):
/// - 位 0-10:   效果标志 (11 位)
/// - 位 16-39:  背景色 (24 位真彩色或 9 位索引)
/// - 位 40-63:  前景色 (24 位真彩色或 9 位索引)
///
/// u64 布局：[63:40] 前景色 [39:16] 背景色 [15:0] 效果标志
pub const STYLE_MASK_EFFECT: u64 = 0x7FF;           // 位 0-10 (11 位效果标志)
pub const STYLE_MASK_BG: u64 = 0xFFFFFF << 16;      // 位 16-39 (24 位背景色)
pub const STYLE_MASK_FG: u64 = 0xFFFFFF << 40;      // 位 40-63 (24 位前景色)

// 真彩色标志位（公开供测试使用）
pub const STYLE_TRUECOLOR_FG: u64 = 1 << 9; // 位 9 - 前景色使用 24 位真彩色
pub const STYLE_TRUECOLOR_BG: u64 = 1 << 10; // 位 10 - 背景色使用 24 位真彩色

// 效果标志（与 Java TextStyle 完全一致）
pub const EFFECT_BOLD: u64 = 1 << 0; // 位 0 - 粗体
pub const EFFECT_ITALIC: u64 = 1 << 1; // 位 1 - 斜体
pub const EFFECT_UNDERLINE: u64 = 1 << 2; // 位 2 - 下划线
pub const EFFECT_BLINK: u64 = 1 << 3; // 位 3 - 闪烁
pub const EFFECT_REVERSE: u64 = 1 << 4; // 位 4 - 反显
pub const EFFECT_INVISIBLE: u64 = 1 << 5; // 位 5 - 隐藏
pub const EFFECT_STRIKETHROUGH: u64 = 1 << 6; // 位 6 - 删除线
pub const EFFECT_PROTECTED: u64 = 1 << 7; // 位 7 - 保护属性
pub const EFFECT_DIM: u64 = 1 << 8; // 位 8 - 淡色/半亮度

// 特殊颜色索引（与 Java TextStyle 一致）
pub const COLOR_INDEX_FOREGROUND: u64 = 256;
pub const COLOR_INDEX_BACKGROUND: u64 = 257;
pub const COLOR_INDEX_CURSOR: u64 = 258;

/// 编码样式（与 Java TextStyle.encode 兼容）
///
/// 参数：
/// - fore_color: 前景色（索引色 0-258 或 24 位真彩色如 0xFFRRGGBB）
/// - back_color: 背景色（索引色 0-258 或 24 位真彩色如 0xFFRRGGBB）
/// - effect: 效果标志（如 EFFECT_BOLD 等）
///
/// 返回：编码后的 64 位样式值
#[inline]
pub const fn encode_style(fore_color: u64, back_color: u64, effect: u64) -> u64 {
    let mut result = effect & 0x7FF; // 效果位 (0-10)

    // 处理前景色 (40-63位)
    if (fore_color & 0xff000000) == 0xff000000 {
        // 24 位真彩色标志 (位 9)
        result |= (1 << 9) | ((fore_color & 0x00ffffff) << 40);
    } else {
        // 索引色（保证 9 位，位 40-48）
        result |= (fore_color & 0x1FF) << 40;
    }

    // 处理背景色 (16-39位)
    if (back_color & 0xff000000) == 0xff000000 {
        // 24 位真彩色标志 (位 10)
        result |= (1 << 10) | ((back_color & 0x00ffffff) << 16);
    } else {
        // 索引色（保证 9 位，位 16-24）
        result |= (back_color & 0x1FF) << 16;
    }

    result
}

/// 默认样式（与 Java TextStyle.NORMAL 一致）
/// 默认样式 (对齐 Java TextStyle.NORMAL): 前景 256, 背景 257, 无效果
pub const STYLE_NORMAL: u64 = encode_style(COLOR_INDEX_FOREGROUND, COLOR_INDEX_BACKGROUND, 0);

/// DECSET 标志位定义（与 Java DECSET_BIT_* 常量一致）
pub const DECSET_BIT_APPLICATION_CURSOR_KEYS: i32 = 1;
pub const DECSET_BIT_REVERSE_VIDEO: i32 = 1 << 1;
pub const DECSET_BIT_ORIGIN_MODE: i32 = 1 << 2;
pub const DECSET_BIT_AUTOWRAP: i32 = 1 << 3;
pub const DECSET_BIT_CURSOR_ENABLED: i32 = 1 << 4;
pub const DECSET_BIT_APPLICATION_KEYPAD: i32 = 1 << 5;
pub const DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE: i32 = 1 << 6;
pub const DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT: i32 = 1 << 7;
pub const DECSET_BIT_SEND_FOCUS_EVENTS: i32 = 1 << 8;
pub const DECSET_BIT_MOUSE_PROTOCOL_SGR: i32 = 1 << 9;
pub const DECSET_BIT_BRACKETED_PASTE_MODE: i32 = 1 << 10;
pub const DECSET_BIT_LEFTRIGHT_MARGIN_MODE: i32 = 1 << 11;

// ============================================================================
// TerminalColors - 259 色颜色管理（与 Java TerminalColors 兼容）
// ============================================================================

/// 默认颜色方案（与 Java TerminalColorScheme.DEFAULT_COLORSCHEME 一致）
pub const DEFAULT_COLORSCHEME: [u32; 259] = [
    // 16 原始颜色（前 8 个是暗色）
    0xff000000, // 0: black
    0xffcd0000, // 1: dim red
    0xff00cd00, // 2: dim green
    0xffcdcd00, // 3: dim yellow
    0xff6495ed, // 4: dim blue
    0xffcd00cd, // 5: dim magenta
    0xff00cdcd, // 6: dim cyan
    0xffe5e5e5, // 7: dim white
    // 后 8 个是亮色
    0xff7f7f7f, // 8: medium grey
    0xffff0000, // 9: bright red
    0xff00ff00, // 10: bright green
    0xffffff00, // 11: bright yellow
    0xff5c5cff, // 12: light blue
    0xffff00ff, // 13: bright magenta
    0xff00ffff, // 14: bright cyan
    0xffffffff, // 15: bright white
    // 216 色立方体（6 色阶每色）- 压缩表示，实际使用时展开
    // 为节省空间，这里用循环初始化
    0xff000000, 0xff00005f, 0xff000087, 0xff0000af, 0xff0000d7, 0xff0000ff, 0xff005f00, 0xff005f5f,
    0xff005f87, 0xff005faf, 0xff005fd7, 0xff005fff, 0xff008700, 0xff00875f, 0xff008787, 0xff0087af,
    0xff0087d7, 0xff0087ff, 0xff00af00, 0xff00af5f, 0xff00af87, 0xff00afaf, 0xff00afd7, 0xff00afff,
    0xff00d700, 0xff00d75f, 0xff00d787, 0xff00d7af, 0xff00d7d7, 0xff00d7ff, 0xff00ff00, 0xff00ff5f,
    0xff00ff87, 0xff00ffaf, 0xff00ffd7, 0xff00ffff, 0xff5f0000, 0xff5f005f, 0xff5f0087, 0xff5f00af,
    0xff5f00d7, 0xff5f00ff, 0xff5f5f00, 0xff5f5f5f, 0xff5f5f87, 0xff5f5faf, 0xff5f5fd7, 0xff5f5fff,
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

/// 终端颜色管理（与 Java TerminalColors 兼容）
pub struct TerminalColors {
    /// 当前 259 色数组
    pub current_colors: [u32; 259],
}

impl TerminalColors {
    /// 创建新实例，使用默认颜色
    pub fn new() -> Self {
        Self {
            current_colors: DEFAULT_COLORSCHEME,
        }
    }

    /// 重置所有颜色为默认值
    pub fn reset(&mut self) {
        self.current_colors = DEFAULT_COLORSCHEME;
    }

    /// 重置特定索引颜色
    pub fn reset_index(&mut self, index: usize) {
        if index < 259 {
            self.current_colors[index] = DEFAULT_COLORSCHEME[index];
        }
    }

    /// 解析颜色字符串（与 Java TerminalColors.parse 兼容）
    /// 支持格式：#RGB, #RRGGBB, rgb:R/G/B
    pub fn parse_color(color_str: &str) -> Option<u32> {
        let color_str = color_str.trim();

        if color_str.starts_with('#') {
            // #RGB, #RRGGBB, #RRRGGGBBB, #RRRRGGGGBBBB
            let hex = &color_str[1..];
            let len = hex.len();

            match len {
                3 => {
                    // #RGB -> #RRGGBB
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
                    // #RRRGGGBBB - 12 位色深，缩放到 8 位
                    let r = u16::from_str_radix(&hex[0..3], 16).ok()?;
                    let g = u16::from_str_radix(&hex[3..6], 16).ok()?;
                    let b = u16::from_str_radix(&hex[6..9], 16).ok()?;
                    let r = ((r * 255) / 4095) as u8;
                    let g = ((g * 255) / 4095) as u8;
                    let b = ((b * 255) / 4095) as u8;
                    Some(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                }
                12 => {
                    // #RRRRGGGGBBBB - 16 位色深，缩放到 8 位
                    let r = u16::from_str_radix(&hex[0..4], 16).ok()?;
                    let g = u16::from_str_radix(&hex[4..8], 16).ok()?;
                    let b = u16::from_str_radix(&hex[8..12], 16).ok()?;
                    let r = ((r as u32 * 255) / 65535) as u8;
                    let g = ((g as u32 * 255) / 65535) as u8;
                    let b = ((b as u32 * 255) / 65535) as u8;
                    Some(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                }
                _ => None,
            }
        } else if color_str.starts_with("rgb:") {
            // rgb:R/G/B 格式
            let rgb_part = &color_str[4..];
            let parts: Vec<&str> = rgb_part.split('/').collect();
            if parts.len() != 3 {
                return None;
            }

            let r = u16::from_str_radix(parts[0], 16).ok()?;
            let g = u16::from_str_radix(parts[1], 16).ok()?;
            let b = u16::from_str_radix(parts[2], 16).ok()?;

            // 根据位数缩放到 8 位
            let scale = match parts[0].len() {
                1 => 17, // 4 位 -> 8 位 (x17 = x * 255/15)
                2 => 1,  // 8 位
                3 => 0,  // 12 位，需要除法
                4 => 0,  // 16 位，需要除法
                _ => return None,
            };

            let r8 = if parts[0].len() == 3 {
                ((r as u32 * 255) / 4095) as u8
            } else if parts[0].len() == 4 {
                ((r as u32 * 255) / 65535) as u8
            } else {
                (r as u8).wrapping_mul(scale)
            };
            let g8 = if parts[1].len() == 3 {
                ((g as u32 * 255) / 4095) as u8
            } else if parts[1].len() == 4 {
                ((g as u32 * 255) / 65535) as u8
            } else {
                (g as u8).wrapping_mul(scale)
            };
            let b8 = if parts[2].len() == 3 {
                ((b as u32 * 255) / 4095) as u8
            } else if parts[2].len() == 4 {
                ((b as u32 * 255) / 65535) as u8
            } else {
                (b as u8).wrapping_mul(scale)
            };

            Some(0xff000000 | ((r8 as u32) << 16) | ((g8 as u32) << 8) | (b8 as u32))
        } else {
            None
        }
    }

    /// 尝试解析并设置颜色（OSC 4 命令）
    pub fn try_parse_color(&mut self, index: usize, color_str: &str) -> bool {
        if let Some(color) = Self::parse_color(color_str) {
            if index < 259 {
                self.current_colors[index] = color;
                return true;
            }
        }
        false
    }

    /// 生成 OSC 颜色报告（用于查询当前颜色）
    pub fn generate_color_report(&self, index: usize) -> String {
        if index >= 259 {
            return String::new();
        }

        let color = self.current_colors[index];
        let r = ((color >> 16) & 0xff) as u16;
        let g = ((color >> 8) & 0xff) as u16;
        let b = (color & 0xff) as u16;

        // 缩放到 16 位值（xterm 格式）
        let r16 = (r * 65535) / 255;
        let g16 = (g * 65535) / 255;
        let b16 = (b * 65535) / 255;

        format!("rgb:{:04x}/{:04x}/{:04x}", r16, g16, b16)
    }
}

impl Default for TerminalColors {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ScreenState {
    pub rows: i32,
    pub cols: i32,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub top_margin: i32,
    pub bottom_margin: i32,
    pub left_margin: i32,
    pub right_margin: i32,
    pub current_style: u64,
    pub saved_x: i32,
    pub saved_y: i32,
    pub saved_style: u64,
    pub saved_about_to_wrap: bool,
    // 保存的光标 DECSET 标志（AUTOWRAP, ORIGIN_MODE）
    pub saved_decset_flags: i32,
    // 保存的行绘图状态（DECSC/DECRC）
    pub saved_use_line_drawing_g0: bool,
    pub saved_use_line_drawing_g1: bool,
    pub saved_use_line_drawing_uses_g0: bool,
    // 保存的颜色属性（DECSC/DECRC）
    pub saved_fore_color: u64,
    pub saved_back_color: u64,
    pub origin_mode: bool,
    pub insert_mode: bool,
    pub application_cursor_keys: bool,
    pub reverse_video: bool,
    pub auto_wrap: bool,
    pub cursor_enabled: bool,
    pub application_keypad: bool,
    pub mouse_tracking: bool,
    pub mouse_button_event: bool,
    pub bracketed_paste: bool,
    pub sgr_mouse: bool,
    pub about_to_wrap: bool,
    pub leftright_margin_mode: bool, // DECSET 69 - DECLRMM 左右边距模式
    pub send_focus_events: bool,     // DECSET 1004 - 发送焦点事件

    // DECSET 标志位（用于保存/恢复）
    pub decset_flags: i32,

    // 制表位
    pub tab_stops: Vec<bool>,

    // 主屏幕缓冲区（包含滚动历史）
    pub buffer: Vec<TerminalRow>,
    pub screen_first_row: usize, // 逻辑第 0 行在物理 buffer 中的索引

    // ========================================================================
    // 新增功能字段
    // ========================================================================

    // 颜色管理 (TerminalColors)
    pub colors: TerminalColors,

    // 标题栈 (OSC 22/23)
    pub title: Option<String>,
    pub title_stack: Vec<String>,

    // 行绘图字符集 (G0/G1)
    pub use_line_drawing_g0: bool,
    pub use_line_drawing_g1: bool,
    pub use_line_drawing_uses_g0: bool, // 当前使用 G0 还是 G1

    // 滚动计数器
    pub scroll_counter: i32,

    // 自动滚动禁用
    pub auto_scroll_disabled: bool,

    // 光标闪烁和样式
    pub cursor_blinking_enabled: bool,
    pub cursor_blink_state: bool,
    pub cursor_style: i32, // 0=block, 1=underline, 2=bar

    // 下划线颜色 (SGR 58/59)
    pub underline_color: u64,

    // 前景色/背景色（索引色或真彩色）
    pub fore_color: u64,
    pub back_color: u64,

    // 效果标志（单独存储用于 SGR 重置）
    pub effect: u64,

    // Java 回调支持
    pub java_callback_obj: Option<jni::objects::GlobalRef>,

    // 窗口大小信息 (用于 OSC 18/19 报告)
    pub cell_width_pixels: i32,
    pub cell_height_pixels: i32,

    // ========================================================================
    // 备用屏幕缓冲区支持 (DECSET 1048/1049)
    // ========================================================================

    // 备用屏幕缓冲区（只保存可见屏幕，不需要滚动历史）
    pub alt_buffer: Vec<TerminalRow>,
    // 当前使用的缓冲区 (true = 备用缓冲区)
    pub use_alternate_buffer: bool,

    // 保存的主屏幕状态 (用于 DECSET 1049)
    pub saved_main_cursor_x: i32,
    pub saved_main_cursor_y: i32,
    pub saved_main_decset_flags: i32,
    pub saved_main_screen_first_row: usize,

    // ========================================================================
    // DirectByteBuffer 零拷贝支持 (新增)
    // ========================================================================

    // 扁平化屏幕缓冲区（用于 DirectByteBuffer 共享）
    pub flat_buffer: Option<FlatScreenBuffer>,
    // 共享内存指针（用于 JNI DirectByteBuffer）
    pub shared_buffer_ptr: *mut SharedScreenBuffer,

    // ========================================================================
    // Sixel 图形支持 (新增)
    // ========================================================================

    // Sixel 解码器
    pub sixel_decoder: SixelDecoder,
}

impl ScreenState {
    pub fn new(cols: i32, rows: i32, total_rows: i32, cell_width: i32, cell_height: i32) -> Self {
        let total_rows_u = max(rows as usize, total_rows as usize);

        // 初始化主屏幕缓冲区（包含滚动历史）
        let mut buffer = Vec::with_capacity(total_rows_u);
        for _ in 0..total_rows_u {
            buffer.push(TerminalRow::new(max(1, cols as usize)));
        }

        // 初始化备用屏幕缓冲区 (大小与屏幕相同，不需要滚动历史)
        let mut alt_buffer = Vec::with_capacity(rows as usize);
        for _ in 0..rows as usize {
            alt_buffer.push(TerminalRow::new(max(1, cols as usize)));
        }

        // 初始化制表位（每 8 列一个，从位置 8 开始：8, 16, 24, ...）
        let mut tab_stops = vec![false; cols as usize];
        for i in (8..cols as usize).step_by(8) {
            if i < tab_stops.len() {
                tab_stops[i] = true;
            }
        }

        Self {
            rows,
            cols,
            cursor_x: 0,
            cursor_y: 0,
            top_margin: 0,
            bottom_margin: rows,
            left_margin: 0,
            right_margin: cols,
            current_style: STYLE_NORMAL,
            origin_mode: false,
            insert_mode: false,
            application_cursor_keys: false,
            reverse_video: false,
            auto_wrap: true,
            cursor_enabled: true,
            application_keypad: false,
            mouse_tracking: false,
            mouse_button_event: false,
            bracketed_paste: false,
            sgr_mouse: false,
            about_to_wrap: false,
            leftright_margin_mode: false, // DECSET 69 - 默认禁用左右边距模式
            send_focus_events: false,     // DECSET 1004 - 默认不发送焦点事件
            decset_flags: 0,              // 初始 DECSET 标志为 0
            tab_stops,
            buffer,
            screen_first_row: 0,

            // 保存状态字段初始化
            saved_x: 0,
            saved_y: 0,
            saved_style: STYLE_NORMAL,
            saved_about_to_wrap: false,
            saved_decset_flags: 0,
            saved_use_line_drawing_g0: false,
            saved_use_line_drawing_g1: false,
            saved_use_line_drawing_uses_g0: true,
            saved_fore_color: COLOR_INDEX_FOREGROUND,
            saved_back_color: COLOR_INDEX_BACKGROUND,

            // 新增功能字段初始化
            colors: TerminalColors::new(),
            title: None,
            title_stack: Vec::new(),
            use_line_drawing_g0: false,
            use_line_drawing_g1: false,
            use_line_drawing_uses_g0: true,
            scroll_counter: 0,
            auto_scroll_disabled: false,
            cursor_blinking_enabled: false,
            cursor_blink_state: true,
            cursor_style: 0, // block cursor
            underline_color: COLOR_INDEX_FOREGROUND,
            fore_color: COLOR_INDEX_FOREGROUND,
            back_color: COLOR_INDEX_BACKGROUND,
            effect: 0,

            java_callback_obj: None,

            // 窗口大小信息初始化
            cell_width_pixels: cell_width,
            cell_height_pixels: cell_height,

            // 备用屏幕缓冲区初始化
            alt_buffer,
            use_alternate_buffer: false,
            saved_main_cursor_x: 0,
            saved_main_cursor_y: 0,
            saved_main_decset_flags: 0,
            saved_main_screen_first_row: 0,

            // DirectByteBuffer 零拷贝支持初始化
            // 使用 total_rows_u 而不是 rows，确保共享内存缓冲区包含所有滚动历史行
            flat_buffer: Some(FlatScreenBuffer::new(cols as usize, total_rows_u)),
            shared_buffer_ptr: std::ptr::null_mut(),

            // Sixel 图形支持初始化
            sixel_decoder: SixelDecoder::new(),
        }
    }

    /// 获取当前活动的缓冲区（主或备）
    #[inline]
    fn get_current_buffer(&self) -> &Vec<TerminalRow> {
        if self.use_alternate_buffer {
            &self.alt_buffer
        } else {
            &self.buffer
        }
    }

    /// 获取当前活动的缓冲区（可变引用）
    #[inline]
    fn get_current_buffer_mut(&mut self) -> &mut Vec<TerminalRow> {
        if self.use_alternate_buffer {
            &mut self.alt_buffer
        } else {
            &mut self.buffer
        }
    }

    /// 将逻辑行号转换为物理数组索引
    #[inline]
    fn external_to_internal_row(&self, row: i32) -> usize {
        let buffer = self.get_current_buffer();
        let total = buffer.len();

        if self.use_alternate_buffer {
            // 备用缓冲区没有滚动历史，直接映射
            (row.max(0) as usize).min(total - 1)
        } else {
            // 主缓冲区使用循环缓冲区映射
            // 处理负数行（滚动历史）：(first_row + row) % total
            // 使用 i64 避免溢出，并实现正确的负数取模
            let first = self.screen_first_row as i64;
            let r = row as i64;
            let t = total as i64;
            let internal = (first + r) % t;
            if internal < 0 {
                (internal + t) as usize
            } else {
                internal as usize
            }
        }
    }

    /// 获取当前缓冲区的总行数
    #[inline]
    fn buffer_len(&self) -> usize {
        self.get_current_buffer().len()
    }

    /// 设置 Java 回调环境
    pub fn set_java_callback(&mut self, obj: jni::objects::GlobalRef) {
        self.java_callback_obj = Some(obj);
    }

    /// 调用 Java 方法报告标题变更
    fn report_title_change(&self, title: &str) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    if let Ok(java_title) = env.new_string(title) {
                        let _ = env.call_method(
                            obj.as_obj(),
                            "reportTitleChange",
                            "(Ljava/lang/String;)V",
                            &[JValue::Object(&java_title)],
                        );
                    }
                }
            }
        }
    }

    /// 调用 Java 方法报告颜色变更
    fn report_colors_changed(&self) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let _ = env.call_method(obj.as_obj(), "reportColorsChanged", "()V", &[]);
                }
            }
        }
    }

    /// 调用 Java 方法报告响铃事件
    fn report_bell(&self) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let _ = env.call_method(obj.as_obj(), "onBell", "()V", &[]);
                }
            }
        }
    }

    /// 调用 Java 方法报告光标可见性变更
    fn report_cursor_visibility(&self, visible: bool) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let _ = env.call_method(
                        obj.as_obj(),
                        "reportCursorVisibility",
                        "(Z)V",
                        &[JValue::Bool(if visible { 1 } else { 0 })],
                    );
                }
            }
        }
    }

    // ========================================================================
    // 备用屏幕缓冲区辅助方法
    // ========================================================================

    /// 清除备用缓冲区
    fn clear_alt_buffer(&mut self) {
        let cols = self.cols as usize;
        let alt_len = self.alt_buffer.len();
        for i in 0..alt_len {
            let row_cols = self.alt_buffer[i].text.len();
            self.alt_buffer[i].clear(0, row_cols.min(cols), STYLE_NORMAL);
        }
    }

    /// 检查是否使用备用缓冲区
    #[inline]
    pub fn is_alternate_buffer_active(&self) -> bool {
        self.use_alternate_buffer
    }

    /// 调用 Java 方法报告屏幕刷新
    pub fn report_screen_update(&self) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    let _ = env.call_method(obj.as_obj(), "onScreenUpdate", "()V", &[]);
                }
            }
        }
    }

    /// 调用 Java 方法复制文本到剪贴板
    fn report_clipboard_copy(&self, text: &str) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    if let Ok(java_text) = env.new_string(text) {
                        let _ = env.call_method(
                            obj.as_obj(),
                            "onCopyTextToClipboard",
                            "(Ljava/lang/String;)V",
                            &[JValue::Object(&java_text)],
                        );
                    }
                }
            }
        }
    }

    /// 调用 Java 方法写入数据到终端
    pub fn write_to_session(&self, data: &str) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    if let Ok(java_data) = env.new_string(data) {
                        let _ = env.call_method(
                            obj.as_obj(),
                            "onWriteToSession",
                            "(Ljava/lang/String;)V",
                            &[JValue::Object(&java_data)],
                        );
                    }
                }
            }
        }
    }

    /// 调用 Java 方法写入字节数据到终端（避免 String 转换）
    pub fn write_to_session_bytes(&self, data: &[u8]) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    // 将字节数组转换为 Java byte[]
                    // 注意：JNI 的 byte 是有符号的 (i8)，但我们可以直接传递 u8 数据
                    if let Ok(java_bytes) = env.new_byte_array(data.len() as i32) {
                        // 安全转换：u8 和 i8 在内存中布局相同，只是解释不同
                        let signed_data: &[i8] = unsafe {
                            std::slice::from_raw_parts(data.as_ptr() as *const i8, data.len())
                        };
                        let _ = env.set_byte_array_region(&java_bytes, 0, signed_data);
                        let _ = env.call_method(
                            obj.as_obj(),
                            "onWriteToSessionBytes",
                            "([B)V",
                            &[JValue::Object(&java_bytes)],
                        );
                    }
                }
            }
        }
    }

    /// 调用 Java 方法报告颜色查询响应
    fn report_color_response(&self, color_spec: &str) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    if let Ok(java_spec) = env.new_string(color_spec) {
                        let _ = env.call_method(
                            obj.as_obj(),
                            "reportColorResponse",
                            "(Ljava/lang/String;)V",
                            &[JValue::Object(&java_spec)],
                        );
                    }
                }
            }
        }
    }

    /// 调用 Java 方法报告终端响应 (DSR/DEC)
    fn report_terminal_response(&self, response: &str) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    if let Ok(java_response) = env.new_string(response) {
                        let _ = env.call_method(
                            obj.as_obj(),
                            "reportTerminalResponse",
                            "(Ljava/lang/String;)V",
                            &[JValue::Object(&java_response)],
                        );
                    }
                }
            }
        }
    }

    /// 报告焦点获得事件
    pub fn report_focus_gain(&self) {
        if self.send_focus_events {
            self.write_to_session("\x1b[I");
        }
    }

    /// 报告焦点失去事件
    pub fn report_focus_loss(&self) {
        if self.send_focus_events {
            self.write_to_session("\x1b[O");
        }
    }

    // ========================================================================
    // Sixel 图形渲染
    // ========================================================================

    /// 渲染 Sixel 图像到屏幕
    pub fn render_sixel_image(&mut self) {
        let decoder = &self.sixel_decoder;

        // 获取图像数据
        let image_data = decoder.get_image_data();
        let width = decoder.width;
        let height = decoder.height;

        if width == 0 || height == 0 {
            return;
        }

        // 计算每个像素在终端中的位置
        // Sixel 图像每个 sixel 单位是 6 像素高，1 像素宽
        let start_x = self.cursor_x;
        let start_y = self.cursor_y;

        // 通过 Java 回调报告图像数据
        self.report_sixel_image(&image_data, width, height, start_x, start_y);

        // 移动光标到图像下方
        let pixels_per_row = 6; // 每个 sixel 行有 6 像素
        let terminal_rows_needed = (height + pixels_per_row - 1) / pixels_per_row;
        self.cursor_y = min(self.cursor_y + terminal_rows_needed as i32, self.rows - 1);
    }

    /// 调用 Java 方法报告 Sixel 图像
    fn report_sixel_image(&self, image_data: &[u8], width: usize, height: usize, x: i32, y: i32) {
        if let Some(obj) = self.java_callback_obj.as_ref() {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.get_env() {
                    // 创建 byte 数组传递图像数据
                    if let Ok(java_image_data) = env.new_byte_array(image_data.len() as i32) {
                        let data_vec: Vec<i8> = image_data.iter().map(|&b| b as i8).collect();

                        // 保存原始引用用于后续调用
                        let image_obj_raw = java_image_data.as_raw();

                        let _ = env.set_byte_array_region(java_image_data, 0, data_vec.as_slice());

                        // 调用 Java 方法 - 使用原始引用重建 JObject
                        let image_obj = unsafe { jni::objects::JObject::from_raw(image_obj_raw) };
                        let _ = env.call_method(
                            obj.as_obj(),
                            "onSixelImage",
                            "([BIIII)V",
                            &[
                                JValue::Object(&image_obj),
                                JValue::Int(width as i32),
                                JValue::Int(height as i32),
                                JValue::Int(x),
                                JValue::Int(y),
                            ],
                        );
                    }
                }
            }
        }
    }

    /// 处理括号粘贴模式 - 开始粘贴
    pub fn paste_start(&mut self, text: &str) {
        if self.bracketed_paste {
            // 发送粘贴开始标记
            self.write_to_session("\x1b[200~");
            // 发送粘贴内容
            self.write_to_session(text);
            // 发送粘贴结束标记
            self.write_to_session("\x1b[201~");
        } else {
            // 非括号粘贴模式，直接发送内容
            self.write_to_session(text);
        }
    }

    // ========================================================================
    // 鼠标和键盘事件处理方法
    // ========================================================================

    /// 发送鼠标事件
    /// 支持 SGR 模式和旧格式模式
    ///
    /// 按钮值定义 (与 Java TerminalEmulator 保持一致):
    /// - 0: MOUSE_LEFT_BUTTON (左键按下)
    /// - 1: MOUSE_MIDDLE_BUTTON (中键按下)
    /// - 2: MOUSE_RIGHT_BUTTON (右键按下)
    /// - 32: MOUSE_LEFT_BUTTON_MOVED (左键移动)
    /// - 33: MOUSE_MIDDLE_BUTTON_MOVED (中键移动)
    /// - 34: MOUSE_RIGHT_BUTTON_MOVED (右键移动)
    /// - 64: MOUSE_WHEELUP_BUTTON (滚轮向上)
    /// - 65: MOUSE_WHEELDOWN_BUTTON (滚轮向下)
    pub fn send_mouse_event(&mut self, mouse_button: u32, column: i32, row: i32, pressed: bool) {
        // 使用 SmallVec 避免小字符串分配
        let mut response = [0u8; 32];
        let len;

        if self.sgr_mouse {
            // SGR 鼠标格式：CSI < button ; x ; y M/m
            // button: 0-2 = 左/中/右按下，3 = 释放，64/65 = 滚轮
            // M = 按下/移动，m = 释放
            let event_type = if pressed { b'M' } else { b'm' };
            // 格式：\x1b[<button;x;yM 或 \x1b[<button;x;ym
            let response_str = format!(
                "\x1b[<{};{};{}{}",
                mouse_button, column, row, event_type as char
            );
            self.write_to_session(&response_str);
            return;
        } else if self.mouse_tracking || self.mouse_button_event {
            // 旧格式鼠标事件
            // 格式：CSI M Cb Cx Cy
            // Cb = 32 + button + modifiers
            // Cx = 32 + column (1-based)
            // Cy = 32 + row (1-based)

            // 检查是否超出旧格式范围 (最大 223 = 255 - 32)
            if column > 223 || row > 223 {
                return;
            }

            // 构建按钮编码
            // 按钮值：0=左，1=中，2=右，3=释放
            // 移动事件：32=左移动，33=中移动，34=右移动
            let mut button_val = mouse_button;

            // 判断是否为移动事件 (32-34 = 移动事件)
            let is_move = mouse_button >= 32 && mouse_button <= 34;
            if is_move && !self.mouse_button_event {
                // 非按钮事件模式下不发送移动事件
                return;
            }

            // 处理移动事件
            if is_move {
                // 移动事件需要减去 32 得到基础按钮值，然后加 32 偏移
                button_val = mouse_button - 32;
            }

            // 释放事件 (非移动事件且 pressed=false)
            if !pressed && !is_move {
                button_val = 3;
            }

            // 添加移动偏移 (32)
            // 在按钮事件模式下，移动事件需要加 32
            if is_move && self.mouse_button_event {
                button_val += 32;
            }

            // 构建响应：CSI M Cb Cx Cy (固定 6 字节)
            // \x1b [ M Cb Cx Cy
            let cb = 32 + button_val as u8;
            let cx = 32 + column as u8;
            let cy = 32 + row as u8;

            // 直接使用字节数组，避免 String 分配
            response[0] = b'\x1b';
            response[1] = b'[';
            response[2] = b'M';
            response[3] = cb;
            response[4] = cx;
            response[5] = cy;
            len = 6;

            self.write_to_session_bytes(&response[..len]);
            return;
        }
        // 如果鼠标跟踪未启用，忽略事件
    }

    /// 发送键盘事件
    /// 处理特殊键和功能键的转义序列
    ///
    /// 参数：
    /// - key_code: Android KeyEvent 键码
    /// - key_char: 字符输入（普通字符键）
    /// - key_mod: 修饰键状态（shift=2, alt=3, ctrl=5, 组合按位或）
    pub fn send_key_event(&mut self, key_code: i32, key_char: Option<String>, key_mod: i32) {
        // 检查修饰键
        let shift = (key_mod & 0x20000000) != 0;
        let ctrl = (key_mod & 0x40000000) != 0;
        let alt = (key_mod & 0x80000000u32 as i32) != 0;

        // 构建修饰键前缀
        let mod_prefix = if alt { "\x1b" } else { "" };

        // 特殊键码映射 (与 Java KeyHandler 兼容)
        let escape_seq: String = match key_code {
            // 功能键 F1-F12 (支持修饰键)
            131 => Self::build_fkey_seq(1, key_mod),  // F1
            132 => Self::build_fkey_seq(2, key_mod),  // F2
            133 => Self::build_fkey_seq(3, key_mod),  // F3
            134 => Self::build_fkey_seq(4, key_mod),  // F4
            135 => Self::build_fkey_seq(5, key_mod),  // F5
            136 => Self::build_fkey_seq(6, key_mod),  // F6
            137 => Self::build_fkey_seq(7, key_mod),  // F7
            138 => Self::build_fkey_seq(8, key_mod),  // F8
            139 => Self::build_fkey_seq(9, key_mod),  // F9
            140 => Self::build_fkey_seq(10, key_mod), // F10
            141 => Self::build_fkey_seq(11, key_mod), // F11
            142 => Self::build_fkey_seq(12, key_mod), // F12

            // 方向键 (支持应用光标键模式和修饰键)
            19 => {
                // 上
                if self.application_cursor_keys {
                    if key_mod == 0 {
                        "\x1bOA".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'A')
                    }
                } else {
                    if key_mod == 0 {
                        "\x1b[A".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'A')
                    }
                }
            }
            20 => {
                // 下
                if self.application_cursor_keys {
                    if key_mod == 0 {
                        "\x1bOB".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'B')
                    }
                } else {
                    if key_mod == 0 {
                        "\x1b[B".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'B')
                    }
                }
            }
            21 => {
                // 左
                if self.application_cursor_keys {
                    if key_mod == 0 {
                        "\x1bOD".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'D')
                    }
                } else {
                    if key_mod == 0 {
                        "\x1b[D".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'D')
                    }
                }
            }
            22 => {
                // 右
                if self.application_cursor_keys {
                    if key_mod == 0 {
                        "\x1bOC".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'C')
                    }
                } else {
                    if key_mod == 0 {
                        "\x1b[C".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'C')
                    }
                }
            }

            // Home/End
            91 => {
                // Home
                if self.application_cursor_keys {
                    if key_mod == 0 {
                        "\x1bOH".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'H')
                    }
                } else {
                    if key_mod == 0 {
                        "\x1b[H".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'H')
                    }
                }
            }
            92 => {
                // End
                if self.application_cursor_keys {
                    if key_mod == 0 {
                        "\x1bOF".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'F')
                    }
                } else {
                    if key_mod == 0 {
                        "\x1b[F".to_string()
                    } else {
                        Self::build_mod_seq("\x1b[1", key_mod, 'F')
                    }
                }
            }

            // 编辑键
            111 => {
                // Backspace
                if alt {
                    if ctrl {
                        "\x1b\x08".to_string()
                    } else {
                        "\x1b\x7f".to_string()
                    }
                } else {
                    if ctrl {
                        "\x08".to_string()
                    } else {
                        "\x7f".to_string()
                    }
                }
            }
            112 => Self::build_mod_seq("\x1b[3", key_mod, '~'), // Delete
            88 => Self::build_mod_seq("\x1b[5", key_mod, '~'),  // Page Up
            89 => Self::build_mod_seq("\x1b[6", key_mod, '~'),  // Page Down
            93 => {
                if shift {
                    "\x1b[Z".to_string()
                } else {
                    "\x09".to_string()
                }
            } // Tab (Shift+Tab = 反向制表)
            113 => Self::build_mod_seq("\x1b[2", key_mod, '~'), // Insert

            // 数字小键盘 (支持应用键盘模式)
            144 => {
                // Num Lock
                if self.application_keypad {
                    "\x1bOP".to_string()
                } else {
                    "0".to_string()
                }
            }
            145 => {
                if self.application_keypad {
                    "\x1bOj".to_string()
                } else {
                    "/".to_string()
                }
            } // KP Divide
            146 => {
                if self.application_keypad {
                    "\x1bOk".to_string()
                } else {
                    "*".to_string()
                }
            } // KP Multiply
            147 => {
                if self.application_keypad {
                    "\x1bOm".to_string()
                } else {
                    "-".to_string()
                }
            } // KP Subtract
            148 => {
                if self.application_keypad {
                    "\x1bOk".to_string()
                } else {
                    "+".to_string()
                }
            } // KP Add
            149 => {
                if self.application_keypad {
                    "\x1bOM".to_string()
                } else {
                    "\r".to_string()
                }
            } // KP Enter
            150 => {
                if self.application_keypad {
                    "\x1bOX".to_string()
                } else {
                    "=".to_string()
                }
            } // KP Equals

            // Escape
            114 => {
                if alt {
                    "\x1b\x1b".to_string()
                } else {
                    "\x1b".to_string()
                }
            } // Escape

            // Space (Ctrl+Space = NUL)
            62 => {
                if ctrl {
                    "\x00".to_string()
                } else {
                    " ".to_string()
                }
            }

            // Enter
            66 => {
                if alt {
                    "\x1b\r".to_string()
                } else {
                    "\r".to_string()
                }
            }

            // 未映射的键，使用 key_char
            _ => {
                if let Some(ref ch) = key_char {
                    // 处理 Ctrl 组合
                    if ctrl && ch.len() == 1 {
                        let c = ch.chars().next().unwrap();
                        if c >= '@' && c <= '_' {
                            // Ctrl+A..Ctrl+Z
                            let ctrl_char = (c as u8 - b'@') as char;
                            self.write_to_session(&format!("{}{}", mod_prefix, ctrl_char));
                            return;
                        }
                    }
                    self.write_to_session(&format!("{}{}", mod_prefix, ch));
                }
                return;
            }
        };

        self.write_to_session(&format!("{}{}", mod_prefix, escape_seq));
    }

    /// 构建功能键转义序列
    /// F1-F4: 无修饰=\x1bOP, 有修饰=\x1b[1;N~
    /// F5-F12: \x1b[NN;N~
    fn build_fkey_seq(fnum: i32, key_mod: i32) -> String {
        if key_mod == 0 {
            match fnum {
                1 => "\x1bOP".to_string(),
                2 => "\x1bOQ".to_string(),
                3 => "\x1bOR".to_string(),
                4 => "\x1bOS".to_string(),
                _ => String::new(),
            }
        } else {
            match fnum {
                1 => Self::build_mod_seq("\x1b[11", key_mod, '~'),
                2 => Self::build_mod_seq("\x1b[12", key_mod, '~'),
                3 => Self::build_mod_seq("\x1b[13", key_mod, '~'),
                4 => Self::build_mod_seq("\x1b[14", key_mod, '~'),
                5 => Self::build_mod_seq("\x1b[15", key_mod, '~'),
                6 => Self::build_mod_seq("\x1b[17", key_mod, '~'),
                7 => Self::build_mod_seq("\x1b[18", key_mod, '~'),
                8 => Self::build_mod_seq("\x1b[19", key_mod, '~'),
                9 => Self::build_mod_seq("\x1b[20", key_mod, '~'),
                10 => Self::build_mod_seq("\x1b[21", key_mod, '~'),
                11 => Self::build_mod_seq("\x1b[23", key_mod, '~'),
                12 => Self::build_mod_seq("\x1b[24", key_mod, '~'),
                _ => String::new(),
            }
        }
    }

    /// 根据修饰键构建转义序列
    /// 格式：start + ";" + modifier + lastChar
    /// modifier: 2=shift, 3=alt, 5=ctrl, 6=shift+ctrl, 7=alt+ctrl, 8=shift+alt+ctrl
    fn build_mod_seq(start: &str, key_mod: i32, last: char) -> String {
        let modifier = if key_mod == 0x20000000 {
            2 // shift
        } else if key_mod < 0 && key_mod == 0x80000000u32 as i32 {
            3 // alt
        } else if key_mod < 0 && key_mod == 0xA0000000u32 as i32 {
            4 // shift+alt
        } else if key_mod == 0x40000000 {
            5 // ctrl
        } else if key_mod == 0x60000000 {
            6 // shift+ctrl
        } else if key_mod < 0 && key_mod == 0xC0000000u32 as i32 {
            7 // alt+ctrl
        } else if key_mod < 0 && key_mod == 0xE0000000u32 as i32 {
            8 // shift+alt+ctrl
        } else {
            return format!("{}{}", start, last);
        };
        format!("{};{}{}", start, modifier, last)
    }

    // ========================================================================
    // OSC 序列处理方法
    // ========================================================================

    /// 设置窗口标题
    pub fn set_title(&mut self, title: &str) {
        let old_title = self.title.clone();
        self.title = Some(title.to_string());
        if old_title.as_deref() != Some(title) {
            self.report_title_change(title);
        }
    }

    /// 保存标题到栈 (OSC 22)
    pub fn push_title(&mut self, _mode: &str) {
        if let Some(ref title) = self.title {
            self.title_stack.push(title.clone());
            // 限制栈大小为 20
            if self.title_stack.len() > 20 {
                self.title_stack.remove(0);
            }
        }
    }

    /// 从栈恢复标题 (OSC 23)
    pub fn pop_title(&mut self, _mode: &str) {
        if let Some(title) = self.title_stack.pop() {
            self.set_title(&title);
        }
    }

    /// OSC 4 - 设置颜色索引
    /// 格式：4;c1;spec1;c2;spec2;... 或 4;c1;spec1;c2;spec2
    pub fn handle_osc4(&mut self, param_text: &str) {
        let parts: Vec<&str> = param_text.split(';').collect();
        let mut i = 0;

        while i + 1 < parts.len() {
            if let Ok(color_index) = parts[i].parse::<usize>() {
                let color_spec = parts[i + 1];
                if color_spec == "?" {
                    // 查询当前颜色
                    let report = self.colors.generate_color_report(color_index);
                    self.report_color_response(&format!("4;{}", report));
                } else {
                    // 设置颜色
                    if self.colors.try_parse_color(color_index, color_spec) {
                        self.report_colors_changed();
                    }
                }
            }
            i += 2;
        }
    }

    /// OSC 10 - 设置默认前景色
    pub fn handle_osc10(&mut self, param_text: &str) {
        if param_text == "?" {
            let report = self
                .colors
                .generate_color_report(COLOR_INDEX_FOREGROUND as usize);
            self.report_color_response(&format!("10;{}", report));
        } else {
            if let Some(color) = TerminalColors::parse_color(param_text) {
                self.colors.current_colors[COLOR_INDEX_FOREGROUND as usize] = color;
                self.report_colors_changed();
            }
        }
    }

    /// OSC 11 - 设置默认背景色
    pub fn handle_osc11(&mut self, param_text: &str) {
        if param_text == "?" {
            let report = self
                .colors
                .generate_color_report(COLOR_INDEX_BACKGROUND as usize);
            self.report_color_response(&format!("11;{}", report));
        } else {
            if let Some(color) = TerminalColors::parse_color(param_text) {
                self.colors.current_colors[COLOR_INDEX_BACKGROUND as usize] = color;
                self.report_colors_changed();
            }
        }
    }

    /// OSC 13 - 报告文本区域像素大小
    pub fn handle_osc13(&self) {
        let width = self.cols * self.cell_width_pixels;
        let height = self.rows * self.cell_height_pixels;
        self.report_terminal_response(&format!("\x1b]13;t={};{}t", width, height));
    }

    /// OSC 14 - 报告屏幕位置像素大小
    pub fn handle_osc14(&self) {
        // 在 Android 上，我们默认返回 0,0 位置
        let width = self.cols * self.cell_width_pixels;
        let height = self.rows * self.cell_height_pixels;
        self.report_terminal_response(&format!("\x1b]14;t=0;0;{};{}t", width, height));
    }

    /// OSC 18 - 报告文本区域单元格大小
    pub fn handle_osc18(&self) {
        self.report_terminal_response(&format!("\x1b]18;t={};{}t", self.cols, self.rows));
    }

    /// OSC 19 - 报告屏幕单元格像素大小
    pub fn handle_osc19(&self) {
        self.report_terminal_response(&format!(
            "\x1b]19;t={};{}t",
            self.cell_width_pixels, self.cell_height_pixels
        ));
    }

    /// OSC 52 - 剪贴板操作
    /// 格式：52;selection;base64_data
    pub fn handle_osc52(&mut self, base64_data: &str) {
        if base64_data == "?" {
            // 目前不支持从 Rust 侧主动读取 Java 剪贴板并通过 OSC 52 返回
            return;
        }

        // 解码 base64
        if let Ok(decoded_bytes) = base64_decode(base64_data) {
            if let Ok(text) = String::from_utf8(decoded_bytes) {
                self.report_clipboard_copy(&text);
            }
        }
    }

    /// OSC 104 - 重置颜色
    pub fn handle_osc104(&mut self, param_text: &str) {
        if param_text.is_empty() {
            self.colors.reset();
            self.report_colors_changed();
        } else {
            for part in param_text.split(';') {
                if let Ok(index) = part.parse::<usize>() {
                    self.colors.reset_index(index);
                }
            }
            self.report_colors_changed();
        }
    }

    /// DECSTR - 软重置 (CSI ! p)
    pub fn decstr_soft_reset(&mut self) {
        self.auto_wrap = true;
        self.origin_mode = false;
        self.insert_mode = false;
        self.cursor_enabled = true;
        self.top_margin = 0;
        self.bottom_margin = self.rows;
        self.left_margin = 0;
        self.right_margin = self.cols;
        self.application_cursor_keys = false;
        self.application_keypad = false;
        self.about_to_wrap = false;
        self.use_line_drawing_g0 = false;
        self.use_line_drawing_g1 = false;
        self.use_line_drawing_uses_g0 = true;
        self.reset_sgr();
        self.report_cursor_visibility(true);
        self.report_colors_changed();
    }

    /// 重置所有 SGR 属性
    pub fn reset_sgr(&mut self) {
        self.current_style = STYLE_NORMAL;
        self.fore_color = COLOR_INDEX_FOREGROUND;
        self.back_color = COLOR_INDEX_BACKGROUND;
        self.effect = 0;
    }

    /// 清除滚动计数器
    pub fn clear_scroll_counter(&mut self) {
        self.scroll_counter = 0;
    }

    /// 切换自动滚动禁用状态
    pub fn toggle_auto_scroll_disabled(&mut self) {
        self.auto_scroll_disabled = !self.auto_scroll_disabled;
    }

    pub fn clamp_cursor(&mut self) {
        self.cursor_x = max(0, min(self.cols - 1, self.cursor_x));
        self.cursor_y = max(0, min(self.rows - 1, self.cursor_y));
    }

    fn print(&mut self, c: char) {
        // 处理行绘图字符集映射
        let c = if (c as u32) >= 0x20 && (c as u32) <= 0x7E {
            if self.use_line_drawing_uses_g0 && self.use_line_drawing_g0 {
                map_line_drawing(c as u8)
            } else if !self.use_line_drawing_uses_g0 && self.use_line_drawing_g1 {
                map_line_drawing(c as u8)
            } else {
                c
            }
        } else {
            c
        };

        let ucs = c as u32;
        let char_width = crate::utils::get_char_width(ucs) as i32;
        if char_width <= 0 {
            return;
        }

        // 宽字符预判：如果当前行剩余空间不足以容纳宽字符，强制提前换行
        if char_width > 1 && self.auto_wrap && self.cursor_x + char_width > self.right_margin {
            self.cursor_x = self.left_margin;
            if self.cursor_y < self.bottom_margin - 1 {
                self.cursor_y += 1;
            } else {
                self.scroll_up();
            }
        }

        // 插入模式处理
        if self.insert_mode {
            for _ in 0..char_width {
                self.insert_character();
            }
        }

        // 写入缓冲区
        let y_internal = self.external_to_internal_row(self.cursor_y);
        let cursor_x = self.cursor_x as usize;
        let current_style = self.current_style;

        {
            let buffer = self.get_current_buffer_mut();
            let row = &mut buffer[y_internal];
            if cursor_x < row.text.len() {
                row.text[cursor_x] = c;
                row.styles[cursor_x] = current_style;

                if char_width == 2 && cursor_x + 1 < row.text.len() {
                    row.text[cursor_x + 1] = c;
                    row.styles[cursor_x + 1] = current_style;
                }
            }
        }

        // 即时更新光标并处理换行 (Immediate Wrap)
        self.cursor_x += char_width;
        if self.auto_wrap && self.cursor_x >= self.right_margin {
            self.cursor_x = self.left_margin;
            if self.cursor_y < self.bottom_margin - 1 {
                self.cursor_y += 1;
            } else {
                self.scroll_up();
            }
        }
    }

    /// 插入模式：在光标位置插入空格
    fn insert_character(&mut self) {
        let y_internal = self.external_to_internal_row(self.cursor_y);
        let cursor_x = self.cursor_x as usize;
        let current_style = self.current_style;
        let buffer = self.get_current_buffer_mut();
        let row = &mut buffer[y_internal];

        // 从右向左移动字符
        for i in (cursor_x + 1..row.text.len()).rev() {
            row.text[i] = row.text[i - 1];
            row.styles[i] = row.styles[i - 1];
        }
        if cursor_x < row.text.len() {
            row.text[cursor_x] = ' ';
            row.styles[cursor_x] = current_style;
        }
    }

    fn execute_control(&mut self, byte: u8) -> bool {
        match byte {
            0x00 => true, // NUL - 忽略
            0x07 => {
                self.report_bell();
                true
            } // BEL - 响铃
            0x08 => {
                self.cursor_x = max(self.left_margin, self.cursor_x - 1);
                self.about_to_wrap = false;
                true
            } // BS
            0x09 => {
                self.cursor_forward_tab();
                self.about_to_wrap = false;
                true
            } // HT
            0x0a..=0x0c => {
                // LF, VT, FF
                if self.cursor_y < self.bottom_margin - 1 {
                    self.cursor_y += 1;
                } else {
                    self.scroll_up();
                }
                self.about_to_wrap = false;
                true
            }
            0x0d => {
                self.cursor_x = self.left_margin;
                self.about_to_wrap = false;
                true
            } // CR
            0x0e => {
                // SO (Shift Out) - 切换到 G1 字符集
                self.use_line_drawing_uses_g0 = false;
                true
            }
            0x0f => {
                // SI (Shift In) - 切换到 G0 字符集
                self.use_line_drawing_uses_g0 = true;
                true
            }
            _ => false,
        }
    }

    /// 光标前进到下一个制表位
    fn cursor_forward_tab(&mut self) {
        // 制表位默认在 0, 8, 16, 24, ... (0-based)
        // 从当前位置 +1 开始找下一个制表位
        let mut new_col = self.cursor_x + 1;
        while new_col < self.cols
            && !self
                .tab_stops
                .get(new_col as usize)
                .copied()
                .unwrap_or(false)
        {
            new_col += 1;
        }
        self.cursor_x = min(self.right_margin - 1, max(self.left_margin, new_col));
    }

    /// 光标后退到上一个制表位
    fn cursor_backward_tab(&mut self, n: i32) {
        let mut count = n;
        while count > 0 && self.cursor_x > self.left_margin {
            self.cursor_x -= 1;
            if self
                .tab_stops
                .get(self.cursor_x as usize)
                .copied()
                .unwrap_or(false)
            {
                count -= 1;
            }
        }
        // 如果没找到制表位，确保回到左边界
        self.cursor_x = max(self.left_margin, self.cursor_x);
    }

    /// 高性能 O(1) 滚动实现
    fn scroll_up(&mut self) {
        let top = self.top_margin;
        let bottom = self.bottom_margin;

        // 增加滚动计数器（用于选择跟随滚动）
        if !self.auto_scroll_disabled {
            self.scroll_counter += 1;
        }

        if top == 0 && bottom == self.rows {
            // 全屏滚动：直接移动起始指针
            let buffer_len = self.buffer_len();
            self.screen_first_row = (self.screen_first_row + 1) % buffer_len;
            // 清理新出现的那一行（逻辑最后一行）
            let last_row_internal = self.external_to_internal_row(self.rows - 1);
            let cols = self.cols as usize;
            let current_style = self.current_style;
            let buffer = self.get_current_buffer_mut();
            buffer[last_row_internal].clear(0, cols, current_style);
        } else {
            // 区域滚动：目前仍需物理拷贝数据，但在终端中较少见
            for i in top..(bottom - 1) {
                let src_idx = self.external_to_internal_row(i + 1);
                let dest_idx = self.external_to_internal_row(i);
                let buffer = self.get_current_buffer_mut();
                let src_row = buffer[src_idx].clone();
                buffer[dest_idx] = src_row;
            }
            let clear_idx = self.external_to_internal_row(bottom - 1);
            let cols = self.cols as usize;
            let current_style = self.current_style;
            let buffer = self.get_current_buffer_mut();
            buffer[clear_idx].clear(0, cols, current_style);
        }
    }

    fn erase_in_display(&mut self, mode: i32) {
        let cols = self.cols as usize;
        let current_style = self.current_style;
        let cursor_y = self.cursor_y;
        let rows = self.rows;

        match mode {
            0 => {
                self.erase_in_line(0);
                for y in (cursor_y + 1)..rows {
                    let idx = self.external_to_internal_row(y);
                    let buffer = self.get_current_buffer_mut();
                    buffer[idx].clear(0, cols, current_style);
                }
            }
            1 => {
                self.erase_in_line(1);
                for y in 0..cursor_y {
                    let idx = self.external_to_internal_row(y);
                    let buffer = self.get_current_buffer_mut();
                    buffer[idx].clear(0, cols, current_style);
                }
            }
            2 => {
                for y in 0..rows {
                    let idx = self.external_to_internal_row(y);
                    let buffer = self.get_current_buffer_mut();
                    buffer[idx].clear(0, cols, current_style);
                }
            }
            3 => {
                // 清除整个物理缓冲区（包括滚动历史）
                let buffer = self.get_current_buffer_mut();
                for row in buffer {
                    row.clear(0, cols, current_style);
                }
                // 重置滚动指针，使当前屏幕位于缓冲区开头
                self.screen_first_row = 0;
                // 重置滚动计数器（对齐 Java 端行为）
                self.scroll_counter = 0;
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: i32) {
        let idx = self.external_to_internal_row(self.cursor_y);
        let cursor_x = self.cursor_x as usize;
        let cols = self.cols as usize;
        let current_style = self.current_style;
        let buffer = self.get_current_buffer_mut();
        let row_len = buffer[idx].text.len();
        let x = min(cursor_x, if row_len > 0 { row_len - 1 } else { 0 });
        match mode {
            0 => {
                buffer[idx].clear(cursor_x, cols, current_style);
            }
            1 => {
                buffer[idx].clear(0, min(row_len, x + 1), current_style);
            }
            2 => {
                buffer[idx].clear(0, cols, current_style);
            }
            _ => {}
        }
    }

    /// 插入字符 (ICH) - CSI {N} @
    fn insert_characters(&mut self, n: i32) {
        let columns_after_cursor = self.right_margin - self.cursor_x;
        let spaces_to_insert = min(n, columns_after_cursor);

        let y_internal = self.external_to_internal_row(self.cursor_y);
        let cursor_x = self.cursor_x as usize;
        let current_style = self.current_style;
        let buffer = self.get_current_buffer_mut();
        let row = &mut buffer[y_internal];

        // 在边界内移动字符
        let move_start = cursor_x;
        let move_count = (columns_after_cursor - spaces_to_insert) as usize;
        let insert_count = spaces_to_insert as usize;

        // 从右向左移动字符
        for i in (move_start..(move_start + move_count).min(row.text.len())).rev() {
            let dest = (i + insert_count).min(row.text.len() - 1);
            row.text[dest] = row.text[i];
            row.styles[dest] = row.styles[i];
        }

        // 清空插入的区域（用空格填充）
        for i in move_start..(move_start + insert_count).min(row.text.len()) {
            row.text[i] = ' ';
            row.styles[i] = current_style;
        }

        // ICH 后光标位置不变
    }

    /// 删除字符 (DCH) - CSI {N} P
    fn delete_characters(&mut self, n: i32) {
        let columns_after_cursor = self.right_margin - self.cursor_x;
        let cells_to_delete = min(n, columns_after_cursor);
        let cells_to_move = columns_after_cursor - cells_to_delete;
        let style = self.current_style;

        let y_internal = self.external_to_internal_row(self.cursor_y);
        let cursor_x = self.cursor_x as usize;
        let right_margin = self.right_margin as usize;
        let buffer = self.get_current_buffer_mut();
        let row = &mut buffer[y_internal];

        // 从左向右移动字符
        for i in 0..cells_to_move as usize {
            let src = cursor_x + i + cells_to_delete as usize;
            let dest = cursor_x + i;
            if src < row.text.len() && dest < row.text.len() {
                row.text[dest] = row.text[src];
                row.styles[dest] = row.styles[src];
            }
        }

        // 清空右侧区域
        let clear_start = cursor_x + cells_to_move as usize;
        for i in clear_start..min(right_margin, row.text.len()) {
            row.text[i] = ' ';
            row.styles[i] = style;
        }
    }

    /// 插入行 (IL) - CSI {N} L
    fn insert_lines(&mut self, n: i32) {
        let lines_after_cursor = self.bottom_margin - self.cursor_y;
        let lines_to_insert = min(n, lines_after_cursor);
        let lines_to_move = lines_after_cursor - lines_to_insert;
        let cursor_y = self.cursor_y;
        let cols = self.cols as usize;

        // 从下向上移动行
        for i in (0..lines_to_move as usize).rev() {
            let src_row = cursor_y as usize + i;
            let dest_row = cursor_y as usize + i + lines_to_insert as usize;

            if dest_row < self.rows as usize {
                let src_idx = self.external_to_internal_row(src_row as i32);
                let dest_idx = self.external_to_internal_row(dest_row as i32);
                let buffer = self.get_current_buffer_mut();
                let src_data = buffer[src_idx].clone();
                buffer[dest_idx] = src_data;
            }
        }

        // 清空插入的区域
        let style = self.current_style;
        let top_margin = self.top_margin;
        for i in 0..lines_to_insert as usize {
            let clear_idx = self.external_to_internal_row(top_margin + i as i32);
            let buffer = self.get_current_buffer_mut();
            buffer[clear_idx].clear(0, cols, style);
        }
    }

    /// 删除行 (DL) - CSI {N} M
    fn delete_lines(&mut self, n: i32) {
        let lines_after_cursor = self.bottom_margin - self.cursor_y;
        let lines_to_delete = min(n, lines_after_cursor);
        let lines_to_move = lines_after_cursor - lines_to_delete;
        let cursor_y = self.cursor_y;
        let cols = self.cols as usize;

        // 从上向下移动行
        for i in 0..lines_to_move as usize {
            let src_row = cursor_y as usize + i + lines_to_delete as usize;
            let dest_row = cursor_y as usize + i;

            let src_idx = self.external_to_internal_row(src_row as i32);
            let dest_idx = self.external_to_internal_row(dest_row as i32);
            let buffer = self.get_current_buffer_mut();
            let src_data = buffer[src_idx].clone();
            buffer[dest_idx] = src_data;
        }

        // 清空底部区域
        let style = self.current_style;
        let bottom_margin = self.bottom_margin;
        for i in 0..lines_to_delete as usize {
            let clear_idx = self.external_to_internal_row(bottom_margin - i as i32 - 1);
            let buffer = self.get_current_buffer_mut();
            buffer[clear_idx].clear(0, cols, style);
        }
    }

    /// 擦除字符 (ECH) - CSI {N} X
    fn erase_characters(&mut self, n: i32) {
        let style = self.current_style;
        let cols = self.cols;
        let cursor_x = self.cursor_x;
        let y_internal = self.external_to_internal_row(self.cursor_y);
        
        let chars_to_erase = min(n, cols - cursor_x);
        let buffer = self.get_current_buffer_mut();
        let row = &mut buffer[y_internal];

        let start = cursor_x as usize;
        let end = min(start + chars_to_erase as usize, row.text.len());
        row.clear(start, end, style);
        self.about_to_wrap = false;
    }

    /// 光标水平绝对 (CHA) - CSI {N} G
    fn cursor_horizontal_absolute(&mut self, n: i32) {
        let col = max(1, n) - 1;
        self.cursor_x = min(max(0, col), self.cols - 1);
        self.about_to_wrap = false;
    }

    /// 光标水平相对 (HPR) - CSI {N} a
    fn cursor_horizontal_relative(&mut self, n: i32) {
        self.cursor_x = min(
            self.right_margin - 1,
            max(self.left_margin, self.cursor_x + n),
        );
        self.about_to_wrap = false;
    }

    /// 下一行 (CNL) - CSI {N} E
    fn cursor_next_line(&mut self, n: i32) {
        self.cursor_x = self.left_margin;
        self.cursor_y = min(self.bottom_margin - 1, self.cursor_y + n);
        self.about_to_wrap = false;
    }

    /// 上一行 (CPL) - CSI {N} F
    fn cursor_previous_line(&mut self, n: i32) {
        self.cursor_x = self.left_margin;
        self.cursor_y = max(self.top_margin, self.cursor_y - n);
        self.about_to_wrap = false;
    }

    /// 垂直绝对 (VPA) - CSI {N} d
    fn cursor_vertical_absolute(&mut self, n: i32) {
        let row = max(1, n) - 1;
        self.cursor_y = min(max(0, row), self.rows - 1);
        self.about_to_wrap = false;
    }

    /// 垂直相对 (VPR) - CSI {N} e
    fn cursor_vertical_relative(&mut self, n: i32) {
        self.cursor_y = min(self.rows - 1, max(0, self.cursor_y + n));
        self.about_to_wrap = false;
    }

    /// 重复字符 (REP) - CSI {N} b
    fn repeat_character(&mut self, n: i32, last_char: char) {
        for _ in 0..n {
            self.print(last_char);
        }
    }

    /// 上滚 (SU) - CSI {N} S
    fn scroll_up_lines(&mut self, n: i32) {
        for _ in 0..n {
            self.scroll_up();
        }
        // 滚动后光标保持在顶部
        self.cursor_x = self.left_margin;
        self.cursor_y = self.top_margin;
        self.about_to_wrap = false;
    }

    /// 下滚 (SD) - CSI {N} T
    fn scroll_down_lines(&mut self, n: i32) {
        let lines_to_scroll = min(n, self.bottom_margin - self.top_margin);
        let top_margin = self.top_margin;
        let bottom_margin = self.bottom_margin;
        let cols = self.cols as usize;

        // 从上向下移动行
        for i in (0..(bottom_margin - top_margin - lines_to_scroll) as usize).rev() {
            let src_row = top_margin as usize + i;
            let dest_row = top_margin as usize + i + lines_to_scroll as usize;

            if dest_row < self.rows as usize {
                let src_idx = self.external_to_internal_row(src_row as i32);
                let dest_idx = self.external_to_internal_row(dest_row as i32);
                let buffer = self.get_current_buffer_mut();
                let src_data = buffer[src_idx].clone();
                buffer[dest_idx] = src_data;
            }
        }

        // 清空顶部区域
        for i in 0..lines_to_scroll as usize {
            let clear_idx = self.external_to_internal_row(top_margin + i as i32);
            let current_style = self.current_style;
            let buffer = self.get_current_buffer_mut();
            buffer[clear_idx].clear(0, cols, current_style);
        }

        // 滚动后光标保持在顶部
        self.cursor_x = self.left_margin;
        self.cursor_y = self.top_margin;
        self.about_to_wrap = false;
    }

    /// DECBI - Back Index 滚动 (ESC 6)
    /// 当光标在左边界时，向左滚动并插入空白列
    fn back_index_scroll(&mut self) {
        let top_margin = self.top_margin;
        let bottom_margin = self.bottom_margin;
        let cols = self.cols as usize;

        // 向左滚动：将区域内所有列向右移动一列
        for y in top_margin..bottom_margin {
            let idx = self.external_to_internal_row(y);
            let buffer = self.get_current_buffer_mut();
            let row = &mut buffer[idx];

            // 从右向左移动字符
            for x in (1..cols).rev() {
                if x < row.text.len() {
                    row.text[x] = row.text[x - 1];
                    row.styles[x] = row.styles[x - 1];
                }
            }
            // 第一列填充空格
            if row.text.len() > 0 {
                row.text[0] = ' ';
                row.styles[0] = STYLE_NORMAL;
            }
        }
    }

    /// DECFI - Forward Index 滚动 (ESC 9)
    /// 当光标在右边界时，向右滚动并插入空白列
    fn forward_index_scroll(&mut self) {
        let top_margin = self.top_margin;
        let bottom_margin = self.bottom_margin;
        let cols = self.cols as usize;

        // 向右滚动：将区域内所有列向左移动一列
        for y in top_margin..bottom_margin {
            let idx = self.external_to_internal_row(y);
            let buffer = self.get_current_buffer_mut();
            let row = &mut buffer[idx];

            // 从左向右移动字符
            for x in 0..(cols - 1) {
                if x < row.text.len() && x + 1 < row.text.len() {
                    row.text[x] = row.text[x + 1];
                    row.styles[x] = row.styles[x + 1];
                }
            }
            // 最后一列填充空格
            let last_col = (cols - 1).min(row.text.len().saturating_sub(1));
            if row.text.len() > last_col {
                row.text[last_col] = ' ';
                row.styles[last_col] = STYLE_NORMAL;
            }
        }
    }

    /// RI - Reverse Index 滚动 (ESC M)
    /// 当光标在顶部边距时，向下滚动并插入空白行
    fn reverse_index_scroll(&mut self) {
        let top_margin = self.top_margin;
        let bottom_margin = self.bottom_margin;
        let cols = self.cols as usize;

        // 向下滚动：将区域内所有行向下移动一行
        for y in (top_margin + 1..bottom_margin).rev() {
            let src_idx = self.external_to_internal_row(y - 1);
            let dest_idx = self.external_to_internal_row(y);
            let buffer = self.get_current_buffer_mut();
            let src_data = buffer[src_idx].clone();
            buffer[dest_idx] = src_data;
        }
        // 清空顶部行
        let clear_idx = self.external_to_internal_row(self.top_margin);
        let style = self.current_style;
        let buffer = self.get_current_buffer_mut();
        buffer[clear_idx].clear(0, cols, style);
    }

    /// DECALN - 屏幕对齐测试 (ESC # 8)
    /// 用字母 'E' 填充整个屏幕，用于测试屏幕对齐
    fn decaln_screen_align(&mut self) {
        let cols = self.cols as usize;
        let rows = self.rows;

        for y in 0..rows as usize {
            let idx = self.external_to_internal_row(y as i32);
            let buffer = self.get_current_buffer_mut();
            let row = &mut buffer[idx];
            for x in 0..row.text.len().min(cols) {
                row.text[x] = 'E';
                row.styles[x] = STYLE_NORMAL;
            }
        }
        // 移动光标到左上角
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    /// RIS - 重置到初始状态 (http://vt100.net/docs/vt510-rm/RIS)
    /// 完整重置：清屏、重置光标、重置样式、重置边距、重置制表位、重置颜色
    pub fn reset_to_initial_state(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.current_style = STYLE_NORMAL;
        let cols = self.cols as usize;
        let rows = self.rows;

        // 清屏
        for y in 0..rows as usize {
            let idx = self.external_to_internal_row(y as i32);
            let buffer = self.get_current_buffer_mut();
            buffer[idx].clear(0, cols, STYLE_NORMAL);
        }

        // 重置所有制表位
        for stop in &mut self.tab_stops {
            *stop = false;
        }

        // 重置边距
        self.top_margin = 0;
        self.bottom_margin = self.rows;
        self.left_margin = 0;
        self.right_margin = self.cols;

        // 重置 DECSET 标志
        self.decset_flags = 0;
        self.auto_wrap = true;
        self.origin_mode = false;
        self.cursor_enabled = true;
        self.application_cursor_keys = false;
        self.application_keypad = false;
        self.reverse_video = false;
        self.insert_mode = false;
        self.bracketed_paste = false;
        self.mouse_tracking = false;
        self.mouse_button_event = false;
        self.sgr_mouse = false;
        self.leftright_margin_mode = false;
        self.send_focus_events = false;

        // 重置行绘图状态
        self.use_line_drawing_g0 = false;
        self.use_line_drawing_g1 = false;
        self.use_line_drawing_uses_g0 = true;

        // 重置 SGR 属性
        self.reset_sgr();

        // 重置颜色为默认值
        self.colors.reset();

        // 重置标题
        self.title = None;
        self.title_stack.clear();

        // 重置滚动计数器
        self.scroll_counter = 0;

        // 通知 Java 层
        self.report_colors_changed();
    }

    /// 清除制表位 (TBC) - CSI {N} g
    fn clear_tab_stop(&mut self, mode: i32) {
        match mode {
            0 => {
                // 清除当前列的制表位
                if self.cursor_x >= 0 && (self.cursor_x as usize) < self.tab_stops.len() {
                    self.tab_stops[self.cursor_x as usize] = false;
                }
            }
            3 => {
                // 清除所有制表位
                for stop in &mut self.tab_stops {
                    *stop = false;
                }
            }
            _ => {}
        }
    }

    /// 完整的 SGR 处理（与 Java TextStyle 格式兼容）
    /// 同时更新 current_style 和独立颜色字段 (fore_color, back_color, effect, underline_color)
    fn handle_sgr(&mut self, params: &Params) {
        let params_vec: Vec<u16> = params.iter().flat_map(|p| p.iter().copied()).collect();
        let mut i = 0;

        // 如果没有参数，默认为重置
        if params_vec.is_empty() {
            self.reset_sgr();
            return;
        }

        while i < params_vec.len() {
            let code = params_vec[i];
            match code {
                0 => self.reset_sgr(),
                1 => {
                    self.effect |= EFFECT_BOLD;
                    self.current_style |= EFFECT_BOLD;
                }
                2 => {
                    self.effect |= EFFECT_DIM;
                    self.current_style |= EFFECT_DIM;
                }
                3 => {
                    self.effect |= EFFECT_ITALIC;
                    self.current_style |= EFFECT_ITALIC;
                }
                4 => {
                    // 下划线（支持子参数）
                    if i + 1 < params_vec.len() && params_vec.get(i + 1) == Some(&0) {
                        // 子参数 0 表示无下划线
                        self.effect &= !EFFECT_UNDERLINE;
                        self.current_style &= !EFFECT_UNDERLINE;
                        i += 1;
                    } else {
                        self.effect |= EFFECT_UNDERLINE;
                        self.current_style |= EFFECT_UNDERLINE;
                    }
                }
                5 => {
                    self.effect |= EFFECT_BLINK;
                    self.current_style |= EFFECT_BLINK;
                }
                7 => {
                    self.effect |= EFFECT_REVERSE;
                    self.current_style |= EFFECT_REVERSE;
                }
                8 => {
                    self.effect |= EFFECT_INVISIBLE;
                    self.current_style |= EFFECT_INVISIBLE;
                }
                9 => {
                    self.effect |= EFFECT_STRIKETHROUGH;
                    self.current_style |= EFFECT_STRIKETHROUGH;
                }
                21 => {
                    self.effect |= EFFECT_BOLD;
                    self.current_style |= EFFECT_BOLD;
                }
                22 => {
                    self.effect &= !(EFFECT_BOLD | EFFECT_DIM);
                    self.current_style &= !(EFFECT_BOLD | EFFECT_DIM);
                }
                23 => {
                    self.effect &= !EFFECT_ITALIC;
                    self.current_style &= !EFFECT_ITALIC;
                }
                24 => {
                    self.effect &= !EFFECT_UNDERLINE;
                    self.current_style &= !EFFECT_UNDERLINE;
                }
                25 => {
                    self.effect &= !EFFECT_BLINK;
                    self.current_style &= !EFFECT_BLINK;
                }
                27 => {
                    self.effect &= !EFFECT_REVERSE;
                    self.current_style &= !EFFECT_REVERSE;
                }
                28 => {
                    self.effect &= !EFFECT_INVISIBLE;
                    self.current_style &= !EFFECT_INVISIBLE;
                }
                29 => {
                    self.effect &= !EFFECT_STRIKETHROUGH;
                    self.current_style &= !EFFECT_STRIKETHROUGH;
                }
                30..=37 => {
                    // 前景色 30-37（标准颜色 0-7）
                    let color = (code - 30) as u64;
                    self.fore_color = color;
                    self.current_style = (self.current_style & !STYLE_MASK_FG & !STYLE_TRUECOLOR_FG) | (color << 40);
                }
                38 => {
                    // 扩展前景色 (38;5;n 或 38;2;r;g;b)
                    if i + 1 < params_vec.len() {
                        let mode = params_vec[i + 1];
                        if mode == 5 && i + 2 < params_vec.len() {
                            // 256 色索引
                            let color = params_vec[i + 2] as u64;
                            self.fore_color = color;
                            self.current_style =
                                (self.current_style & !STYLE_MASK_FG & !STYLE_TRUECOLOR_FG) | ((color & 0x1FF) << 40);
                            i += 2;
                        } else if mode == 2 && i + 4 < params_vec.len() {
                            // 24 位真彩色 (38;2;R;G;B)
                            let r = params_vec[i + 2] as u64;
                            let g = params_vec[i + 3] as u64;
                            let b = params_vec[i + 4] as u64;
                            let truecolor = 0xff000000 | (r << 16) | (g << 8) | b;
                            self.fore_color = truecolor;
                            self.current_style = (self.current_style & !STYLE_MASK_FG & !STYLE_TRUECOLOR_FG)
                                | STYLE_TRUECOLOR_FG
                                | ((truecolor & 0x00ffffff) << 40);
                            i += 4;
                        }
                    }
                }
                39 => {
                    // 默认前景色
                    self.fore_color = COLOR_INDEX_FOREGROUND;
                    self.current_style =
                        (self.current_style & !STYLE_MASK_FG & !STYLE_TRUECOLOR_FG) | (COLOR_INDEX_FOREGROUND << 40);
                }
                40..=47 => {
                    // 背景色 40-47（标准颜色 0-7）
                    let color = (code - 40) as u64;
                    self.back_color = color;
                    self.current_style = (self.current_style & !STYLE_MASK_BG & !STYLE_TRUECOLOR_BG) | (color << 16);
                }
                48 => {
                    // 扩展背景色 (48;5;n 或 48;2;r;g;b)
                    if i + 1 < params_vec.len() {
                        let mode = params_vec[i + 1];
                        if mode == 5 && i + 2 < params_vec.len() {
                            // 256 色索引
                            let color = params_vec[i + 2] as u64;
                            self.back_color = color;
                            self.current_style =
                                (self.current_style & !STYLE_MASK_BG & !STYLE_TRUECOLOR_BG) | ((color & 0x1FF) << 16);
                            i += 2;
                        } else if mode == 2 && i + 4 < params_vec.len() {
                            // 24 位真彩色 (48;2;R;G;B)
                            let r = params_vec[i + 2] as u64;
                            let g = params_vec[i + 3] as u64;
                            let b = params_vec[i + 4] as u64;
                            let truecolor = 0xff000000 | (r << 16) | (g << 8) | b;
                            self.back_color = truecolor;
                            self.current_style = (self.current_style & !STYLE_MASK_BG & !STYLE_TRUECOLOR_BG)
                                | STYLE_TRUECOLOR_BG
                                | ((truecolor & 0x00ffffff) << 16);
                            i += 4;
                        }
                    }
                }
                49 => {
                    // 默认背景色
                    self.back_color = COLOR_INDEX_BACKGROUND;
                    self.current_style =
                        (self.current_style & !STYLE_MASK_BG & !STYLE_TRUECOLOR_BG) | (COLOR_INDEX_BACKGROUND << 16);
                }
                58 => {
                    // 下划线颜色 (58;5;n 或 58;2;r;g;b)
                    if i + 1 < params_vec.len() {
                        let mode = params_vec[i + 1];
                        if mode == 5 && i + 2 < params_vec.len() {
                            // 256 色索引
                            self.underline_color = params_vec[i + 2] as u64;
                            i += 2;
                        } else if mode == 2 && i + 4 < params_vec.len() {
                            // 24 位真彩色
                            let r = params_vec[i + 2] as u64;
                            let g = params_vec[i + 3] as u64;
                            let b = params_vec[i + 4] as u64;
                            self.underline_color = 0xff000000 | (r << 16) | (g << 8) | b;
                            i += 4;
                        }
                    }
                }
                59 => {
                    // 默认下划线颜色
                    self.underline_color = COLOR_INDEX_FOREGROUND;
                }
                90..=97 => {
                    // 亮色前景色 90-97（高亮颜色 8-15）
                    let color = (code - 90 + 8) as u64;
                    self.fore_color = color;
                    self.current_style = (self.current_style & !STYLE_MASK_FG & !STYLE_TRUECOLOR_FG) | (color << 40);
                }
                100..=107 => {
                    // 亮色背景色 100-107（高亮颜色 8-15）
                    let color = (code - 100 + 8) as u64;
                    self.back_color = color;
                    self.current_style = (self.current_style & !STYLE_MASK_BG & !STYLE_TRUECOLOR_BG) | (color << 16);
                }
                _ => {} // 忽略未知参数
            }
            i += 1;
        }
    }

    /// 处理设置/重置模式 (SM/RM)
    fn handle_set_mode(&mut self, params: &Params, set: bool) {
        for param in params {
            for &val in param {
                match val {
                    4 => self.insert_mode = set, // IRM - 插入模式
                    20 => {}                     // LNM - 自动换行（忽略）
                    _ => {}                      // 其他模式忽略
                }
            }
        }
    }

    /// 处理 DECSET/DECRST 私有模式 (CSI ? h / CSI ? l)
    fn handle_decset(&mut self, params: &Params, set: bool) {
        for param in params {
            for &val in param {
                match val {
                    1 => {
                        // DECCKM - 应用光标键
                        self.application_cursor_keys = set;
                        self.update_decset_flag(DECSET_BIT_APPLICATION_CURSOR_KEYS, set);
                    }
                    3 => {
                        // DECCOLM - 列模式 (80/132)
                        // 简化处理：忽略列切换，只记录状态
                    }
                    5 => {
                        // DECSCNM - 反显模式
                        self.reverse_video = set;
                        self.update_decset_flag(DECSET_BIT_REVERSE_VIDEO, set);
                    }
                    6 => {
                        // DECOM - 原点模式
                        self.origin_mode = set;
                        self.update_decset_flag(DECSET_BIT_ORIGIN_MODE, set);
                    }
                    7 => {
                        // DECAWM - 自动换行
                        self.auto_wrap = set;
                        self.update_decset_flag(DECSET_BIT_AUTOWRAP, set);
                    }
                    12 => {
                        // 本地回显（忽略）
                    }
                    25 => {
                        // DECTCEM - 光标可见性
                        self.cursor_enabled = set;
                        self.update_decset_flag(DECSET_BIT_CURSOR_ENABLED, set);
                        self.report_cursor_visibility(set);
                    }
                    66 => {
                        // DECNKM - 应用键盘
                        self.application_keypad = set;
                        self.update_decset_flag(DECSET_BIT_APPLICATION_KEYPAD, set);
                    }
                    69 => {
                        // DECLRMM - 左右边距模式
                        self.leftright_margin_mode = set;
                        self.update_decset_flag(DECSET_BIT_LEFTRIGHT_MARGIN_MODE, set);
                    }
                    1000 => {
                        // 鼠标跟踪（按下&释放）
                        // 鼠标模式互斥：设置 1000 时清除 1002
                        if set {
                            self.mouse_tracking = true;
                            self.mouse_button_event = false;
                            self.update_decset_flag(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE, true);
                            self.update_decset_flag(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT, false);
                        } else {
                            self.mouse_tracking = false;
                            self.update_decset_flag(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE, false);
                        }
                    }
                    1002 => {
                        // 鼠标按钮事件跟踪
                        // 鼠标模式互斥：设置 1002 时清除 1000
                        if set {
                            self.mouse_button_event = true;
                            self.mouse_tracking = false;
                            self.update_decset_flag(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT, true);
                            self.update_decset_flag(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE, false);
                        } else {
                            self.mouse_button_event = false;
                            self.update_decset_flag(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT, false);
                        }
                    }
                    1004 => {
                        // 发送焦点事件
                        self.send_focus_events = set;
                        self.update_decset_flag(DECSET_BIT_SEND_FOCUS_EVENTS, set);
                    }
                    1006 => {
                        // SGR 鼠标协议
                        self.sgr_mouse = set;
                        self.update_decset_flag(DECSET_BIT_MOUSE_PROTOCOL_SGR, set);
                    }
                    2004 => {
                        // 括号粘贴模式
                        self.bracketed_paste = set;
                        self.update_decset_flag(DECSET_BIT_BRACKETED_PASTE_MODE, set);
                    }
                    1048 => {
                        // 备用光标 (保存/恢复光标位置)
                        if set {
                            // 保存当前光标位置
                            self.saved_main_cursor_x = self.cursor_x;
                            self.saved_main_cursor_y = self.cursor_y;
                        } else {
                            // 恢复光标位置
                            self.cursor_x = self.saved_main_cursor_x;
                            self.cursor_y = self.saved_main_cursor_y;
                            self.clamp_cursor();
                        }
                    }
                    1049 => {
                        // 备用屏幕缓冲区 (包含 1048 的光标保存/恢复)
                        if set {
                            // 切换到备用缓冲区
                            // 保存当前光标位置
                            self.saved_main_cursor_x = self.cursor_x;
                            self.saved_main_cursor_y = self.cursor_y;
                            self.saved_main_decset_flags = self.decset_flags;
                            self.saved_main_screen_first_row = self.screen_first_row;

                            // 切换到备用缓冲区
                            self.use_alternate_buffer = true;

                            // 清除备用缓冲区
                            self.clear_alt_buffer();

                            // 重置光标到左上角
                            self.cursor_x = 0;
                            self.cursor_y = 0;
                        } else {
                            // 切换到主缓冲区
                            self.use_alternate_buffer = false;

                            // 恢复光标位置
                            self.cursor_x = self.saved_main_cursor_x;
                            self.cursor_y = self.saved_main_cursor_y;
                            self.clamp_cursor();
                        }
                    }
                    _ => {
                        // 忽略未知模式
                    }
                }
            }
        }
    }

    /// 更新 DECSET 标志位
    fn update_decset_flag(&mut self, bit: i32, set: bool) {
        if set {
            self.decset_flags |= bit;
        } else {
            self.decset_flags &= !bit;
        }
    }

    /// 设置上下边距 (DECSTBM)
    fn set_margins(&mut self, top: i32, bottom: i32) {
        // DECSTBM 使用 1-based 索引
        let t = max(1, top) - 1;
        let b = min(self.rows, max(t + 1, bottom));

        self.top_margin = t;
        self.bottom_margin = b;

        // DECSTBM 移动光标到左上角
        self.cursor_x = self.left_margin;
        self.cursor_y = if self.origin_mode { self.top_margin } else { 0 };
    }

    /// 设置左右边距 (DECSLRM) - CSI $ P_left ; $ P_right s
    fn set_left_right_margins(&mut self, left: i32, right: i32) {
        // 只有在 DECLRMM 模式下才有效
        if !self.leftright_margin_mode {
            return;
        }

        // DECSLRM 使用 1-based 索引
        let l = max(1, left) - 1;
        let r = min(self.cols, max(l + 1, right));

        self.left_margin = l;
        self.right_margin = r;

        // DECSLRM 移动光标到左上角
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    /// 保存光标 (DECSC)
    /// 保存：光标位置、样式、DECSET 标志、行绘图状态、颜色属性
    fn save_cursor(&mut self) {
        self.saved_x = self.cursor_x;
        self.saved_y = self.cursor_y;
        self.saved_style = self.current_style;
        self.saved_about_to_wrap = self.about_to_wrap;
        // 保存 DECSET 标志（与 Java 端一致，只保存相关标志）
        // 包括：AUTOWRAP, ORIGIN_MODE
        let mask = DECSET_BIT_AUTOWRAP | DECSET_BIT_ORIGIN_MODE;
        self.saved_decset_flags = self.decset_flags & mask;

        // 保存行绘图状态
        self.saved_use_line_drawing_g0 = self.use_line_drawing_g0;
        self.saved_use_line_drawing_g1 = self.use_line_drawing_g1;
        self.saved_use_line_drawing_uses_g0 = self.use_line_drawing_uses_g0;

        // 保存颜色属性
        self.saved_fore_color = self.fore_color;
        self.saved_back_color = self.back_color;
    }

    /// 恢复光标 (DECRC)
    /// 恢复：光标位置、样式、DECSET 标志、行绘图状态、颜色属性
    fn restore_cursor(&mut self) {
        self.cursor_x = self.saved_x;
        self.cursor_y = self.saved_y;
        self.current_style = self.saved_style;
        self.about_to_wrap = self.saved_about_to_wrap;
        // 恢复 DECSET 标志（只恢复 AUTOWRAP 和 ORIGIN_MODE）
        let mask = DECSET_BIT_AUTOWRAP | DECSET_BIT_ORIGIN_MODE;
        self.decset_flags = (self.decset_flags & !mask) | (self.saved_decset_flags & mask);
        self.auto_wrap = (self.decset_flags & DECSET_BIT_AUTOWRAP) != 0;
        self.origin_mode = (self.decset_flags & DECSET_BIT_ORIGIN_MODE) != 0;

        // 恢复行绘图状态
        self.use_line_drawing_g0 = self.saved_use_line_drawing_g0;
        self.use_line_drawing_g1 = self.saved_use_line_drawing_g1;
        self.use_line_drawing_uses_g0 = self.saved_use_line_drawing_uses_g0;

        // 恢复颜色属性
        self.fore_color = self.saved_fore_color;
        self.back_color = self.saved_back_color;
    }

    pub fn copy_row_text(&self, row: usize, dest: &mut [u16]) {
        let idx = self.external_to_internal_row(row as i32);
        let buffer = self.get_current_buffer();
        let src = &buffer[idx].text;
        let mut dest_idx = 0;

        for &c in src {
            if dest_idx >= dest.len() {
                break;
            }
            if c == '\0' {
                continue;
            } // Skip placeholder for the second half of wide characters

            let mut b = [0; 2];
            let encoded = c.encode_utf16(&mut b);
            for &u in encoded.iter() {
                if dest_idx < dest.len() {
                    dest[dest_idx] = u;
                    dest_idx += 1;
                }
            }
        }

        // Pad the rest with spaces just in case
        while dest_idx < dest.len() {
            dest[dest_idx] = ' ' as u16;
            dest_idx += 1;
        }
    }

    pub fn copy_row_styles(&self, row: usize, dest: &mut [i64]) {
        let idx = self.external_to_internal_row(row as i32);
        let buffer = self.get_current_buffer();
        let src = &buffer[idx].styles;
        for i in 0..min(src.len(), dest.len()) {
            dest[i] = src[i] as i64;
        }
    }

    pub fn resize(&mut self, new_cols: i32, new_rows: i32) {
        // 1. 先扩容主缓冲区
        let mut main_indices = Vec::with_capacity(self.rows as usize);
        for y in 0..self.rows {
            main_indices.push(self.external_to_internal_row(y));
        }

        let mut new_main = Vec::with_capacity(max(new_rows as usize, self.buffer.len()));
        for old_idx in main_indices {
            let mut row = self.buffer[old_idx].clone();
            row.text.resize(new_cols as usize, ' ');
            row.styles.resize(new_cols as usize, 0);
            new_main.push(row);
        }
        while new_main.len() < new_rows as usize {
            new_main.push(TerminalRow::new(new_cols as usize));
        }
        self.buffer = new_main;

        // 2. 扩容备用缓冲区
        let mut new_alt = Vec::with_capacity(new_rows as usize);
        for row in &self.alt_buffer {
            let mut new_row = row.clone();
            new_row.text.resize(new_cols as usize, ' ');
            new_row.styles.resize(new_cols as usize, 0);
            new_alt.push(new_row);
        }
        while new_alt.len() < new_rows as usize {
            new_alt.push(TerminalRow::new(new_cols as usize));
        }
        self.alt_buffer = new_alt;

        // 3. 更新状态
        self.screen_first_row = 0;
        self.cols = new_cols;
        self.rows = new_rows;
        self.bottom_margin = new_rows;
        self.right_margin = new_cols;
        self.clamp_cursor();
    }
}

pub struct PurePerformHandler<'a> {
    pub state: &'a mut ScreenState,
    pub unhandled_sequences: Vec<String>,
    pub last_printed_char: Option<char>,
}

impl<'a> Perform for PurePerformHandler<'a> {
    fn print(&mut self, c: char) {
        self.last_printed_char = Some(c);
        self.state.print(c);
    }

    fn execute(&mut self, byte: u8) {
        self.state.execute_control(byte);
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // OSC 序列处理
        if params.is_empty() {
            return;
        }

        let opcode = std::str::from_utf8(params[0]).unwrap_or("");

        // 将所有参数拼接成字符串供后续处理
        let param_text = params[1..]
            .iter()
            .filter_map(|p| std::str::from_utf8(p).ok())
            .collect::<Vec<&str>>()
            .join(";");

        match opcode {
            "0" => {
                // 设置图标名和窗口标题
                if params.len() > 1 {
                    let title = std::str::from_utf8(params[1]).unwrap_or("");
                    self.state.set_title(title);
                }
            }
            "2" => {
                // 设置窗口标题
                if params.len() > 1 {
                    let title = std::str::from_utf8(params[1]).unwrap_or("");
                    self.state.set_title(title);
                }
            }
            "4" => {
                // OSC 4 ; c ; spec → 设置颜色索引 c 为 spec
                // 格式：4;c;spec 或 4;c1;spec1;c2;spec2;...
                self.state.handle_osc4(&param_text);
            }
            "10" => {
                // OSC 10 ; spec → 设置默认前景色
                self.state.handle_osc10(&param_text);
            }
            "11" => {
                // OSC 11 ; spec → 设置默认背景色
                self.state.handle_osc11(&param_text);
            }
            "12" => {
                // OSC 12 ; spec → 设置光标颜色
                if let Some(color) = TerminalColors::parse_color(&param_text) {
                    self.state.colors.current_colors[COLOR_INDEX_CURSOR as usize] = color;
                    self.state.report_colors_changed();
                }
            }
            "13" => {
                self.state.handle_osc13();
            }
            "14" => {
                self.state.handle_osc14();
            }
            "18" => {
                self.state.handle_osc18();
            }
            "19" => {
                self.state.handle_osc19();
            }
            "22" => {
                // OSC 22 ; 0 → 保存图标和窗口标题到栈
                // OSC 22 ; 1 → 保存图标标题到栈
                // OSC 22 ; 2 → 保存窗口标题到栈
                self.state.push_title(opcode);
            }
            "23" => {
                // OSC 23 → 从栈恢复标题
                // OSC 23 ; 0 → 恢复图标和窗口标题
                // OSC 23 ; 1 → 恢复图标标题
                // OSC 23 ; 2 → 恢复窗口标题
                self.state.pop_title(opcode);
            }
            "52" => {
                // OSC 52 ; selection ; base64_data → 剪贴板操作
                // 需要 Java 层处理，这里只报告
                if params.len() > 2 {
                    if let Ok(base64_data) = std::str::from_utf8(params[2]) {
                        self.state.handle_osc52(base64_data);
                    }
                }
            }
            "104" => {
                // OSC 104 ; c → 重置颜色索引 c
                // OSC 104 → 重置所有颜色
                self.state.handle_osc104(&param_text);
            }
            "110" => {
                // OSC 110 → 重置默认前景色
                self.state
                    .colors
                    .reset_index(COLOR_INDEX_FOREGROUND as usize);
                self.state.report_colors_changed();
            }
            "111" => {
                // OSC 111 → 重置默认背景色
                self.state
                    .colors
                    .reset_index(COLOR_INDEX_BACKGROUND as usize);
                self.state.report_colors_changed();
            }
            "112" => {
                // OSC 112 → 重置光标颜色
                self.state.colors.reset_index(COLOR_INDEX_CURSOR as usize);
                self.state.report_colors_changed();
            }
            _ => {
                // 未知 OSC 序列，忽略
            }
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // 提前提取所需信息，释放对 self 的借用
        let is_private = intermediates.contains(&b'?');
        let is_bang = intermediates.contains(&b'!');

        match action {
            '@' => {
                // ICH - 插入字符
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.insert_characters(*n as i32);
            }
            'A' => {
                // CUU - 光标上移
                let dist = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_y =
                    max(self.state.top_margin, self.state.cursor_y - *dist as i32);
                self.state.about_to_wrap = false;
            }
            'B' => {
                // CUD - 光标下移
                let dist = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_y = min(
                    self.state.bottom_margin - 1,
                    self.state.cursor_y + *dist as i32,
                );
                self.state.about_to_wrap = false;
            }
            'C' | 'a' => {
                // CUF/HPR - 光标右移/水平相对
                let dist = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_horizontal_relative(*dist as i32);
            }
            'D' => {
                // CUB - 光标左移
                let dist = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_x =
                    max(self.state.left_margin, self.state.cursor_x - *dist as i32);
                self.state.about_to_wrap = false;
            }
            'E' => {
                // CNL - 下一行
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_next_line(*n as i32);
            }
            'F' => {
                // CPL - 上一行
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_previous_line(*n as i32);
            }
            'G' => {
                // CHA - 光标水平绝对
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_horizontal_absolute(*n as i32);
            }
            'H' | 'f' => {
                // CUP/HVP - 光标定位
                let mut iter = params.iter();
                let row = iter.next().and_then(|p| p.first()).unwrap_or(&1);
                let col = iter.next().and_then(|p| p.first()).unwrap_or(&1);
                // 考虑原点模式
                if self.state.origin_mode {
                    self.state.cursor_y = max(
                        self.state.top_margin,
                        min(
                            self.state.bottom_margin - 1,
                            self.state.top_margin + *row as i32 - 1,
                        ),
                    );
                } else {
                    self.state.cursor_y = max(0, min(self.state.rows - 1, *row as i32 - 1));
                }
                self.state.cursor_x = max(
                    self.state.left_margin,
                    min(self.state.right_margin - 1, *col as i32 - 1),
                );
                self.state.about_to_wrap = false;
            }
            'I' => {
                // CHT - 光标前进制表
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                for _ in 0..*n {
                    self.state.cursor_forward_tab();
                }
            }
            'J' => {
                // ED - 清屏
                let mode = params.iter().next().and_then(|p| p.first()).unwrap_or(&0);
                self.state.erase_in_display(*mode as i32);
            }
            'K' => {
                // EL - 清线
                let mode = params.iter().next().and_then(|p| p.first()).unwrap_or(&0);
                self.state.erase_in_line(*mode as i32);
            }
            'L' => {
                // IL - 插入行
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.insert_lines(*n as i32);
            }
            'M' => {
                // DL - 删除行
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.delete_lines(*n as i32);
            }
            'P' => {
                // DCH - 删除字符
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.delete_characters(*n as i32);
            }
            'S' => {
                // SU - 上滚
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.scroll_up_lines(*n as i32);
            }
            'T' => {
                // SD - 下滚
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.scroll_down_lines(*n as i32);
            }
            'X' => {
                // ECH - 擦除字符
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.erase_characters(*n as i32);
            }
            'Z' => {
                // CBT - 光标后退制表
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_backward_tab(*n as i32);
            }
            '`' => {
                // HPA - 水平绝对
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_horizontal_absolute(*n as i32);
            }
            'b' => {
                // REP - 重复字符
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                if let Some(c) = self.last_printed_char {
                    self.state.repeat_character(*n as i32, c);
                }
            }
            'c' => {
                // DA - 设备属性
                self.state.report_terminal_response("\x1b[?6c");
            }
            'd' => {
                // VPA - 垂直绝对
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_vertical_absolute(*n as i32);
            }
            'e' => {
                // VPR - 垂直相对
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_vertical_relative(*n as i32);
            }
            'g' => {
                // TBC - 清除制表位
                let mode = params.iter().next().and_then(|p| p.first()).unwrap_or(&0);
                self.state.clear_tab_stop(*mode as i32);
            }
            'h' => {
                // SM - 设置模式
                if is_private {
                    self.state.handle_decset(params, true);
                } else {
                    self.state.handle_set_mode(params, true);
                }
            }
            'l' => {
                // RM - 重置模式
                if is_private {
                    self.state.handle_decset(params, false);
                } else {
                    self.state.handle_set_mode(params, false);
                }
            }
            'm' => {
                // SGR - 字符属性
                self.state.handle_sgr(params);
            }
            'n' => {
                // DSR - 设备状态报告
                let mode = params.iter().next().and_then(|p| p.first()).unwrap_or(&0);
                match mode {
                    5 => self.state.report_terminal_response("\x1b[0n"),
                    6 => {
                        let r = self.state.cursor_y + 1;
                        let c = self.state.cursor_x + 1;
                        self.state
                            .report_terminal_response(&format!("\x1b[{};{}R", r, c));
                    }
                    _ => {}
                }
            }
            'p' => {
                // 软重置: CSI ! p
                if is_bang {
                    self.state.decstr_soft_reset();
                }
            }
            'r' => {
                // DECSTBM - 设置上下边距
                let mut iter = params.iter();
                let top = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                let bottom = iter
                    .next()
                    .and_then(|p| p.first())
                    .copied()
                    .unwrap_or(self.state.rows as u16) as i32;
                self.state.set_margins(top, bottom);
            }
            's' => {
                if self.state.leftright_margin_mode {
                    let mut iter = params.iter();
                    let left = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
                    let right = iter
                        .next()
                        .and_then(|p| p.first())
                        .copied()
                        .unwrap_or(self.state.cols as u16) as i32;
                    self.state.set_left_right_margins(left, right);
                } else {
                    self.state.save_cursor();
                }
            }
            'u' => {
                self.state.restore_cursor();
            }
            _ => self.unhandled_sequences.push(format!("CSI {:?}", action)),
        }
    }

    // ========================================================================
    // DCS 序列处理 - Sixel 图形支持
    // ========================================================================

    fn hook(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // DCS 序列开始：DCS Pn1;Pn2;... Pn action
        if action == 'q' && intermediates.is_empty() {
            // DCS q - Sixel 图形
            self.state.sixel_decoder.start(params);
        }
    }

    fn put(&mut self, byte: u8) {
        // DCS 数据部分
        if self.state.sixel_decoder.state == SixelState::Data {
            // 收集 Sixel 数据
            let data = [byte];
            self.state.sixel_decoder.process_data(&data);
        }
    }

    fn unhook(&mut self) {
        // DCS 序列结束
        if self.state.sixel_decoder.state == SixelState::Data {
            self.state.sixel_decoder.finish();
            // 渲染 Sixel 图像到屏幕
            self.state.render_sixel_image();
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            // ESC # 8 - DECALN 屏幕对齐测试
            (&[b'#'], b'8') => {
                self.state.decaln_screen_align();
            }
            (&[b'#'], _) => {
                // 其他 ESC # 序列，忽略
            }
            // ESC ( 0 - 选择 G0 字符集（行绘图）
            (&[b'('], b'0') => {
                self.state.use_line_drawing_g0 = true;
                self.state.use_line_drawing_uses_g0 = true;
            }
            // ESC ( B - 选择 G0 字符集（ASCII）
            (&[b'('], b'B') => {
                self.state.use_line_drawing_g0 = false;
            }
            // ESC ) 0 - 选择 G1 字符集（行绘图）
            (&[b')'], b'0') => {
                self.state.use_line_drawing_g1 = true;
                self.state.use_line_drawing_uses_g0 = false;
            }
            // ESC ) B - 选择 G1 字符集（ASCII）
            (&[b')'], b'B') => {
                self.state.use_line_drawing_g1 = false;
            }
            (&[], b'6') => {
                // DECBI - Back Index (http://www.vt100.net/docs/vt510-rm/DECBI)
                // 向左移动光标，如果在左边界则向左滚动并插入空白列
                if self.state.cursor_x > self.state.left_margin {
                    self.state.cursor_x -= 1;
                } else {
                    // 向左滚动：将区域内所有列向右移动一列
                    self.state.back_index_scroll();
                }
            }
            (&[], b'7') => {
                // DECSC - 保存光标
                self.state.save_cursor();
            }
            (&[], b'8') => {
                // DECRC - 恢复光标
                self.state.restore_cursor();
            }
            (&[], b'9') => {
                // DECFI - Forward Index (http://www.vt100.net/docs/vt510-rm/DECFI)
                // 向右移动光标，如果在右边界则向右滚动并插入空白列
                if self.state.cursor_x < self.state.right_margin - 1 {
                    self.state.cursor_x += 1;
                } else {
                    // 向右滚动：将区域内所有列向左移动一列
                    self.state.forward_index_scroll();
                }
            }
            (&[], b'c') => {
                // RIS - 重置到初始状态 (http://vt100.net/docs/vt510-rm/RIS)
                // 完整重置：清屏、重置光标、重置样式、重置边距、重置制表位
                self.state.reset_to_initial_state();
            }
            (&[], b'D') => {
                // IND - 索引（换行）
                if self.state.cursor_y < self.state.bottom_margin - 1 {
                    self.state.cursor_y += 1;
                } else {
                    self.state.scroll_up();
                }
            }
            (&[], b'E') => {
                // NEL - 下一行
                if self.state.cursor_y < self.state.bottom_margin - 1 {
                    self.state.cursor_y += 1;
                    self.state.cursor_x = self.state.left_margin;
                } else {
                    self.state.scroll_up();
                    self.state.cursor_x = self.state.left_margin;
                }
            }
            (&[], b'F') => {
                // 光标到左下角
                self.state.cursor_x = self.state.left_margin;
                self.state.cursor_y = self.state.bottom_margin - 1;
            }
            (&[], b'H') => {
                // HTS - 设置制表位
                if self.state.cursor_x >= 0
                    && (self.state.cursor_x as usize) < self.state.tab_stops.len()
                {
                    self.state.tab_stops[self.state.cursor_x as usize] = true;
                }
            }
            (&[], b'M') => {
                // RI - 反向索引 (http://www.vt100.net/docs/vt100-ug/chapter3.html)
                // 将活动位置移动到上一行的相同水平位置
                // 如果活动位置在顶部边距，则执行向下滚动
                if self.state.cursor_y > self.state.top_margin {
                    self.state.cursor_y -= 1;
                } else {
                    // 向下滚动区域
                    self.state.reverse_index_scroll();
                }
            }
            (&[], b'N') => {
                // SS2 - 单移位 2，忽略
            }
            (&[], b'0') => {
                // SS3 - 单移位 3，忽略
            }
            (&[], b'P') => {
                // DCS - 设备控制字符串
                // DCS 序列由 vte 解析器处理，put_string 回调会被调用
                // 目前主要支持 DECSIXEL 图形，其他 DCS 序列忽略
            }
            (&[], b'=') => {
                // DECKPAM - 应用键盘模式
                self.state.application_keypad = true;
            }
            (&[], b'>') => {
                // DECKPNM - 数字键盘模式
                self.state.application_keypad = false;
            }
            (&[], b'[') => {
                // CSI - 由 vte 解析器处理
            }
            (&[], b']') => {
                // OSC - 由 vte 解析器处理，osc_dispatch 回调会被调用
            }
            (&[], b'_') => {
                // APC - 应用程序命令
                // 目前简单处理：忽略 APC 内容
                // APC 序列格式：ESC _ (数据) ESC \\ 或 ESC _ (数据) BEL
            }
            _ => self.unhandled_sequences.push(format!("ESC {:?}", byte)),
        }
    }
}

pub struct TerminalEngine {
    pub parser: Parser,
    pub state: ScreenState,
}

impl TerminalEngine {
    pub fn new(cols: i32, rows: i32, total_rows: i32, cell_width: i32, cell_height: i32) -> Self {
        Self {
            parser: Parser::new(),
            state: ScreenState::new(cols, rows, total_rows, cell_width, cell_height),
        }
    }

    pub fn process_bytes(&mut self, data: &[u8]) {
        let mut handler = PurePerformHandler {
            state: &mut self.state,
            unhandled_sequences: Vec::new(),
            last_printed_char: None,
        };
        self.parser.advance(&mut handler, data);
        self.state.clamp_cursor();
    }

    pub fn resize(&mut self, new_cols: i32, new_rows: i32) {
        self.state.resize(new_cols, new_rows);
    }
}
