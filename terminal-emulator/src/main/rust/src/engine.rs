use jni::JNIEnv;
use jni::objects::{JObject, JValue};
use jni::sys::jobject;
use std::cmp::{max, min};
use unicode_width::UnicodeWidthChar;
use vte::{Params, Parser, Perform};

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
pub const STYLE_MASK_BG: u64 = 0x1FF0000;           // 位 16-24 (9 位索引色背景)
pub const STYLE_MASK_FG: u64 = 0x1FF0000000000;     // 位 40-48 (9 位索引色前景)

// 真彩色标志位（公开供测试使用）
pub const STYLE_TRUECOLOR_FG: u64 = 1 << 9;         // 位 9 - 前景色使用 24 位真彩色
pub const STYLE_TRUECOLOR_BG: u64 = 1 << 10;        // 位 10 - 背景色使用 24 位真彩色

// 效果标志（与 Java TextStyle 完全一致）
pub const EFFECT_BOLD: u64 = 1 << 0;                // 位 0 - 粗体
pub const EFFECT_ITALIC: u64 = 1 << 1;              // 位 1 - 斜体
pub const EFFECT_UNDERLINE: u64 = 1 << 2;           // 位 2 - 下划线
pub const EFFECT_BLINK: u64 = 1 << 3;               // 位 3 - 闪烁
pub const EFFECT_REVERSE: u64 = 1 << 4;             // 位 4 - 反显
pub const EFFECT_INVISIBLE: u64 = 1 << 5;           // 位 5 - 隐藏
pub const EFFECT_STRIKETHROUGH: u64 = 1 << 6;       // 位 6 - 删除线
pub const EFFECT_PROTECTED: u64 = 1 << 7;           // 位 7 - 保护属性
pub const EFFECT_DIM: u64 = 1 << 8;                 // 位 8 - 淡色/半亮度

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
    let mut result = effect & STYLE_MASK_EFFECT;
    
    // 处理前景色
    if (fore_color & 0xff000000) == 0xff000000 {
        // 24 位真彩色
        result |= STYLE_TRUECOLOR_FG | ((fore_color & 0x00ffffff) << 40);
    } else {
        // 索引色（9 位）
        result |= (fore_color & 0x1FF) << 40;
    }
    
    // 处理背景色
    if (back_color & 0xff000000) == 0xff000000 {
        // 24 位真彩色
        result |= STYLE_TRUECOLOR_BG | ((back_color & 0x00ffffff) << 16);
    } else {
        // 索引色（9 位）
        result |= (back_color & 0x1FF) << 16;
    }
    
    result
}

/// 默认样式（与 Java TextStyle.NORMAL 一致）
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
    pub leftright_margin_mode: bool, // DECSET 69 - DECLRMM 左右边距模式
    pub send_focus_events: bool,     // DECSET 1004 - 发送焦点事件

    // DECSET 标志位（用于保存/恢复）
    pub decset_flags: i32,
    pub saved_decset_flags: i32, // 保存的光标 DECSET 标志

    // 制表位
    pub tab_stops: Vec<bool>,

    // 循环缓冲区核心
    pub buffer: Vec<TerminalRow>,
    pub screen_first_row: usize, // 逻辑第 0 行在物理 buffer 中的索引

    // Java 回调支持
    pub java_callback_env: Option<*mut jni::sys::JNIEnv>,
    pub java_callback_obj: Option<jobject>,
}

impl ScreenState {
    pub fn new(cols: i32, rows: i32, total_rows: i32) -> Self {
        let total_rows_u = max(rows as usize, total_rows as usize);
        let mut buffer = Vec::with_capacity(total_rows_u);
        for _ in 0..total_rows_u {
            buffer.push(TerminalRow::new(max(1, cols as usize)));
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
            saved_x: 0,
            saved_y: 0,
            saved_style: STYLE_NORMAL,
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
            leftright_margin_mode: false, // DECSET 69 - 默认禁用左右边距模式
            send_focus_events: false,     // DECSET 1004 - 默认不发送焦点事件
            decset_flags: 0,              // 初始 DECSET 标志为 0
            saved_decset_flags: 0,        // 保存的 DECSET 标志初始为 0
            tab_stops,
            buffer,
            screen_first_row: 0,
            java_callback_env: None,
            java_callback_obj: None,
        }
    }

    /// 将逻辑行号转换为物理数组索引
    #[inline]
    fn external_to_internal_row(&self, row: i32) -> usize {
        let total = self.buffer.len();
        (self.screen_first_row + row as usize) % total
    }

    /// 设置 Java 回调环境
    pub fn set_java_callback(&mut self, env: *mut jni::sys::JNIEnv, obj: jobject) {
        self.java_callback_env = Some(env);
        self.java_callback_obj = Some(obj);
    }

    /// 调用 Java 方法报告标题变更
    fn report_title_change(&self, title: &str) {
        if let (Some(env_ptr), Some(obj)) = (self.java_callback_env, self.java_callback_obj) {
            unsafe {
                if let Ok(mut env) = JNIEnv::from_raw(env_ptr)
                    && let Ok(java_title) = env.new_string(title) {
                        let _ = env.call_method(
                            JObject::from_raw(obj),
                            "reportTitleChange",
                            "(Ljava/lang/String;)V",
                            &[JValue::Object(&JObject::from_raw(java_title.as_raw()))],
                        );
                    }
            }
        }
    }

    /// 调用 Java 方法报告颜色变更
    fn report_colors_changed(&self) {
        if let (Some(env_ptr), Some(obj)) = (self.java_callback_env, self.java_callback_obj) {
            unsafe {
                if let Ok(mut env) = JNIEnv::from_raw(env_ptr) {
                    let _ =
                        env.call_method(JObject::from_raw(obj), "reportColorsChanged", "()V", &[]);
                }
            }
        }
    }

    /// 调用 Java 方法报告光标可见性变更
    fn report_cursor_visibility(&self, visible: bool) {
        if let (Some(env_ptr), Some(obj)) = (self.java_callback_env, self.java_callback_obj) {
            unsafe {
                if let Ok(mut env) = JNIEnv::from_raw(env_ptr) {
                    let _ = env.call_method(
                        JObject::from_raw(obj),
                        "reportCursorVisibility",
                        "(Z)V",
                        &[JValue::Bool(if visible { 1 } else { 0 })],
                    );
                }
            }
        }
    }

    pub fn clamp_cursor(&mut self) {
        self.cursor_x = max(0, min(self.cols - 1, self.cursor_x));
        self.cursor_y = max(0, min(self.rows - 1, self.cursor_y));
    }

    fn print(&mut self, c: char) {
        let char_width = c.width().unwrap_or(0) as i32;
        if char_width == 0 {
            return;
        }

        // 自动换行处理 - 当光标位置 + 字符宽度 >= 右边界时换行
        if self.cursor_x + char_width >= self.right_margin {
            if self.auto_wrap {
                self.cursor_x = self.left_margin;
                if self.cursor_y < self.bottom_margin - 1 {
                    self.cursor_y += 1;
                    if self.origin_mode {
                        self.cursor_y = self.top_margin;
                    }
                } else {
                    self.scroll_up();
                }
            } else {
                // 非自动换行模式，限制在右边界
                self.cursor_x = self.right_margin - char_width;
            }
        }

        // 插入模式处理
        if self.insert_mode && char_width == 1 {
            self.insert_character();
        }

        let y_internal = self.external_to_internal_row(self.cursor_y);
        let x = self.cursor_x as usize;

        let row = &mut self.buffer[y_internal];
        if (self.cursor_x as usize) < row.text.len() {
            row.text[x] = c;
            row.styles[x] = self.current_style;
            if char_width == 2 && x + 1 < row.text.len() {
                row.text[x + 1] = '\0'; // Placeholder for second half of wide char
                row.styles[x + 1] = self.current_style;
            }
            self.cursor_x += char_width;
        }
    }

    /// 插入模式：在光标位置插入空格
    fn insert_character(&mut self) {
        let y_internal = self.external_to_internal_row(self.cursor_y);
        let row = &mut self.buffer[y_internal];

        // 从右向左移动字符
        for i in ((self.cursor_x as usize + 1)..row.text.len()).rev() {
            row.text[i] = row.text[i - 1];
            row.styles[i] = row.styles[i - 1];
        }
        if (self.cursor_x as usize) < row.text.len() {
            row.text[self.cursor_x as usize] = ' ';
            row.styles[self.cursor_x as usize] = STYLE_NORMAL;
        }
    }

    fn execute_control(&mut self, byte: u8) -> bool {
        match byte {
            0x00 => true, // NUL - 忽略
            0x07 => true, // BEL - 响铃（目前忽略）
            0x08 => {
                self.cursor_x = max(self.left_margin, self.cursor_x - 1);
                true
            } // BS
            0x09 => {
                self.cursor_forward_tab();
                true
            } // HT
            0x0a..=0x0c => {
                // LF, VT, FF
                if self.cursor_y < self.bottom_margin - 1 {
                    self.cursor_y += 1;
                } else {
                    self.scroll_up();
                }
                true
            }
            0x0d => {
                self.cursor_x = self.left_margin;
                true
            } // CR
            0x0e => true, // SO - 忽略
            0x0f => true, // SI - 忽略
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

        if top == 0 && bottom == self.rows {
            // 全屏滚动：直接移动起始指针
            self.screen_first_row = (self.screen_first_row + 1) % self.buffer.len();
            // 清理新出现的那一行（逻辑最后一行）
            let last_row_internal = self.external_to_internal_row(self.rows - 1);
            self.buffer[last_row_internal].clear(0, self.cols as usize, self.current_style);
        } else {
            // 区域滚动：目前仍需物理拷贝数据，但在终端中较少见
            for i in top..(bottom - 1) {
                let src_idx = self.external_to_internal_row(i + 1);
                let dest_idx = self.external_to_internal_row(i);
                let src_row = self.buffer[src_idx].clone();
                self.buffer[dest_idx] = src_row;
            }
            let clear_idx = self.external_to_internal_row(bottom - 1);
            self.buffer[clear_idx].clear(0, self.cols as usize, self.current_style);
        }
    }

    fn erase_in_display(&mut self, mode: i32) {
        match mode {
            0 => {
                self.erase_in_line(0);
                for y in (self.cursor_y + 1)..self.rows {
                    let idx = self.external_to_internal_row(y);
                    self.buffer[idx].clear(0, self.cols as usize, self.current_style);
                }
            }
            1 => {
                self.erase_in_line(1);
                for y in 0..self.cursor_y {
                    let idx = self.external_to_internal_row(y);
                    self.buffer[idx].clear(0, self.cols as usize, self.current_style);
                }
            }
            2 | 3 => {
                for y in 0..self.rows {
                    let idx = self.external_to_internal_row(y);
                    self.buffer[idx].clear(0, self.cols as usize, self.current_style);
                }
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: i32) {
        let idx = self.external_to_internal_row(self.cursor_y);
        let row_len = self.buffer[idx].text.len();
        let x = min(
            self.cursor_x as usize,
            if row_len > 0 { row_len - 1 } else { 0 },
        );
        match mode {
            0 => self.buffer[idx].clear(x, row_len, self.current_style),
            1 => self.buffer[idx].clear(0, min(row_len, x + 1), self.current_style),
            2 => self.buffer[idx].clear(0, row_len, self.current_style),
            _ => {}
        }
    }

    /// 插入字符 (ICH) - CSI {N} @
    fn insert_characters(&mut self, n: i32) {
        let columns_after_cursor = self.right_margin - self.cursor_x;
        let spaces_to_insert = min(n, columns_after_cursor);

        let y_internal = self.external_to_internal_row(self.cursor_y);
        let row = &mut self.buffer[y_internal];

        // 在边界内移动字符
        let move_start = self.cursor_x as usize;
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
            row.styles[i] = self.current_style;
        }

        // ICH 后光标位置不变
    }

    /// 删除字符 (DCH) - CSI {N} P
    fn delete_characters(&mut self, n: i32) {
        let columns_after_cursor = self.right_margin - self.cursor_x;
        let cells_to_delete = min(n, columns_after_cursor);
        let cells_to_move = columns_after_cursor - cells_to_delete;

        let y_internal = self.external_to_internal_row(self.cursor_y);
        let row = &mut self.buffer[y_internal];

        // 从左向右移动字符
        for i in 0..cells_to_move as usize {
            let src = self.cursor_x as usize + i + cells_to_delete as usize;
            let dest = self.cursor_x as usize + i;
            if src < row.text.len() && dest < row.text.len() {
                row.text[dest] = row.text[src];
                row.styles[dest] = row.styles[src];
            }
        }

        // 清空右侧区域
        let clear_start = self.cursor_x as usize + cells_to_move as usize;
        for i in clear_start..min(self.right_margin as usize, row.text.len()) {
            row.text[i] = ' ';
            row.styles[i] = STYLE_NORMAL;
        }
    }

    /// 插入行 (IL) - CSI {N} L
    fn insert_lines(&mut self, n: i32) {
        let lines_after_cursor = self.bottom_margin - self.cursor_y;
        let lines_to_insert = min(n, lines_after_cursor);
        let lines_to_move = lines_after_cursor - lines_to_insert;

        // 从下向上移动行
        for i in (0..lines_to_move as usize).rev() {
            let src_row = self.cursor_y as usize + i;
            let dest_row = self.cursor_y as usize + i + lines_to_insert as usize;

            if dest_row < self.rows as usize {
                let src_idx = self.external_to_internal_row(src_row as i32);
                let dest_idx = self.external_to_internal_row(dest_row as i32);
                let src_data = self.buffer[src_idx].clone();
                self.buffer[dest_idx] = src_data;
            }
        }

        // 清空插入的区域
        for i in 0..lines_to_insert as usize {
            let clear_idx = self.external_to_internal_row(self.cursor_y + i as i32);
            self.buffer[clear_idx].clear(0, self.cols as usize, self.current_style);
        }
    }

    /// 删除行 (DL) - CSI {N} M
    fn delete_lines(&mut self, n: i32) {
        let lines_after_cursor = self.bottom_margin - self.cursor_y;
        let lines_to_delete = min(n, lines_after_cursor);
        let lines_to_move = lines_after_cursor - lines_to_delete;

        // 从上向下移动行
        for i in 0..lines_to_move as usize {
            let src_row = self.cursor_y as usize + i + lines_to_delete as usize;
            let dest_row = self.cursor_y as usize + i;

            let src_idx = self.external_to_internal_row(src_row as i32);
            let dest_idx = self.external_to_internal_row(dest_row as i32);
            let src_data = self.buffer[src_idx].clone();
            self.buffer[dest_idx] = src_data;
        }

        // 清空底部区域
        for i in 0..lines_to_delete as usize {
            let clear_idx =
                self.external_to_internal_row(self.bottom_margin - i as i32 - 1);
            self.buffer[clear_idx].clear(0, self.cols as usize, self.current_style);
        }
    }

    /// 擦除字符 (ECH) - CSI {N} X
    fn erase_characters(&mut self, n: i32) {
        let chars_to_erase = min(n, self.cols - self.cursor_x);
        let y_internal = self.external_to_internal_row(self.cursor_y);
        let row = &mut self.buffer[y_internal];

        let start = self.cursor_x as usize;
        let end = min(start + chars_to_erase as usize, row.text.len());
        row.clear(start, end, self.current_style);
    }

    /// 光标水平绝对 (CHA) - CSI {N} G
    fn cursor_horizontal_absolute(&mut self, n: i32) {
        let col = max(1, n) - 1;
        self.cursor_x = min(max(0, col), self.cols - 1);
    }

    /// 光标水平相对 (HPR) - CSI {N} a
    fn cursor_horizontal_relative(&mut self, n: i32) {
        self.cursor_x = min(
            self.right_margin - 1,
            max(self.left_margin, self.cursor_x + n),
        );
    }

    /// 下一行 (CNL) - CSI {N} E
    fn cursor_next_line(&mut self, n: i32) {
        self.cursor_x = self.left_margin;
        self.cursor_y = min(self.bottom_margin - 1, self.cursor_y + n);
    }

    /// 上一行 (CPL) - CSI {N} F
    fn cursor_previous_line(&mut self, n: i32) {
        self.cursor_x = self.left_margin;
        self.cursor_y = max(self.top_margin, self.cursor_y - n);
    }

    /// 垂直绝对 (VPA) - CSI {N} d
    fn cursor_vertical_absolute(&mut self, n: i32) {
        let row = max(1, n) - 1;
        self.cursor_y = min(max(0, row), self.rows - 1);
    }

    /// 垂直相对 (VPR) - CSI {N} e
    fn cursor_vertical_relative(&mut self, n: i32) {
        self.cursor_y = min(self.rows - 1, max(0, self.cursor_y + n));
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
    }

    /// 下滚 (SD) - CSI {N} T
    fn scroll_down_lines(&mut self, n: i32) {
        let lines_to_scroll = min(n, self.bottom_margin - self.top_margin);

        // 从上向下移动行
        for i in (0..(self.bottom_margin - self.top_margin - lines_to_scroll) as usize).rev() {
            let src_row = self.top_margin as usize + i;
            let dest_row = self.top_margin as usize + i + lines_to_scroll as usize;

            if dest_row < self.rows as usize {
                let src_idx = self.external_to_internal_row(src_row as i32);
                let dest_idx = self.external_to_internal_row(dest_row as i32);
                let src_data = self.buffer[src_idx].clone();
                self.buffer[dest_idx] = src_data;
            }
        }

        // 清空顶部区域
        for i in 0..lines_to_scroll as usize {
            let clear_idx = self.external_to_internal_row(self.top_margin + i as i32);
            self.buffer[clear_idx].clear(0, self.cols as usize, self.current_style);
        }

        // 滚动后光标保持在顶部
        self.cursor_x = self.left_margin;
        self.cursor_y = self.top_margin;
    }

    /// DECBI - Back Index 滚动 (ESC 6)
    /// 当光标在左边界时，向左滚动并插入空白列
    fn back_index_scroll(&mut self) {
        // 向左滚动：将区域内所有列向右移动一列
        for y in self.top_margin..self.bottom_margin {
            let idx = self.external_to_internal_row(y);
            let row = &mut self.buffer[idx];
            
            // 从右向左移动字符
            for x in (1..self.cols as usize).rev() {
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
        // 向右滚动：将区域内所有列向左移动一列
        for y in self.top_margin..self.bottom_margin {
            let idx = self.external_to_internal_row(y);
            let row = &mut self.buffer[idx];
            
            // 从左向右移动字符
            for x in 0..(self.cols as usize - 1) {
                if x < row.text.len() && x + 1 < row.text.len() {
                    row.text[x] = row.text[x + 1];
                    row.styles[x] = row.styles[x + 1];
                }
            }
            // 最后一列填充空格
            let last_col = (self.cols as usize - 1).min(row.text.len().saturating_sub(1));
            if row.text.len() > last_col {
                row.text[last_col] = ' ';
                row.styles[last_col] = STYLE_NORMAL;
            }
        }
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
    fn handle_sgr(&mut self, params: &Params) {
        let params_vec: Vec<u16> = params.iter().flat_map(|p| p.iter().copied()).collect();
        let mut i = 0;

        // 如果没有参数，默认为重置
        if params_vec.is_empty() {
            self.current_style = STYLE_NORMAL;
            return;
        }

        while i < params_vec.len() {
            let code = params_vec[i];
            match code {
                0 => self.current_style = STYLE_NORMAL,                  // 重置
                1 => self.current_style |= EFFECT_BOLD,                  // 粗体
                2 => self.current_style |= EFFECT_DIM,                   // 淡色
                3 => self.current_style |= EFFECT_ITALIC,                // 斜体
                4 => {
                    // 下划线（支持子参数）
                    if i + 1 < params_vec.len() && params_vec.get(i + 1) == Some(&0) {
                        // 子参数 0 表示无下划线
                        self.current_style &= !EFFECT_UNDERLINE;
                        i += 1;
                    } else {
                        self.current_style |= EFFECT_UNDERLINE;
                    }
                }
                5 => self.current_style |= EFFECT_BLINK,                 // 闪烁
                7 => self.current_style |= EFFECT_REVERSE,               // 反显
                8 => self.current_style |= EFFECT_INVISIBLE,             // 隐藏
                9 => self.current_style |= EFFECT_STRIKETHROUGH,         // 删除线
                21 => self.current_style |= EFFECT_BOLD,                 // 双粗体（视为粗体）
                22 => self.current_style &= !(EFFECT_BOLD | EFFECT_DIM), // 正常强度
                23 => self.current_style &= !EFFECT_ITALIC,              // 非斜体
                24 => self.current_style &= !EFFECT_UNDERLINE,           // 非下划线
                25 => self.current_style &= !EFFECT_BLINK,               // 非闪烁
                27 => self.current_style &= !EFFECT_REVERSE,             // 非反显
                28 => self.current_style &= !EFFECT_INVISIBLE,           // 非隐藏
                29 => self.current_style &= !EFFECT_STRIKETHROUGH,       // 非删除线
                30..=37 => {
                    // 前景色 30-37（标准颜色 0-7）
                    self.current_style =
                        (self.current_style & !STYLE_MASK_FG) | ((code as u64 - 30) << 40);
                }
                38 => {
                    // 扩展前景色 (38;5;n 或 38;2;r;g;b)
                    if i + 1 < params_vec.len() {
                        let mode = params_vec[i + 1];
                        if mode == 5 && i + 2 < params_vec.len() {
                            // 256 色索引
                            let color = params_vec[i + 2];
                            self.current_style = (self.current_style & !STYLE_MASK_FG)
                                | ((color as u64 & 0x1FF) << 40);
                            i += 2;  // 跳过 mode 和 color
                        } else if mode == 2 && i + 4 < params_vec.len() {
                            // 24 位真彩色 (38;2;R;G;B)
                            let r = params_vec[i + 2] as u64;
                            let g = params_vec[i + 3] as u64;
                            let b = params_vec[i + 4] as u64;
                            let truecolor = 0xff000000 | (r << 16) | (g << 8) | b;
                            self.current_style = (self.current_style & !STYLE_MASK_FG)
                                | STYLE_TRUECOLOR_FG
                                | ((truecolor & 0x00ffffff) << 40);
                            i += 4;  // 跳过 mode, r, g, b (i+=1 在循环末尾)
                        }
                    }
                }
                39 => {
                    // 默认前景色
                    self.current_style = (self.current_style & !STYLE_MASK_FG)
                        | (COLOR_INDEX_FOREGROUND << 40);
                }
                40..=47 => {
                    // 背景色 40-47（标准颜色 0-7）
                    self.current_style =
                        (self.current_style & !STYLE_MASK_BG) | ((code as u64 - 40) << 16);
                }
                48 => {
                    // 扩展背景色 (48;5;n 或 48;2;r;g;b)
                    if i + 1 < params_vec.len() {
                        let mode = params_vec[i + 1];
                        if mode == 5 && i + 2 < params_vec.len() {
                            // 256 色索引
                            let color = params_vec[i + 2];
                            self.current_style = (self.current_style & !STYLE_MASK_BG)
                                | ((color as u64 & 0x1FF) << 16);
                            i += 2;  // 跳过 mode 和 color
                        } else if mode == 2 && i + 4 < params_vec.len() {
                            // 24 位真彩色 (48;2;R;G;B)
                            let r = params_vec[i + 2] as u64;
                            let g = params_vec[i + 3] as u64;
                            let b = params_vec[i + 4] as u64;
                            let truecolor = 0xff000000 | (r << 16) | (g << 8) | b;
                            self.current_style = (self.current_style & !STYLE_MASK_BG)
                                | STYLE_TRUECOLOR_BG
                                | ((truecolor & 0x00ffffff) << 16);
                            i += 4;  // 跳过 mode, r, g, b (i+=1 在循环末尾)
                        }
                    }
                }
                49 => {
                    // 默认背景色
                    self.current_style = (self.current_style & !STYLE_MASK_BG)
                        | (COLOR_INDEX_BACKGROUND << 16);
                }
                58 => {
                    // 下划线颜色 (58;5;n 或 58;2;r;g;b)
                    // 注意：目前只解析，实际渲染时需要额外存储下划线颜色
                    if i + 1 < params_vec.len() {
                        let mode = params_vec[i + 1];
                        if mode == 5 && i + 2 < params_vec.len() {
                            // 256 色索引 - 目前存储在前景色位置作为临时方案
                            i += 2;
                        } else if mode == 2 && i + 4 < params_vec.len() {
                            // 24 位真彩色
                            i += 4;
                        }
                    }
                }
                59 => {
                    // 默认下划线颜色
                }
                90..=97 => {
                    // 亮色前景色 90-97（高亮颜色 8-15）
                    self.current_style =
                        (self.current_style & !STYLE_MASK_FG) | ((code as u64 - 90 + 8) << 40);
                }
                100..=107 => {
                    // 亮色背景色 100-107（高亮颜色 8-15）
                    self.current_style =
                        (self.current_style & !STYLE_MASK_BG) | ((code as u64 - 100 + 8) << 16);
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
    fn save_cursor(&mut self) {
        self.saved_x = self.cursor_x;
        self.saved_y = self.cursor_y;
        self.saved_style = self.current_style;
        // 保存 DECSET 标志（与 Java 端一致，只保存相关标志）
        // 包括：AUTOWRAP, ORIGIN_MODE
        let mask = DECSET_BIT_AUTOWRAP | DECSET_BIT_ORIGIN_MODE;
        self.saved_decset_flags = self.decset_flags & mask;
    }

    /// 恢复光标 (DECRC)
    fn restore_cursor(&mut self) {
        self.cursor_x = self.saved_x;
        self.cursor_y = self.saved_y;
        self.current_style = self.saved_style;
        // 恢复 DECSET 标志（只恢复 AUTOWRAP 和 ORIGIN_MODE）
        let mask = DECSET_BIT_AUTOWRAP | DECSET_BIT_ORIGIN_MODE;
        self.decset_flags = (self.decset_flags & !mask) | (self.saved_decset_flags & mask);
        self.auto_wrap = (self.decset_flags & DECSET_BIT_AUTOWRAP) != 0;
        self.origin_mode = (self.decset_flags & DECSET_BIT_ORIGIN_MODE) != 0;
    }

    pub fn copy_row_text(&self, row: usize, dest: &mut [u16]) {
        let idx = self.external_to_internal_row(row as i32);
        let src = &self.buffer[idx].text;
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
        let src = &self.buffer[idx].styles;
        for i in 0..min(src.len(), dest.len()) {
            dest[i] = src[i] as i64;
        }
    }

    pub fn resize(&mut self, new_cols: i32, new_rows: i32) {
        // Resize 时需将循环缓冲区物理展开，否则数据会错乱
        let mut new_buffer = Vec::with_capacity(max(new_rows as usize, self.buffer.len()));
        for y in 0..self.rows {
            let old_idx = self.external_to_internal_row(y);
            let mut row = self.buffer[old_idx].clone();
            row.text.resize(new_cols as usize, ' ');
            row.styles.resize(new_cols as usize, 0);
            new_buffer.push(row);
        }

        // 补齐新行
        while new_buffer.len() < new_rows as usize {
            new_buffer.push(TerminalRow::new(new_cols as usize));
        }

        self.buffer = new_buffer;
        self.screen_first_row = 0;
        self.cols = new_cols;
        self.rows = new_rows;
        self.bottom_margin = new_rows;
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

        match opcode {
            "0" | "2" => {
                // 设置窗口标题
                if params.len() > 1 {
                    let title = std::str::from_utf8(params[1]).unwrap_or("");
                    self.state.report_title_change(title);
                }
            }
            "4" => {
                // 设置颜色
                self.state.report_colors_changed();
            }
            "10" | "11" | "12" => {
                // 设置前景色/背景色/光标色
                self.state.report_colors_changed();
            }
            "52" => { // 剪贴板操作
                // 需要 Java 层处理
            }
            "104" | "110" | "111" | "112" => {
                // 重置颜色
                self.state.report_colors_changed();
            }
            _ => {
                // 未知 OSC 序列
            }
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        #[allow(clippy::get_first)]
        // 检查是否是私有 CSI 序列（intermediates 包含 '?'）
        let is_private = intermediates.contains(&b'?');

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
            }
            'B' => {
                // CUD - 光标下移
                let dist = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                self.state.cursor_y = min(
                    self.state.bottom_margin - 1,
                    self.state.cursor_y + *dist as i32,
                );
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
                // ED 后光标位置不变
            }
            'K' => {
                // EL - 清线
                let mode = params.iter().next().and_then(|p| p.first()).unwrap_or(&0);
                self.state.erase_in_line(*mode as i32);
                // EL 后光标位置不变
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
                // ECH 后光标位置不变
            }
            'Z' => {
                // CBT - 光标后退制表
                let n = params.iter().next().and_then(|p| p.first()).unwrap_or(&1);
                println!("CBT called with n={}", n);
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
            'c' => { // DA - 设备属性
                // 忽略，由 Java 层处理
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
                    // DECSET - 私有模式设置
                    self.state.handle_decset(params, true);
                } else {
                    // 标准模式设置
                    self.state.handle_set_mode(params, true);
                }
            }
            'l' => {
                // RM - 重置模式
                if is_private {
                    // DECRST - 私有模式重置
                    self.state.handle_decset(params, false);
                } else {
                    // 标准模式重置
                    self.state.handle_set_mode(params, false);
                }
            }
            'm' => {
                // SGR - 字符属性
                self.state.handle_sgr(params);
            }
            'n' => { // DSR - 设备状态报告
                // 忽略，由 Java 层处理
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
                // DECSC - 保存光标 或 DECSLRM - 设置左右边距
                // 当 DECLRMM 启用时，DECSLRM 优先
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
                    // 否则保存光标
                    self.state.save_cursor();
                }
            }
            'u' => {
                // DECRC - 恢复光标
                self.state.restore_cursor();
            }
            _ => self.unhandled_sequences.push(format!("CSI {:?}", action)),
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'#' => {
                // ESC # - 暂时忽略，由 Java 层处理
            }
            b'(' => {
                // ESC ( - 设计 G0 字符集（行绘图）
                // 由 Java 层处理
            }
            b')' => {
                // ESC ) - 设计 G1 字符集（行绘图）
                // 由 Java 层处理
            }
            b'6' => {
                // DECBI - Back Index (http://www.vt100.net/docs/vt510-rm/DECBI)
                // 向左移动光标，如果在左边界则向左滚动并插入空白列
                if self.state.cursor_x > self.state.left_margin {
                    self.state.cursor_x -= 1;
                } else {
                    // 向左滚动：将区域内所有列向右移动一列
                    self.state.back_index_scroll();
                }
            }
            b'7' => {
                // DECSC - 保存光标
                self.state.save_cursor();
            }
            b'8' => {
                // DECRC - 恢复光标
                self.state.restore_cursor();
            }
            b'9' => {
                // DECFI - Forward Index (http://www.vt100.net/docs/vt510-rm/DECFI)
                // 向右移动光标，如果在右边界则向右滚动并插入空白列
                if self.state.cursor_x < self.state.right_margin - 1 {
                    self.state.cursor_x += 1;
                } else {
                    // 向右滚动：将区域内所有列向左移动一列
                    self.state.forward_index_scroll();
                }
            }
            b'c' => {
                // RIS - 重置到初始状态 (http://vt100.net/docs/vt510-rm/RIS)
                // 完整重置：清屏、重置光标、重置样式、重置边距、重置制表位
                self.state.cursor_x = 0;
                self.state.cursor_y = 0;
                self.state.current_style = STYLE_NORMAL;
                // 清屏
                for y in 0..self.state.rows as usize {
                    let idx = self.state.external_to_internal_row(y as i32);
                    self.state.buffer[idx].clear(0, self.state.cols as usize, STYLE_NORMAL);
                }
                // 重置所有制表位
                for stop in &mut self.state.tab_stops {
                    *stop = false;
                }
                // 重置边距
                self.state.top_margin = 0;
                self.state.bottom_margin = self.state.rows;
                self.state.left_margin = 0;
                self.state.right_margin = self.state.cols;
                // 重置 DECSET 标志
                self.state.decset_flags = 0;
                self.state.auto_wrap = true;
                self.state.origin_mode = false;
                self.state.cursor_enabled = true;
                self.state.application_cursor_keys = false;
                self.state.application_keypad = false;
                self.state.reverse_video = false;
                self.state.insert_mode = false;
                self.state.bracketed_paste = false;
                self.state.mouse_tracking = false;
                self.state.mouse_button_event = false;
                self.state.sgr_mouse = false;
                self.state.leftright_margin_mode = false;
                self.state.send_focus_events = false;
            }
            b'D' => {
                // IND - 索引（换行）
                if self.state.cursor_y < self.state.bottom_margin - 1 {
                    self.state.cursor_y += 1;
                } else {
                    self.state.scroll_up();
                }
            }
            b'E' => {
                // NEL - 下一行
                if self.state.cursor_y < self.state.bottom_margin - 1 {
                    self.state.cursor_y += 1;
                    self.state.cursor_x = self.state.left_margin;
                } else {
                    self.state.scroll_up();
                    self.state.cursor_x = self.state.left_margin;
                }
            }
            b'F' => {
                // 光标到左下角
                self.state.cursor_x = self.state.left_margin;
                self.state.cursor_y = self.state.bottom_margin - 1;
            }
            b'H' => {
                // HTS - 设置制表位
                if self.state.cursor_x >= 0
                    && (self.state.cursor_x as usize) < self.state.tab_stops.len()
                {
                    self.state.tab_stops[self.state.cursor_x as usize] = true;
                }
            }
            b'M' => {
                // RI - 反向索引
                if self.state.cursor_y > self.state.top_margin {
                    self.state.cursor_y -= 1;
                } else {
                    // 向下滚动区域（简化处理：暂时忽略）
                }
            }
            b'N' => {
                // SS2 - 单移位 2，忽略
            }
            b'0' => {
                // SS3 - 单移位 3，忽略
            }
            b'P' => {
                // DCS - 设备控制字符串，由 Java 层处理
            }
            b'=' => {
                // DECKPAM - 应用键盘模式
                self.state.application_keypad = true;
            }
            b'>' => {
                // DECKPNM - 数字键盘模式
                self.state.application_keypad = false;
            }
            b'[' => {
                // CSI - 由 vte 解析器处理
            }
            b']' => {
                // OSC - 由 Java 层处理
            }
            b'_' => {
                // APC - 应用程序命令，由 Java 层处理
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
    pub fn new(cols: i32, rows: i32, total_rows: i32) -> Self {
        Self {
            parser: Parser::new(),
            state: ScreenState::new(cols, rows, total_rows),
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
