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

    pub fn find_char_index_at_column(&self, column: usize) -> usize {
        let mut current_column = 0;
        let mut current_char_index = 0;
        while current_char_index < self.text.len() {
            let c = self.text[current_char_index];
            let width = crate::utils::get_char_width(c as u32) as usize;
            if width > 0 {
                if current_column == column {
                    return current_char_index;
                } else if current_column > column {
                    return current_char_index;
                }
                current_column += width;
            } else {
                if current_column == column {
                    return current_char_index;
                } else if current_column > column {
                    return current_char_index;
                }
            }
            current_char_index += 1;
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
        let len = self.text.len();
        if column >= len { return String::new(); }
        let mut start = column;
        while start > 0 && self.text[start-1].is_alphanumeric() { start -= 1; }
        let mut end = column;
        while end < len && self.text[end].is_alphanumeric() { end += 1; }
        if start == end && !self.text[column].is_whitespace() { return self.text[column..column+1].iter().collect(); }
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
        let mut res = String::new();
        for y in min(y1, y2)..=max(y1, y2) {
            let row = self.get_row(y);
            let cur_x1 = if y == y1 { x1 as usize } else { 0 };
            let cur_x2 = if y == y2 { x2 as usize + 1 } else { self.cols as usize };
            res.push_str(&row.get_selected_text(cur_x1, cur_x2));
            if y < max(y1, y2) && !row.line_wrap { res.push('\n'); }
        }
        res
    }

    pub fn get_transcript_text(&self) -> String {
        let mut res = String::new();
        for y in -(self.active_transcript_rows as i32)..self.rows {
            let row = self.get_row(y);
            let used = row.get_space_used();
            if used > 0 { res.push_str(&row.text[0..used].iter().collect::<String>()); }
            if !row.line_wrap && y < self.rows - 1 { res.push('\n'); }
        }
        res
    }

    pub fn erase_in_display(&mut self, mode: i32, cursor_y: i32, style: u64) {
        let cols = self.cols as usize;
        match mode {
            0 => { for y in (cursor_y + 1)..self.rows { self.get_row_mut(y).clear(0, cols, style); } }
            1 => { for y in 0..cursor_y { self.get_row_mut(y).clear(0, cols, style); } }
            2 => { for y in 0..self.rows { self.get_row_mut(y).clear(0, cols, style); } }
            3 => { for row in &mut self.buffer { row.clear(0, cols, style); } self.first_row = 0; self.active_transcript_rows = 0; }
            _ => {}
        }
    }

    pub fn insert_lines(&mut self, cursor_y: i32, bottom_margin: i32, n: i32, style: u64) {
        let cols = self.cols as usize;
        let to_insert = min(n, bottom_margin - cursor_y);
        let to_move = (bottom_margin - cursor_y) - to_insert;
        for i in (0..to_move).rev() {
            let src = self.internal_row(cursor_y + i);
            let dest = self.internal_row(cursor_y + i + to_insert);
            self.buffer[dest] = self.buffer[src].clone();
        }
        for i in 0..to_insert { self.get_row_mut(cursor_y + i).clear(0, cols, style); }
    }

    pub fn delete_lines(&mut self, cursor_y: i32, bottom_margin: i32, n: i32, style: u64) {
        let cols = self.cols as usize;
        let to_delete = min(n, bottom_margin - cursor_y);
        let to_move = (bottom_margin - cursor_y) - to_delete;
        for i in 0..to_move {
            let src = self.internal_row(cursor_y + i + to_delete);
            let dest = self.internal_row(cursor_y + i);
            self.buffer[dest] = self.buffer[src].clone();
        }
        for i in 0..to_delete { self.get_row_mut(bottom_margin - i - 1).clear(0, cols, style); }
    }

    pub fn scroll_up(&mut self, top: i32, bottom: i32, style: u64) {
        let cols = self.cols as usize;
        if top == 0 && bottom == self.rows {
            self.first_row = (self.first_row + 1) % self.buffer.len();
            if self.active_transcript_rows < self.buffer.len() - self.rows as usize { self.active_transcript_rows += 1; }
            self.get_row_mut(self.rows - 1).clear(0, cols, style);
        } else {
            for i in top..(bottom - 1) {
                let src = self.internal_row(i + 1);
                let dest = self.internal_row(i);
                self.buffer[dest] = self.buffer[src].clone();
            }
            self.get_row_mut(bottom - 1).clear(0, cols, style);
        }
    }

    pub fn resize_with_reflow(&mut self, new_cols: i32, new_rows: i32) {
        if new_cols == self.cols && new_rows == self.rows { return; }
        let old_total = self.buffer.len();
        let old_cols = self.cols as usize;
        let n_cols = new_cols as usize;
        
        let mut content = Vec::new();
        for i in -(self.active_transcript_rows as i32)..self.rows {
            let row = self.get_row(i);
            let used = if row.line_wrap { old_cols } else { row.get_space_used() };
            content.push((row.text[0..used].to_vec(), row.styles[0..used].to_vec(), row.line_wrap));
        }

        let mut reflowed = Vec::new();
        let (mut cur_t, mut cur_s) = (Vec::new(), Vec::new());
        for (t, s, wrapped) in content {
            cur_t.extend_from_slice(&t); cur_s.extend_from_slice(&s);
            while cur_t.len() > n_cols {
                let mut nr = TerminalRow::new(n_cols);
                nr.text[..n_cols].copy_from_slice(&cur_t[0..n_cols]);
                nr.styles[..n_cols].copy_from_slice(&cur_s[0..n_cols]);
                nr.line_wrap = true;
                reflowed.push(nr);
                cur_t = cur_t.split_off(n_cols); cur_s = cur_s.split_off(n_cols);
            }
            if !wrapped {
                let mut nr = TerminalRow::new(n_cols);
                let l = cur_t.len();
                if l > 0 { nr.text[0..l].copy_from_slice(&cur_t); nr.styles[0..l].copy_from_slice(&cur_s); }
                nr.line_wrap = false;
                reflowed.push(nr);
                cur_t.clear(); cur_s.clear();
            }
        }
        if !cur_t.is_empty() {
            let mut nr = TerminalRow::new(n_cols);
            let l = cur_t.len();
            nr.text[0..l].copy_from_slice(&cur_t); nr.styles[0..l].copy_from_slice(&cur_s);
            reflowed.push(nr);
        }

        let mut new_buffer = vec![TerminalRow::new(n_cols); old_total];
        let to_copy = min(reflowed.len(), old_total);
        
        // 关键对齐：如果内容行数少于一屏，放在顶部。如果多于一屏，填满顶部并增加历史记录计数。
        for i in 0..to_copy { new_buffer[i] = reflowed[i].clone(); }
        
        self.buffer = new_buffer; self.cols = new_cols; self.rows = new_rows;
        self.first_row = 0;
        self.active_transcript_rows = to_copy.saturating_sub(new_rows as usize);
    }
}
