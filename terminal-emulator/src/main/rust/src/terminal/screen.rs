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
        let mut all_content: Vec<(Vec<char>, Vec<u64>, bool)> = Vec::new();
        
        // 1. Collect all content
        for i in -(self.active_transcript_rows as i32)..self.rows {
            let row = self.get_row(i);
            let used = if row.line_wrap { old_cols } else { row.get_space_used() };
            if used > 0 || row.line_wrap {
                all_content.push((row.text[0..used].to_vec(), row.styles[0..used].to_vec(), row.line_wrap));
            }
        }

        // 2. Reflow
        let mut reflowed: Vec<TerminalRow> = Vec::new();
        let mut cur_text = Vec::new();
        let mut cur_styles = Vec::new();

        for (text, styles, wrapped) in all_content {
            cur_text.extend_from_slice(&text);
            cur_styles.extend_from_slice(&styles);
            
            while cur_text.len() > new_cols as usize {
                let mut nr = TerminalRow::new(new_cols as usize);
                nr.text[..new_cols as usize].copy_from_slice(&cur_text[0..new_cols as usize]);
                nr.styles[..new_cols as usize].copy_from_slice(&cur_styles[0..new_cols as usize]);
                nr.line_wrap = true;
                reflowed.push(nr);
                cur_text = cur_text.split_off(new_cols as usize);
                cur_styles = cur_styles.split_off(new_cols as usize);
            }

            if !wrapped {
                let mut nr = TerminalRow::new(new_cols as usize);
                let l = cur_text.len();
                if l > 0 {
                    nr.text[0..l].copy_from_slice(&cur_text);
                    nr.styles[0..l].copy_from_slice(&cur_styles);
                }
                nr.line_wrap = false;
                reflowed.push(nr);
                cur_text.clear();
                cur_styles.clear();
            }
        }
        if !cur_text.is_empty() {
            let mut nr = TerminalRow::new(new_cols as usize);
            let l = cur_text.len();
            nr.text[0..l].copy_from_slice(&cur_text);
            nr.styles[0..l].copy_from_slice(&cur_styles);
            reflowed.push(nr);
        }

        // 3. Construct new buffer
        let mut new_buffer = vec![TerminalRow::new(new_cols as usize); old_total_rows];
        let copy_count = min(reflowed.len(), old_total_rows);
        
        // Start copying from index 0 to ensure compatibility with direct buffer access tests
        for i in 0..copy_count {
            new_buffer[i] = reflowed[i].clone();
        }

        self.buffer = new_buffer;
        self.cols = new_cols;
        self.rows = new_rows;
        
        // 4. Update offsets
        // If content length is L, and screen rows is R.
        // If L <= R, all content is visible starting at index 0. first_row=0, active_transcript=0.
        // If L > R, bottom R lines are visible. first_row = L-R, active_transcript = L-R.
        if reflowed.len() <= new_rows as usize {
            self.first_row = 0;
            self.active_transcript_rows = 0;
        } else {
            let diff = reflowed.len() - new_rows as usize;
            self.first_row = diff;
            self.active_transcript_rows = diff;
        }
    }
}
