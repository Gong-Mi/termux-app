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
        let len = self.text.len();
        let end = min(end, len);
        if start < end {
            for i in start..end {
                self.text[i] = ' ';
                self.styles[i] = style;
            }
        }
    }

    /// 清空整行，对齐官方 Java TerminalRow.clear() 方法
    pub fn clear_all(&mut self, style: u64) {
        for i in 0..self.text.len() {
            self.text[i] = ' ';
            self.styles[i] = style;
        }
        // 注意：Java 版本 clear() 不重置 line_wrap
        // 只重置 mSpaceUsed 和 mHasNonOneWidthOrSurrogateChars
        // Rust 版本没有这些字段，所以不需要额外操作
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
            // 修复：\0 是 CJK 宽字符的占位符，不应算作有效内容
            // 对齐官方 Java TerminalRow.isBlank() 的行为
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
        let _c = self.cols as usize;
        for i in (top + 1..bottom).rev() {
            let s = self.internal_row(i - 1);
            let d = self.internal_row(i);
            self.buffer[d] = self.buffer[s].clone();
        }
        self.get_row_mut(top).clear_all(style);
    }

    /// Resize with reflow, aligning with official Java TerminalBuffer.resize() logic.
    ///
    /// Key differences from previous implementation:
    /// - Uses `skipped_blank_lines` delay insertion mechanism like Java
    /// - Processes character by character with dynamic line wrapping
    /// - Properly handles cursor position tracking during reflow
    /// 
    /// ## Fast Path Optimization
    /// 
    /// When only rows change (columns unchanged) and new rows <= total rows,
    /// we use O(1) pointer adjustment instead of O(n) buffer rebuild.
    /// This matches Java's fast path behavior.
    pub fn resize_with_reflow(&mut self, new_cols: i32, new_rows: i32, current_style: u64, cursor_x: i32, cursor_y: i32) -> (i32, i32) {
        let old_cols = self.cols as usize;
        let old_total = self.buffer.len();

        // =====================================================================
        // Fast Path: Only rows changed (columns unchanged)
        // =====================================================================
        // This matches Java's fast path in TerminalBuffer.resize():
        // "if (newColumns == mColumns && newRows <= mTotalRows)"
        if new_cols as usize == old_cols && new_rows as usize <= old_total {
            return self.resize_rows_only(new_rows, cursor_x, cursor_y, current_style);
        }

        // =====================================================================
        // Slow Path: Columns changed or rows expanded - need full reflow
        // =====================================================================
        let n_cols = new_cols as usize;
        let old_cols = self.cols as usize;
        let old_rows = self.rows as usize;
        let old_active_transcript = self.active_transcript_rows;

        // Create new buffer with new column size
        let mut new_buffer: Vec<TerminalRow> = Vec::with_capacity(old_total);
        for _ in 0..old_total {
            let mut row = TerminalRow::new(n_cols);
            row.clear_all(current_style);
            new_buffer.push(row);
        }

        let mut new_cursor_x: i32 = 0;
        let mut new_cursor_y: i32 = 0;
        let mut cursor_placed = false;

        // Output position tracking - can go beyond new_rows into history area
        let mut output_row: usize = 0;
        let mut output_col: usize = 0;

        // Track skipped blank lines (Java logic)
        let mut skipped_blank_lines = 0;

        // Loop over every character in the initial state
        let start_row = -(old_active_transcript as i32);
        let end_row = old_rows as i32;

        for external_old_row in start_row..end_row {
            let internal_old_row = self.internal_row(external_old_row);
            let old_line = &self.buffer[internal_old_row];
            let cursor_at_this_row = external_old_row == cursor_y;

            // Check if line is blank (skip logic like Java)
            let is_blank = {
                let used = old_line.get_space_used();
                used == 0 || (0..used).all(|i| old_line.text[i] == ' ')
            };

            // Skip blank lines unless cursor is on this row
            if is_blank && !cursor_at_this_row {
                skipped_blank_lines += 1;
                continue;
            }

            // Insert skipped blank lines when encountering non-blank line
            if skipped_blank_lines > 0 {
                for _ in 0..skipped_blank_lines {
                    if output_row >= old_total - 1 {
                        // Buffer is full - need to scroll (shouldn't happen in normal cases)
                        // Scroll up: line 1→0, 2→1, etc., clear bottom
                        if cursor_placed && new_cursor_y > 0 {
                            new_cursor_y -= 1;
                        }
                        for i in 0..(old_total - 1) {
                            new_buffer[i] = new_buffer[i + 1].clone();
                        }
                        new_buffer[old_total - 1].clear_all(current_style);
                        output_row = old_total - 1;
                    } else {
                        output_row += 1;
                    }
                    output_col = 0;
                }
                skipped_blank_lines = 0;
            }

            // Determine how much of the line to process
            let last_non_space_index = if cursor_at_this_row || old_line.line_wrap {
                old_line.text.len()
            } else {
                old_line.get_space_used()
            };

            let just_to_cursor = cursor_at_this_row;

            // Process each character in the old line
            let mut current_old_col: usize = 0;
            let mut style_at_col = current_style;

            for i in 0..last_non_space_index {
                let c = old_line.text[i];
                let code_point = c as u32;
                let display_width = local_get_width(code_point);

                // Update style for this column
                if display_width > 0 && current_old_col < old_cols {
                    style_at_col = old_line.styles[current_old_col];
                }

                // Line wrap as necessary
                if output_col + display_width as usize > n_cols {
                    if output_row < new_buffer.len() {
                        new_buffer[output_row].line_wrap = true;
                    }
                    if output_row >= old_total - 1 {
                        // Buffer is full - need to scroll (shouldn't happen in normal cases)
                        if cursor_placed && new_cursor_y > 0 {
                            new_cursor_y -= 1;
                        }
                        for i in 0..(old_total - 1) {
                            new_buffer[i] = new_buffer[i + 1].clone();
                        }
                        new_buffer[old_total - 1].clear_all(current_style);
                        output_row = old_total - 1;
                    } else {
                        output_row += 1;
                    }
                    output_col = 0;
                }

                // Handle combining characters
                let offset = if display_width <= 0 && output_col > 0 { 1 } else { 0 };
                let output_column = output_col.saturating_sub(offset);

                // Set character in new buffer
                if output_column < n_cols && output_row < new_buffer.len() {
                    new_buffer[output_row].text[output_column] = c;
                    new_buffer[output_row].styles[output_column] = style_at_col;
                }

                // Track cursor position
                if cursor_at_this_row && current_old_col == cursor_x as usize && !cursor_placed {
                    new_cursor_x = output_col as i32;
                    new_cursor_y = output_row as i32;
                    cursor_placed = true;
                }

                if display_width > 0 {
                    current_old_col += display_width;
                    output_col += display_width as usize;

                    // Break if we've placed cursor and just copying to cursor
                    if just_to_cursor && cursor_placed {
                        break;
                    }
                }
            }

            // Check if we need to insert newline (line was not wrapping)
            if external_old_row != (end_row - 1) && !old_line.line_wrap {
                if output_row >= old_total - 1 {
                    // Buffer is full - need to scroll (shouldn't happen in normal cases)
                    if cursor_placed && new_cursor_y > 0 {
                        new_cursor_y -= 1;
                    }
                    for i in 0..(old_total - 1) {
                        new_buffer[i] = new_buffer[i + 1].clone();
                    }
                    new_buffer[old_total - 1].clear_all(current_style);
                    output_row = old_total - 1;
                } else {
                    output_row += 1;
                }
                output_col = 0;
            }
        }

        // Handle cursor scrolling off screen
        if !cursor_placed || new_cursor_x < 0 || new_cursor_y < 0 {
            new_cursor_x = 0;
            new_cursor_y = 0;
        }

        // Copy new_buffer to self.buffer
        self.buffer = new_buffer;
        self.cols = n_cols as i32;
        self.rows = new_rows;
        // Calculate active_transcript_rows:
        // output_row is 0-indexed, so output_row + 1 = total rows written
        // active_transcript_rows = rows written - visible rows (if positive)
        let total_written = output_row + 1;
        self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
        // Set first_row so that content is correctly mapped:
        // Content is written from new_buffer[0] to new_buffer[output_row]
        // History rows: 0 to active_transcript_rows-1 (in buffer)
        // Screen rows: active_transcript_rows to total_written-1 (in buffer)
        // We want internal_row(0) = active_transcript_rows (screen row 0 is at buffer[active_transcript_rows])
        // internal_row(0) = (first_row + 0) % total = first_row
        // So first_row = active_transcript_rows
        self.first_row = self.active_transcript_rows % self.buffer.len();

        (new_cursor_x, new_cursor_y)
    }

    /// Fast path resize: only rows change (columns unchanged)
    /// 
    /// This is O(1) pointer adjustment, matching Java's fast path behavior.
    /// 
    /// ## Parameters
    /// - `new_rows`: New number of visible rows
    /// - `cursor_x`, `cursor_y`: Current cursor position
    /// - `current_style`: Current text style for clearing blank lines
    /// 
    /// ## Returns
    /// - New cursor position (cursor_x, cursor_y)
    /// 
    /// ## Algorithm (matches Java TerminalBuffer.resize fast path)
    /// 1. Calculate `shift_down_of_top_row = old_rows - new_rows`
    /// 2. If shrinking (shift > 0), check if we can skip blank rows at bottom
    /// 3. If expanding (shift < 0), only move screen up if there's transcript
    /// 4. Adjust `first_row` pointer by shift amount
    /// 5. Update `active_transcript_rows` and cursor position
    fn resize_rows_only(&mut self, new_rows: i32, cursor_x: i32, cursor_y: i32, current_style: u64) -> (i32, i32) {
        let old_rows = self.rows as usize;
        
        // Calculate shift: positive = shrinking, negative = expanding
        let mut shift_down_of_top_row = old_rows as i32 - new_rows as i32;
        
        if shift_down_of_top_row > 0 && shift_down_of_top_row < old_rows as i32 {
            // Shrinking: check if we can skip blank rows at bottom below cursor
            for i in (1..old_rows).rev() {
                if cursor_y >= i as i32 {
                    break;
                }
                let internal_row = self.internal_row(i as i32);
                let row_is_blank = {
                    let line = &self.buffer[internal_row];
                    let used = line.get_space_used();
                    used == 0 || (0..used).all(|j| line.text[j] == ' ')
                };
                if row_is_blank {
                    shift_down_of_top_row -= 1;
                    if shift_down_of_top_row == 0 {
                        break;
                    }
                }
            }
        } else if shift_down_of_top_row < 0 {
            // Expanding: only move screen up if there's transcript to show
            let actual_shift = std::cmp::max(shift_down_of_top_row, -(self.active_transcript_rows as i32));
            
            if shift_down_of_top_row != actual_shift {
                // The new lines revealed by resizing are not all from transcript
                // Blank the below ones (Java allocates new lines, we just clear)
                let blank_count = actual_shift - shift_down_of_top_row;
                for i in 0..blank_count {
                    let row_to_clear = (self.first_row + old_rows + i as usize) % self.buffer.len();
                    self.buffer[row_to_clear].clear_all(current_style);
                }
                shift_down_of_top_row = actual_shift;
            }
        }
        
        // Adjust first_row pointer (O(1) operation)
        let new_first_row = self.first_row as i32 + shift_down_of_top_row;
        self.first_row = if new_first_row < 0 {
            (new_first_row + self.buffer.len() as i32) as usize
        } else {
            (new_first_row as usize) % self.buffer.len()
        };
        
        // Update active_transcript_rows
        let shift_i32 = shift_down_of_top_row;
        self.active_transcript_rows = if shift_i32 > 0 {
            // Shrinking: increase transcript rows
            self.active_transcript_rows + shift_i32 as usize
        } else {
            // Expanding: decrease transcript rows
            self.active_transcript_rows.saturating_sub((-shift_i32) as usize)
        };
        
        // Adjust cursor position
        let new_cursor_y = cursor_y - shift_i32;
        
        // Update rows
        self.rows = new_rows;
        
        (cursor_x, new_cursor_y)
    }
}
