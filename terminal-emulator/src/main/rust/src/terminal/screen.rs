use std::cmp::{max, min};
use crate::terminal::style::{STYLE_NORMAL};

#[derive(Clone)]
pub struct TerminalRow {
    pub text: Vec<char>,
    pub styles: Vec<u64>,
    pub line_wrap: bool,
}

impl TerminalRow {
    pub fn new(cols: usize) -> Self {
        Self {
            text: vec![' '; cols],
            styles: vec![STYLE_NORMAL; cols],
            line_wrap: false,
        }
    }

    pub fn clear(&mut self, start: usize, end: usize, style: u64) {
        let end = min(end, self.text.len());
        if start < end {
            for i in start..end {
                self.text[i] = ' ';
                self.styles[i] = style;
            }
        }
    }

    pub fn set_char(&mut self, column: usize, code_point: u32, style: u64) {
        if column < self.text.len() {
            self.text[column] = std::char::from_u32(code_point).unwrap_or(' ');
            self.styles[column] = style;
        }
    }

    pub fn insert_spaces(&mut self, column: usize, n: usize, style: u64) {
        let len = self.text.len();
        if column < len {
            let n = min(n, len - column);
            for i in (column + n..len).rev() {
                self.text[i] = self.text[i - n];
                self.styles[i] = self.styles[i - n];
            }
            for i in column..column + n {
                self.text[i] = ' ';
                self.styles[i] = style;
            }
        }
    }

    pub fn delete_characters(&mut self, column: usize, n: usize, style: u64) {
        let len = self.text.len();
        if column < len {
            let n = min(n, len - column);
            for i in column..len - n {
                self.text[i] = self.text[i + n];
                self.styles[i] = self.styles[i + n];
            }
            for i in len - n..len {
                self.text[i] = ' ';
                self.styles[i] = style;
            }
        }
    }

    pub fn get_space_used(&self) -> usize {
        for i in (0..self.text.len()).rev() {
            // 只要不是空字符或默认空格，就认为该列已被使用
            if self.text[i] != ' ' && self.text[i] != '\0' {
                return i + 1;
            }
        }
        0
    }

    pub fn copy_text(&self, start: usize, end: usize, dest: &mut [u16]) {
        let text_len = self.text.len();
        let end = min(end, text_len);
        let count = end.saturating_sub(start);
        for i in 0..min(count, dest.len()) {
            dest[i] = self.text[start + i] as u16;
        }
    }

    pub fn find_char_index_at_column(&self, column: usize) -> usize {
        let mut cur_col = 0;
        let mut cur_idx = 0;
        while cur_idx < self.text.len() {
            let c = self.text[cur_idx];
            let width = local_get_width(c as u32) as usize;
            if width > 0 {
                if cur_col >= column { return cur_idx; }
                cur_col += width;
            } else {
                if cur_col >= column { return cur_idx; }
            }
            cur_idx += 1;
        }
        self.get_space_used()
    }

    pub fn get_selected_text(&self, x1: usize, x2: usize) -> String {
        let cols = self.text.len();
        if x1 >= cols { return String::new(); }
        let end = min(x2, cols);
        self.text[x1..end].iter().collect()
    }

    pub fn get_word_at(&self, column: usize) -> String {
        let cols = self.text.len();
        if column >= cols { return String::new(); }
        fn is_word(c: char) -> bool { c.is_alphanumeric() || c == '_' }
        if !is_word(self.text[column]) { return String::new(); }
        let mut s = column; while s > 0 && is_word(self.text[s-1]) { s -= 1; }
        let mut e = column; while e + 1 < cols && is_word(self.text[e+1]) { e += 1; }
        self.text[s..=e].iter().collect()
    }
}

fn local_get_width(ucs: u32) -> usize {
    if ucs == 0 || ucs == 32 { return 1; } 
    if ucs < 32 || (ucs >= 0x7F && ucs < 0xA0) { return 0; }
    if (ucs >= 0x2E80 && ucs <= 0x9FFF) || (ucs >= 0xAC00 && ucs <= 0xD7A3) || (ucs >= 0xFF01 && ucs <= 0xFF60) { return 2; }
    1
}

pub struct Screen {
    pub rows: i32,
    pub cols: i32,
    pub buffer: Vec<TerminalRow>,
    pub first_row: usize,
    pub active_transcript_rows: usize,
}

impl Screen {
    pub fn new(cols: i32, rows: i32, total_rows: i32) -> Self {
        let t_u = max(rows as usize, total_rows as usize);
        let mut b = Vec::with_capacity(t_u);
        for _ in 0..t_u { b.push(TerminalRow::new(max(1, cols as usize))); }
        Self { rows, cols, buffer: b, first_row: 0, active_transcript_rows: 0 }
    }

    #[inline]
    pub fn internal_row(&self, row: i32) -> usize {
        let t = self.buffer.len() as i64;
        if t == 0 { return 0; }
        (((self.first_row as i64 + row as i64) % t + t) % t) as usize
    }

    pub fn get_row(&self, row: i32) -> &TerminalRow { &self.buffer[self.internal_row(row)] }
    pub fn get_row_mut(&mut self, row: i32) -> &mut TerminalRow { let idx = self.internal_row(row); &mut self.buffer[idx] }

    pub fn block_clear(&mut self, top: usize, left: usize, bottom: usize, right: usize, style: u64) {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        for row in top..min(bottom, rows) {
            self.get_row_mut(row as i32).clear(left, min(right, cols), style);
        }
    }

    pub fn get_transcript_text(&self) -> String {
        let mut res = String::new();
        let first_y = -(self.active_transcript_rows as i32);
        for y in first_y..self.rows {
            let row = self.get_row(y);
            res.push_str(&row.get_selected_text(0, row.get_space_used()));
            if !row.line_wrap && y < self.rows - 1 { res.push('\n'); }
        }
        res
    }

    pub fn get_selected_text(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> String {
        let mut res = String::new();
        let (sy, sx, ey, ex) = if y1 < y2 || (y1 == y2 && x1 <= x2) { (y1, x1, y2, x2) } else { (y2, x2, y1, x1) };
        for y in sy..=ey {
            let row = self.get_row(y);
            let s_x = if y == sy { max(0, sx) as usize } else { 0 };
            let e_x = if y == ey { min(self.cols, ex + 1) as usize } else { self.cols as usize };
            if s_x < e_x { res.push_str(&row.get_selected_text(s_x, e_x)); }
            if y < ey && !row.line_wrap { res.push('\n'); }
        }
        res
    }

    pub fn erase_in_display(&mut self, mode: i32, cursor_y: i32, style: u64) {
        let c = self.cols as usize;
        match mode {
            0 => { for y in (cursor_y + 1)..self.rows { self.get_row_mut(y).clear(0, c, style); } }
            1 => { for y in 0..cursor_y { self.get_row_mut(y).clear(0, c, style); } }
            2 => { for y in 0..self.rows { self.get_row_mut(y).clear(0, c, style); } }
            3 => {
                for y in 0..self.buffer.len() { self.buffer[y].clear(0, c, style); }
                self.first_row = 0; self.active_transcript_rows = 0;
            }
            _ => {}
        }
    }

    pub fn insert_lines(&mut self, cursor_y: i32, bottom: i32, n: i32, style: u64) {
        let c = self.cols as usize;
        let to_insert = min(n, bottom - cursor_y);
        let to_move = (bottom - cursor_y) - to_insert;
        for i in (0..to_move).rev() {
            let s = self.internal_row(cursor_y + i);
            let d = self.internal_row(cursor_y + i + to_insert);
            self.buffer[d] = self.buffer[s].clone();
        }
        for i in 0..to_insert { self.get_row_mut(cursor_y + i).clear(0, c, style); }
    }

    pub fn delete_lines(&mut self, cursor_y: i32, bottom: i32, n: i32, style: u64) {
        let c = self.cols as usize;
        let to_delete = min(n, bottom - cursor_y);
        let to_move = (bottom - cursor_y) - to_delete;
        for i in 0..to_move {
            let s = self.internal_row(cursor_y + i + to_delete);
            let d = self.internal_row(cursor_y + i);
            self.buffer[d] = self.buffer[s].clone();
        }
        for i in 0..to_delete { self.get_row_mut(bottom - i - 1).clear(0, c, style); }
    }

    pub fn scroll_up(&mut self, top: i32, bottom: i32, style: u64) {
        let c = self.cols as usize;
        if top == 0 && bottom == self.rows {
            self.first_row = (self.first_row + 1) % self.buffer.len();
            if self.active_transcript_rows < self.buffer.len() - self.rows as usize { self.active_transcript_rows += 1; }
            self.get_row_mut(self.rows - 1).clear(0, c, style);
        } else {
            for i in top..(bottom - 1) {
                let s = self.internal_row(i + 1);
                let d = self.internal_row(i);
                self.buffer[d] = self.buffer[s].clone();
            }
            self.get_row_mut(bottom - 1).clear(0, c, style);
        }
    }

    pub fn scroll_down(&mut self, top: i32, bottom: i32, style: u64) {
        let c = self.cols as usize;
        for i in (top + 1..bottom).rev() {
            let s = self.internal_row(i - 1);
            let d = self.internal_row(i);
            self.buffer[d] = self.buffer[s].clone();
        }
        self.get_row_mut(top).clear(0, c, style);
    }

    pub fn resize_with_reflow(&mut self, new_cols: i32, new_rows: i32, current_style: u64, cursor_x: i32, cursor_y: i32) -> (i32, i32) {
        let old_total = self.buffer.len();
        let n_cols = new_cols as usize;

        // 1. 提取所有逻辑行
        let mut logical_lines = Vec::new();
        let mut cur_text = Vec::new();
        let mut cur_styles = Vec::new();
        let mut cursor_logic_pos = None;

        let start_row = -(self.active_transcript_rows as i32);
        let end_row = self.rows as i32;

        for r_idx in start_row..end_row {
            let row = self.get_row(r_idx);
            let used = if row.line_wrap { self.cols as usize } else { row.get_space_used() };

            if r_idx == cursor_y {
                cursor_logic_pos = Some((logical_lines.len(), cur_text.len() + min(cursor_x as usize, self.cols as usize)));
            }

            for col in 0..used {
                cur_text.push(row.text[col]);
                cur_styles.push(row.styles[col]);
            }

            if !row.line_wrap {
                let bg = if !cur_styles.is_empty() { *cur_styles.last().unwrap() } else { current_style };
                logical_lines.push((cur_text, cur_styles, false, bg));
                cur_text = Vec::new(); cur_styles = Vec::new();
            }
        }
        if !cur_text.is_empty() || (cursor_logic_pos.is_some() && cursor_logic_pos.unwrap().0 == logical_lines.len()) {
            let last_bg = if cur_styles.is_empty() { current_style } else { *cur_styles.last().unwrap_or(&current_style) };
            logical_lines.push((cur_text, cur_styles, true, last_bg));
        }

        // 2. 切分重排
        let mut reflowed = Vec::new();
        let (mut new_cx, mut new_cy) = (0, 0);

        for (seq_idx, (text, styles, was_wrapped, bg_style)) in logical_lines.into_iter().enumerate() {
            if text.is_empty() {
                if let Some((cs, _)) = cursor_logic_pos { if cs == seq_idx { new_cx = 0; new_cy = reflowed.len() as i32; } }
                let mut row = TerminalRow::new(n_cols); for s in &mut row.styles { *s = bg_style; }
                reflowed.push(row); continue;
            }

            let mut offset = 0;
            while offset < text.len() {
                let mut new_row = TerminalRow::new(n_cols);
                for s in &mut new_row.styles { *s = bg_style; }
                let mut col = 0;
                while offset < text.len() && col < n_cols {
                    if let Some((cs, co)) = cursor_logic_pos { if cs == seq_idx && co == offset { new_cx = col as i32; new_cy = reflowed.len() as i32; } }
                    let c = text[offset]; let s = styles[offset]; let w = local_get_width(c as u32);
                    if col + w > n_cols { break; }
                    new_row.text[col] = c; new_row.styles[col] = s; col += 1;
                    if w == 2 { if col < n_cols { new_row.text[col] = '\0'; new_row.styles[col] = s; col += 1; } }
                    offset += 1;
                }
                if let Some((cs, co)) = cursor_logic_pos { if cs == seq_idx && co == offset && offset == text.len() { new_cx = col as i32; new_cy = reflowed.len() as i32; } }
                new_row.line_wrap = if offset < text.len() { true } else { was_wrapped };
                reflowed.push(new_row);
            }
        }

        // 3. 映射回物理缓冲区 (顶对齐)
        let total_reflowed = reflowed.len();
        let to_copy = min(total_reflowed, old_total);
        let start_in_reflowed = total_reflowed.saturating_sub(to_copy);
        
        let mut new_buffer = vec![TerminalRow::new(n_cols); old_total];
        for r in &mut new_buffer { for style in &mut r.styles { *style = current_style; } }
        for i in 0..to_copy {
            new_buffer[i] = reflowed[start_in_reflowed + i].clone();
        }

        self.buffer = new_buffer;
        self.cols = n_cols as i32;
        self.rows = new_rows;
        
        // 4. 重置状态：first_row 指向屏幕起始行
        self.active_transcript_rows = if total_reflowed > new_rows as usize {
            min(total_reflowed - new_rows as usize, old_total - new_rows as usize)
        } else {
            0
        };
        self.first_row = self.active_transcript_rows;

        let final_cy = new_cy - (total_reflowed.saturating_sub(new_rows as usize) as i32);
        (new_cx, final_cy)
    }
}
