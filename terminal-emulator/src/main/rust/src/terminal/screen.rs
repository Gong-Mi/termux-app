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

    pub fn get_selected_text(&self, x1: usize, x2: usize) -> String {
        let cols = self.text.len();
        if x1 >= cols { return String::new(); }
        let end = min(x2, cols);
        self.text[x1..end].iter().collect()
    }

    // 识别单词边界的辅助函数
    fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/'
    }

    pub fn get_word_at(&self, column: usize) -> String {
        let len = self.text.len();
        if column >= len { return String::new(); }
        
        let mut start = column;
        while start > 0 && Self::is_word_char(self.text[start - 1]) {
            start -= 1;
        }
        
        let mut end = column;
        while end < len && Self::is_word_char(self.text[end]) {
            end += 1;
        }
        
        if start == end && !self.text[column].is_whitespace() {
            // 如果不是单词字符也不是空格，至少选中自己
            return self.text[column..column+1].iter().collect();
        }
        
        self.text[start..end].iter().collect()
    }
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
        let total_rows_u = max(rows as usize, total_rows as usize);
        let mut buffer = Vec::with_capacity(total_rows_u);
        for _ in 0..total_rows_u {
            buffer.push(TerminalRow::new(max(1, cols as usize)));
        }
        Self {
            rows,
            cols,
            buffer,
            first_row: 0,
            active_transcript_rows: 0,
        }
    }

    #[inline]
    pub fn internal_row(&self, row: i32) -> usize {
        let total = self.buffer.len();
        if total == 0 { return 0; }
        let first = self.first_row as i64;
        let r = row as i64;
        let t = total as i64;
        (((first + r) % t + t) % t) as usize
    }

    pub fn get_row(&self, row: i32) -> &TerminalRow {
        let idx = self.internal_row(row);
        &self.buffer[idx]
    }

    pub fn get_row_mut(&mut self, row: i32) -> &mut TerminalRow {
        let idx = self.internal_row(row);
        &mut self.buffer[idx]
    }

    pub fn get_selected_text(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> String {
        let mut result = String::new();
        let (y_start, y_end) = (min(y1, y2), max(y1, y2));
        
        for y in y_start..=y_end {
            let row = self.get_row(y);
            let cur_x1 = if y == y1 { x1 as usize } else { 0 };
            let cur_x2 = if y == y2 { x2 as usize + 1 } else { self.cols as usize };
            
            let line_text = row.get_selected_text(cur_x1, cur_x2);
            result.push_str(&line_text);
            
            if y < y_end && !row.line_wrap {
                result.push('\n');
            }
        }
        result
    }

    pub fn get_transcript_text(&self) -> String {
        let mut result = String::new();
        let start_y = -(self.active_transcript_rows as i32);
        for y in start_y..self.rows {
            let row = self.get_row(y);
            let used = row.get_space_used();
            if used > 0 {
                let line: String = row.text[0..used].iter().collect();
                result.push_str(&line);
            }
            if !row.line_wrap && y < self.rows - 1 {
                result.push('\n');
            }
        }
        result
    }

    pub fn erase_in_display(&mut self, mode: i32, cursor_y: i32, style: u64) {
        let cols = self.cols as usize;
        match mode {
            0 => { // From cursor to end
                for y in (cursor_y + 1)..self.rows { self.get_row_mut(y).clear(0, cols, style); }
            }
            1 => { // From start to cursor
                for y in 0..cursor_y { self.get_row_mut(y).clear(0, cols, style); }
            }
            2 => { // All
                for y in 0..self.rows { self.get_row_mut(y).clear(0, cols, style); }
            }
            3 => { // All + History
                for row in &mut self.buffer { row.clear(0, cols, style); }
                self.first_row = 0;
                self.active_transcript_rows = 0;
            }
            _ => {}
        }
    }

    pub fn insert_lines(&mut self, cursor_y: i32, bottom_margin: i32, n: i32, style: u64) {
        let cols = self.cols as usize;
        let lines_after = bottom_margin - cursor_y;
        let to_insert = min(n, lines_after);
        let to_move = lines_after - to_insert;
        for i in (0..to_move).rev() {
            let src_idx = self.internal_row(cursor_y + i);
            let dest_idx = self.internal_row(cursor_y + i + to_insert);
            self.buffer[dest_idx] = self.buffer[src_idx].clone();
        }
        for i in 0..to_insert { self.get_row_mut(cursor_y + i).clear(0, cols, style); }
    }

    pub fn delete_lines(&mut self, cursor_y: i32, bottom_margin: i32, n: i32, style: u64) {
        let cols = self.cols as usize;
        let lines_after = bottom_margin - cursor_y;
        let to_delete = min(n, lines_after);
        let to_move = lines_after - to_delete;
        for i in 0..to_move {
            let src_idx = self.internal_row(cursor_y + i + to_delete);
            let dest_idx = self.internal_row(cursor_y + i);
            self.buffer[dest_idx] = self.buffer[src_idx].clone();
        }
        for i in 0..to_delete { self.get_row_mut(bottom_margin - i - 1).clear(0, cols, style); }
    }

    pub fn scroll_up(&mut self, top: i32, bottom: i32, style: u64) {
        let cols = self.cols as usize;
        let rows = self.rows;
        if top == 0 && bottom == rows {
            self.first_row = (self.first_row + 1) % self.buffer.len();
            if self.active_transcript_rows < self.buffer.len() - rows as usize {
                self.active_transcript_rows += 1;
            }
            self.get_row_mut(rows - 1).clear(0, cols, style);
        } else {
            for i in top..(bottom - 1) {
                let src_idx = self.internal_row(i + 1);
                let dest_idx = self.internal_row(i);
                self.buffer[dest_idx] = self.buffer[src_idx].clone();
            }
            self.get_row_mut(bottom - 1).clear(0, cols, style);
        }
    }

    pub fn resize_with_reflow(&mut self, new_cols: i32, new_rows: i32) {
        if new_cols == self.cols && new_rows == self.rows { return; }
        
        let old_total_rows = self.buffer.len();
        let old_cols = self.cols as usize;
        let n_cols = new_cols as usize;
        
        let mut all_cells: Vec<(char, u64, bool)> = Vec::new();
        let mut last_sig_row = self.rows - 1;
        while last_sig_row > -(self.active_transcript_rows as i32) {
            let row = self.get_row(last_sig_row);
            if row.line_wrap || row.get_space_used() > 0 { break; }
            last_sig_row -= 1;
        }

        for i in -(self.active_transcript_rows as i32)..=last_sig_row {
            let row = self.get_row(i);
            let used = if row.line_wrap { old_cols } else { row.get_space_used() };
            let mut j = 0;
            while j < used {
                let c = row.text[j];
                let s = row.styles[j];
                let w = crate::utils::get_char_width(c as u32);
                all_cells.push((c, s, !row.line_wrap && j == used - 1));
                j += if w > 1 { w } else { 1 };
            }
            if used == 0 && !row.line_wrap {
                all_cells.push(('\n', STYLE_NORMAL, true));
            }
        }

        let mut reflowed_rows: Vec<TerminalRow> = Vec::new();
        let mut current_row = TerminalRow::new(n_cols);
        let mut cur_x = 0;

        for (c, s, is_hard_break) in all_cells {
            if c == '\n' && is_hard_break {
                reflowed_rows.push(current_row);
                current_row = TerminalRow::new(n_cols);
                cur_x = 0;
                continue;
            }

            let w = crate::utils::get_char_width(c as u32);
            let char_w = if w == 0 { 1 } else { w };

            if cur_x + char_w > n_cols {
                current_row.line_wrap = true;
                reflowed_rows.push(current_row);
                current_row = TerminalRow::new(n_cols);
                cur_x = 0;
            }
            
            if cur_x < n_cols {
                current_row.text[cur_x] = c;
                current_row.styles[cur_x] = s;
                if char_w == 2 && cur_x + 1 < n_cols {
                    current_row.text[cur_x + 1] = ' ';
                    current_row.styles[cur_x + 1] = s;
                }
                cur_x += char_w;
            }

            if is_hard_break {
                reflowed_rows.push(current_row);
                current_row = TerminalRow::new(n_cols);
                cur_x = 0;
            }
        }
        if cur_x > 0 {
            reflowed_rows.push(current_row);
        }

        let mut new_buffer = vec![TerminalRow::new(n_cols); old_total_rows];
        let content_len = reflowed_rows.len();
        let screen_rows = new_rows as usize;
        
        let to_copy = min(content_len, old_total_rows);
        let start_in_reflow = content_len - to_copy;
        
        for i in 0..to_copy {
            new_buffer[old_total_rows - to_copy + i] = reflowed_rows[start_in_reflow + i].clone();
        }

        self.buffer = new_buffer;
        self.cols = new_cols;
        self.rows = new_rows;
        
        if content_len < screen_rows {
            self.first_row = old_total_rows - to_copy;
            self.active_transcript_rows = 0;
        } else {
            self.first_row = old_total_rows - screen_rows;
            self.active_transcript_rows = to_copy - screen_rows;
        }
    }
}
