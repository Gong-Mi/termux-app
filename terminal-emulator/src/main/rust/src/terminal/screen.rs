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
            // 修复：\0 虽然渲染不可见，但它是宽字符的物理占位符，不能被截断
            if self.text[i] != ' ' {
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
        self.text[x1..end].iter().filter(|&&c| c != '\0').collect()
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
    if ucs == 0 { return 0; } // \0 是宽字符占位符，宽度为 0
    if ucs == 32 { return 1; } // 空格
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

    /// Get a row by external row number (e.g., 0 = first visible row, -1 = last history row)
    /// Adds bounds checking to prevent accessing invalid rows
    pub fn get_row(&self, row: i32) -> &TerminalRow {
        // Bounds checking: row must be in [-active_transcript_rows, rows-1]
        let min_row = -(self.active_transcript_rows as i32);
        let max_row = self.rows as i32 - 1;
        let clamped_row = row.max(min_row).min(max_row);
        &self.buffer[self.internal_row(clamped_row)]
    }

    /// Get a mutable row by external row number
    pub fn get_row_mut(&mut self, row: i32) -> &mut TerminalRow {
        // Bounds checking: row must be in [-active_transcript_rows, rows-1]
        let min_row = -(self.active_transcript_rows as i32);
        let max_row = self.rows as i32 - 1;
        let clamped_row = row.max(min_row).min(max_row);
        let idx = self.internal_row(clamped_row);
        &mut self.buffer[idx]
    }

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
            let mut e_x = if y == ey { min(self.cols, ex + 1) as usize } else { self.cols as usize };
            
            // Trim trailing spaces for lines that don't wrap and aren't fully selected
            let space_used = row.get_space_used();
            if e_x > space_used && (!row.line_wrap || y == ey) {
                e_x = space_used;
            }
            
            if s_x < e_x {
                let text = row.get_selected_text(s_x, e_x);
                // Filter out the '\0' placeholder characters used for wide chars
                let filtered: String = text.chars().filter(|&c| c != '\0').collect();
                res.push_str(&filtered);
            }
            if y < ey && !row.line_wrap { res.push('\n'); }
        }
        res
    }

    pub fn erase_in_display(&mut self, mode: i32, cursor_y: i32, cursor_x: i32, style: u64) {
        let c = self.cols as usize;
        match mode {
            0 => {
                // Erase from cursor to end of screen (including current row from cursor)
                self.get_row_mut(cursor_y).clear(cursor_x as usize, c, style);
                for y in (cursor_y + 1)..self.rows { self.get_row_mut(y).clear(0, c, style); }
            }
            1 => {
                // Erase from start of screen to cursor (including current row up to cursor)
                for y in 0..cursor_y { self.get_row_mut(y).clear(0, c, style); }
                self.get_row_mut(cursor_y).clear(0, (cursor_x + 1) as usize, style);
            }
            2 => { for y in 0..self.rows { self.get_row_mut(y).clear(0, c, style); } }
            3 => {
                // CSI 3 J - 清除滚动历史 (Transcript)，保留屏幕上的可见内容
                // 对齐 Java TerminalBuffer.clearTranscript() 和 xterm 的行为
                self.clear_transcript(style);
            }
            _ => {}
        }
    }

    /// 清除历史行（transcript），保留屏幕上的可见内容
    pub fn clear_transcript(&mut self, style: u64) {
        let total_rows = self.buffer.len();
        let c = self.cols as usize;
        
        if self.active_transcript_rows > 0 {
            // 清除逻辑：找到历史行的物理索引并清空，然后重置 active_transcript_rows
            if self.first_row < self.active_transcript_rows {
                // 历史记录跨越了缓冲区末尾
                let start = total_rows + self.first_row - self.active_transcript_rows;
                for i in start..total_rows {
                    self.buffer[i].clear(0, c, style);
                }
                for i in 0..self.first_row {
                    self.buffer[i].clear(0, c, style);
                }
            } else {
                let start = self.first_row - self.active_transcript_rows;
                for i in start..self.first_row {
                    self.buffer[i].clear(0, c, style);
                }
            }
            self.active_transcript_rows = 0;
        }
    }

    pub fn insert_lines(&mut self, cursor_y: i32, bottom: i32, n: i32, style: u64) {
        let to_insert = min(n, bottom - cursor_y);
        let to_move = (bottom - cursor_y) - to_insert;
        
        if to_move > 0 {
            for i in (0..to_move).rev() {
                let s = self.internal_row(cursor_y + i);
                let d = self.internal_row(cursor_y + i + to_insert);
                // 使用 swap 避免 clone，并复用对象
                let (low, high) = if s < d { (s, d) } else { (d, s) };
                let (left, right) = self.buffer.split_at_mut(high);
                std::mem::swap(&mut left[low], &mut right[0]);
            }
        }
        for i in 0..to_insert { self.get_row_mut(cursor_y + i).clear_all(style); }
    }

    pub fn delete_lines(&mut self, cursor_y: i32, bottom: i32, n: i32, style: u64) {
        let to_delete = min(n, bottom - cursor_y);
        let to_move = (bottom - cursor_y) - to_delete;
        
        if to_move > 0 {
            for i in 0..to_move {
                let s = self.internal_row(cursor_y + i + to_delete);
                let d = self.internal_row(cursor_y + i);
                let (low, high) = if s < d { (s, d) } else { (d, s) };
                let (left, right) = self.buffer.split_at_mut(high);
                std::mem::swap(&mut left[low], &mut right[0]);
            }
        }
        for i in 0..to_delete { self.get_row_mut(bottom - i - 1).clear_all(style); }
    }

    pub fn scroll_up(&mut self, top: i32, bottom: i32, style: u64) {
        if top == 0 && bottom == self.rows {
            // Full screen scroll - use ring buffer pointer adjustment (O(1))
            self.first_row = (self.first_row + 1) % self.buffer.len();
            let max_transcript_rows = self.buffer.len() - self.rows as usize;
            if self.active_transcript_rows < max_transcript_rows {
                self.active_transcript_rows += 1;
            }
            self.get_row_mut(self.rows - 1).clear_all(style);
        } else {
            // Partial scroll - move data
            let to_move = (bottom - top) - 1;
            for i in 0..to_move {
                let s = self.internal_row(top + i + 1);
                let d = self.internal_row(top + i);
                let (low, high) = if s < d { (s, d) } else { (d, s) };
                let (left, right) = self.buffer.split_at_mut(high);
                std::mem::swap(&mut left[low], &mut right[0]);
            }
            self.get_row_mut(bottom - 1).clear_all(style);
        }
    }

    pub fn scroll_down(&mut self, top: i32, bottom: i32, style: u64) {
        let to_move = (bottom - top) - 1;
        for i in (0..to_move).rev() {
            let s = self.internal_row(top + i);
            let d = self.internal_row(top + i + 1);
            let (low, high) = if s < d { (s, d) } else { (d, s) };
            let (left, right) = self.buffer.split_at_mut(high);
            std::mem::swap(&mut left[low], &mut right[0]);
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

        // 使用与 Java 相同的 newTotalRows
        let new_total_rows = old_total;

        // Create new buffer with sufficient capacity
        let mut new_buffer: Vec<TerminalRow> = Vec::with_capacity(new_total_rows);
        for _ in 0..new_total_rows {
            let mut row = TerminalRow::new(n_cols);
            row.clear_all(current_style);
            new_buffer.push(row);
        }

        let mut new_cursor_x: i32 = 0;
        let mut new_cursor_y: i32 = 0;
        let mut cursor_placed = false;

        // 使用环形缓冲区写入：维护 first_row 和 output_row
        // 内容写入 (first_row + output_row) % total_rows
        let mut screen_first_row: usize = 0;
        let mut output_row: usize = 0; // 相对于 first_row 的偏移
        let mut output_col: usize = 0;

        // Track skipped blank lines (Java logic)
        let mut skipped_blank_lines = 0;

        // 实际屏幕行数（用于滚动判断）
        let screen_rows = new_rows as usize;

        // 追踪历史行数（模拟 Java 的 scrollDownOneLine 累积逻辑）
        let mut new_active_transcript_rows: usize = 0;
        let max_transcript_rows = new_total_rows.saturating_sub(screen_rows);

        // 辅助闭包：获取当前 output_row 对应的 buffer 索引
        let row_idx = |first_row: usize, row: usize, total: usize| -> usize {
            (first_row + row) % total
        };

        // 辅助闭包：执行滚动（模拟 Java scrollDownOneLine）
        let do_scroll = |first_row: &mut usize, active: &mut usize, sr: usize, style: u64, total: usize, max_active: usize, buf: &mut Vec<TerminalRow>| {
            // Java: mScreenFirstRow = (mScreenFirstRow + 1) % mTotalRows;
            *first_row = (*first_row + 1) % total;
            // Java: if (mActiveTranscriptRows < mTotalRows - mScreenRows) mActiveTranscriptRows++;
            if *active < max_active { *active += 1; }
            // 清空新底部行
            let bottom_idx = (*first_row + sr - 1) % total;
            buf[bottom_idx].clear_all(style);
        };

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
                    if output_row >= screen_rows - 1 {
                        // Buffer is full - scroll up
                        if cursor_placed && new_cursor_y > 0 {
                            new_cursor_y -= 1;
                        }
                        do_scroll(&mut screen_first_row, &mut new_active_transcript_rows, screen_rows, current_style, new_total_rows, max_transcript_rows, &mut new_buffer);
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

            let _just_to_cursor = cursor_at_this_row;

            // Process each character in the old line
            let mut i = 0;
            let mut current_old_col: usize = 0;
            let mut style_at_col = current_style;

            while i < last_non_space_index {
                let c = old_line.text[i];
                let code_point = c as u32;
                let display_width = local_get_width(code_point);
                
                // 核心修复：宽字符原子性检测
                // 如果当前是宽字符，检查下一个是否是 \0 占位符，并将它们作为一个整体处理
                let is_atomic_pair = display_width == 2 && i + 1 < old_line.text.len() && old_line.text[i+1] == '\0';
                let unit_width = if is_atomic_pair { 2 } else { display_width as usize };

                // Update style for this column
                if display_width > 0 && current_old_col < old_cols {
                    style_at_col = old_line.styles[current_old_col];
                }

                // Line wrap as necessary (check if the entire unit fits)
                if output_col + unit_width > n_cols {
                    if output_row < new_buffer.len() {
                        let idx = row_idx(screen_first_row, output_row, new_total_rows);
                        new_buffer[idx].line_wrap = true;
                    }
                    if output_row >= screen_rows - 1 {
                        if cursor_placed && new_cursor_y > 0 { new_cursor_y -= 1; }
                        do_scroll(&mut screen_first_row, &mut new_active_transcript_rows, screen_rows, current_style, new_total_rows, max_transcript_rows, &mut new_buffer);
                    } else {
                        output_row += 1;
                    }
                    output_col = 0;
                }

                // Set character unit in new buffer
                if output_row < new_buffer.len() {
                    let idx = row_idx(screen_first_row, output_row, new_total_rows);
                    new_buffer[idx].text[output_col] = c;
                    new_buffer[idx].styles[output_col] = style_at_col;
                    
                    if is_atomic_pair && output_col + 1 < n_cols {
                        new_buffer[idx].text[output_col + 1] = '\0';
                        new_buffer[idx].styles[output_col + 1] = style_at_col;
                    }
                }

                // Track cursor position
                if cursor_at_this_row && current_old_col == cursor_x as usize && !cursor_placed {
                    new_cursor_x = output_col as i32;
                    new_cursor_y = output_row as i32;
                    cursor_placed = true;
                }

                // Advance indices
                if is_atomic_pair {
                    i += 2;
                    current_old_col += 2;
                    output_col += 2;
                } else {
                    i += 1;
                    if display_width > 0 {
                        current_old_col += display_width as usize;
                        output_col += display_width as usize;
                    }
                }
            }

            // Check if we need to insert newline (line was not wrapping)
            if external_old_row != (end_row - 1) && !old_line.line_wrap {
                if output_row >= screen_rows - 1 {
                    // Buffer is full - scroll up
                    if cursor_placed && new_cursor_y > 0 {
                        new_cursor_y -= 1;
                    }
                    do_scroll(&mut screen_first_row, &mut new_active_transcript_rows, screen_rows, current_style, new_total_rows, max_transcript_rows, &mut new_buffer);
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
        self.first_row = screen_first_row;

        // 使用正确累积的历史行数（模拟 Java scrollDownOneLine 逻辑）
        self.active_transcript_rows = new_active_transcript_rows;

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
            // Java logic: actualShift = max(shiftDownOfTopRow, -mActiveTranscriptRows)
            let actual_shift = std::cmp::max(shift_down_of_top_row, -(self.active_transcript_rows as i32));

            if shift_down_of_top_row != actual_shift {
                // The new lines revealed by resizing are not all from transcript.
                // Blank the below ones.
                // Java: for (int i = 0; i < actualShift - shiftDownOfTopRow; i++)
                //         allocateFullLineIfNecessary((mScreenFirstRow + mScreenRows + i) % mTotalRows).clear(currentStyle);
                let blank_count = actual_shift - shift_down_of_top_row;
                
                // Calculate the position of new visible rows AFTER expansion
                // The new rows will be at positions [old_rows, old_rows + 1, ..., new_rows - 1]
                // In the ring buffer, these correspond to:
                // (first_row + old_rows) % len, (first_row + old_rows + 1) % len, etc.
                // But first_row will change! We need to calculate based on the NEW first_row.
                
                // After expansion, first_row will be: first_row + actual_shift
                // The new visible rows start at: first_row + actual_shift + old_rows
                // But we want to clear rows that will be at the bottom of the NEW screen
                
                // Actually, Java clears rows at the BOTTOM of the old screen area
                // These are rows that will become visible but are not from transcript
                for i in 0..blank_count {
                    // The row to clear is at position (first_row + old_rows + i) in the ring buffer
                    // But we need to calculate this BEFORE changing first_row
                    let row_idx = (self.first_row + old_rows + i as usize) % self.buffer.len();
                    self.buffer[row_idx].clear_all(current_style);
                }
                shift_down_of_top_row = actual_shift;
            }
        }
        
        // Adjust first_row pointer (O(1) operation)
        // Matches Java: mScreenFirstRow = (mScreenFirstRow < 0) ? (mScreenFirstRow + mTotalRows) : (mScreenFirstRow % mTotalRows)
        let new_first_row = self.first_row as i32 + shift_down_of_top_row;
        self.first_row = if new_first_row < 0 {
            // Use modulo to handle large negative shifts (e.g., expanding from very small to very large)
            ((new_first_row % self.buffer.len() as i32) + self.buffer.len() as i32) as usize % self.buffer.len()
        } else {
            (new_first_row as usize) % self.buffer.len()
        };
        
        // Update active_transcript_rows (matches Java: mActiveTranscriptRows = max(0, mActiveTranscriptRows + shiftDownOfTopRow))
        // shift_down_of_top_row > 0 means shrinking (more transcript rows)
        // shift_down_of_top_row < 0 means expanding (fewer transcript rows)
        let shift_i32 = shift_down_of_top_row;
        self.active_transcript_rows = if shift_i32 > 0 {
            // Shrinking: increase transcript rows
            self.active_transcript_rows + shift_i32 as usize
        } else {
            // Expanding: decrease transcript rows (use saturating_sub for max(0, ...))
            self.active_transcript_rows.saturating_sub((-shift_i32) as usize)
        };

        // Ensure active_transcript_rows doesn't exceed max possible (matches Java constraint)
        // Java: mActiveTranscriptRows is implicitly limited by mTotalRows - mScreenRows
        let max_transcript_rows = self.buffer.len().saturating_sub(self.rows as usize);
        self.active_transcript_rows = self.active_transcript_rows.min(max_transcript_rows);
        
        // Adjust cursor position
        let new_cursor_y = cursor_y - shift_i32;
        
        // Update rows
        self.rows = new_rows;
        
        (cursor_x, new_cursor_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- TerminalRow tests ---

    #[test]
    fn test_row_new() {
        let r = TerminalRow::new(80);
        assert_eq!(r.text.len(), 80);
        assert_eq!(r.styles.len(), 80);
        assert!(!r.line_wrap);
        assert!(r.text.iter().all(|&c| c == ' '));
    }

    #[test]
    fn test_row_clear() {
        let mut r = TerminalRow::new(10);
        r.text[3] = 'A';
        r.clear(2, 6, 0xDEAD);
        assert_eq!(r.text[3], ' ');
        assert_eq!(r.styles[3], 0xDEAD);
    }

    #[test]
    fn test_row_clear_all() {
        let mut r = TerminalRow::new(10);
        r.text[5] = 'X';
        r.styles[5] = 0xBEEF;
        r.clear_all(0);
        assert!(r.text.iter().all(|&c| c == ' '));
        assert!(r.styles.iter().all(|&s| s == 0));
    }

    #[test]
    fn test_row_set_char() {
        let mut r = TerminalRow::new(10);
        r.set_char(3, 'A' as u32, 0x1234);
        assert_eq!(r.text[3], 'A');
        assert_eq!(r.styles[3], 0x1234);
        // Other cells unchanged
        assert_eq!(r.text[0], ' ');
    }

    #[test]
    fn test_row_insert_spaces() {
        let mut r = TerminalRow::new(10);
        r.text[0] = 'A'; r.styles[0] = 1;
        r.text[2] = 'B'; r.styles[2] = 2;

        // Insert 2 spaces at column 1 with style 99
        r.insert_spaces(1, 2, 99);

        // Positions 1,2 are spaces with style 99
        assert_eq!(r.text[1], ' ');
        assert_eq!(r.text[2], ' ');
        assert_eq!(r.styles[1], 99);
        assert_eq!(r.styles[2], 99);
        // Position 0 unchanged
        assert_eq!(r.text[0], 'A');
        assert_eq!(r.styles[0], 1);
        // Position 2 content 'B' shifted to position 4 (2+2)
        assert_eq!(r.text[4], 'B');
        assert_eq!(r.styles[4], 2);
    }

    #[test]
    fn test_row_delete_characters() {
        let mut r = TerminalRow::new(10);
        for i in 0..10 {
            r.set_char(i, (b'A' + i as u8) as u32, i as u64);
        }
        r.delete_characters(2, 3, 0);
        // chars at 2,3,4 removed; chars from 5+ shifted left
        assert_eq!(r.text[2], 'F');
        assert_eq!(r.text[3], 'G');
        assert_eq!(r.text[4], 'H');
        // Last 3 cells are spaces
        assert_eq!(r.text[7], ' ');
        assert_eq!(r.text[8], ' ');
        assert_eq!(r.text[9], ' ');
    }

    #[test]
    fn test_row_copy_text() {
        let mut r = TerminalRow::new(10);
        for i in 0..5 {
            r.set_char(i, (b'a' + i as u8) as u32, 0);
        }
        let mut dest = [0u16; 10];
        r.copy_text(1, 4, &mut dest);
        assert_eq!(dest[0], 'b' as u16);
        assert_eq!(dest[1], 'c' as u16);
        assert_eq!(dest[2], 'd' as u16);
        assert_eq!(dest[3], 0u16); // rest is null
    }

    #[test]
    fn test_row_get_word_at() {
        let mut r = TerminalRow::new(20);
        let text: Vec<char> = "  hello  world  ".chars().collect();
        for (i, ch) in text.into_iter().enumerate() {
            r.text[i] = ch;
        }
        // "hello" at column 2
        let word = r.get_word_at(2);
        assert_eq!(word, "hello");
        // "world" at column 9
        let word2 = r.get_word_at(9);
        assert_eq!(word2, "world");
    }

    // --- Screen tests ---

    #[test]
    fn test_screen_new() {
        let s = Screen::new(80, 24, 100);
        assert_eq!(s.cols, 80);
        assert_eq!(s.rows, 24);
        assert_eq!(s.buffer.len(), 100);
        assert_eq!(s.active_transcript_rows, 0);
        assert_eq!(s.first_row, 0);
    }

    #[test]
    fn test_screen_internal_row_simple() {
        let s = Screen::new(80, 24, 24);
        // No scrolling, direct mapping
        assert_eq!(s.internal_row(0), 0);
        assert_eq!(s.internal_row(23), 23);
    }

    #[test]
    fn test_screen_block_clear() {
        let mut s = Screen::new(10, 5, 5);
        // Put content in center
        let row = s.get_row_mut(2);
        row.set_char(5, 'X' as u32, 1);
        // Removed drop(row) call as it was a reference and did nothing
        s.block_clear(1, 0, 3, 9, 0); // clear rows 1-3 fully

        let row_after = s.get_row(2);
        assert_eq!(row_after.text[5], ' ');
    }

    #[test]
    fn test_screen_scroll_up_partial() {
        // Partial scroll (not full screen) uses data movement
        let mut s = Screen::new(10, 3, 5);
        s.get_row_mut(0).set_char(0, 'A' as u32, 0);
        s.get_row_mut(1).set_char(0, 'B' as u32, 1);
        s.get_row_mut(2).set_char(0, 'C' as u32, 2);

        // Full screen scroll (top=0, bottom=rows) uses ring buffer pointer shift
        s.scroll_up(0, 3, 0);

        // After ring buffer scroll: first_row shifts, row 0 gets old row 1, etc.
        // first_row was 0, now 1. Row at internal index 1 = old index 2 = 'C'
        // So visible row 0 → internal row (first_row + 0) % len = 1 → 'B'
        // Wait, let me trace: buffer has 5 rows. first_row=1.
        // visible row 0 → internal_row(0) = (first_row + 0) % 5 = 1 → old row 1 = 'B'
        // visible row 1 → internal_row(1) = 2 → old row 2 = 'C'
        // visible row 2 → internal_row(2) = 3 → newly cleared = ' '
        assert_eq!(s.get_row(0).text[0], 'B');
        assert_eq!(s.get_row(1).text[0], 'C');
        assert_eq!(s.get_row(2).text[0], ' ');
        // active_transcript_rows should increase
        assert_eq!(s.active_transcript_rows, 1);
    }

    #[test]
    fn test_screen_scroll_down_full() {
        let mut s = Screen::new(10, 3, 5);
        s.get_row_mut(0).set_char(0, 'A' as u32, 0);
        s.get_row_mut(1).set_char(0, 'B' as u32, 1);
        s.get_row_mut(2).set_char(0, 'C' as u32, 2);

        s.scroll_down(0, 3, 0);

        // scroll_down copies from i-1 to i (reverse), then clears row 0
        // row 2 ← row 1 = 'B', row 1 ← row 0 = 'A', row 0 cleared
        assert_eq!(s.get_row(0).text[0], ' ');
        assert_eq!(s.get_row(1).text[0], 'A');
        assert_eq!(s.get_row(2).text[0], 'B');
    }

    #[test]
    fn test_screen_erase_in_display_all() {
        let mut s = Screen::new(10, 5, 5);
        // Fill visible rows
        for r in 0..5 {
            s.get_row_mut(r).set_char(0, (b'A' + r as u8) as u32, 0);
        }

        s.erase_in_display(2, 0, 0, 0); // erase all (mode=2, cursor at 0,0)

        for r in 0..5 {
            assert_eq!(s.get_row(r).text[0], ' ', "Row {} should be cleared", r);
        }
    }

    #[test]
    fn test_screen_erase_below_cursor() {
        let mut s = Screen::new(10, 5, 5);
        for r in 0..5 {
            s.get_row_mut(r).set_char(0, (b'A' + r as u8) as u32, 0);
        }

        // mode=1: erase from cursor to end of screen
        s.erase_in_display(0, 2, 2, 0); // erase from (2,2) to end

        // Rows above cursor (0, 1) should still have content
        assert_eq!(s.get_row(0).text[0], 'A');
        assert_eq!(s.get_row(1).text[0], 'B');
        // From cursor row onward: row 2 cleared from col 2+, rows 3-4 fully cleared
        assert_eq!(s.get_row(3).text[0], ' ');
        assert_eq!(s.get_row(4).text[0], ' ');
    }

    #[test]
    fn test_screen_insert_lines() {
        let mut s = Screen::new(10, 5, 10);
        for r in 0..5 {
            s.get_row_mut(r).set_char(0, (b'A' + r as u8) as u32, 0);
        }

        // insert 2 lines at row 2, bottom=4 (scroll region [2, 4))
        // to_insert = min(2, 4-2) = 2, to_move = 0
        // Rows 2,3 get cleared; rows 0,1,4 unaffected
        s.insert_lines(2, 4, 2, 0);

        assert_eq!(s.get_row(0).text[0], 'A');
        assert_eq!(s.get_row(1).text[0], 'B');
        assert_eq!(s.get_row(2).text[0], ' '); // cleared
        assert_eq!(s.get_row(3).text[0], ' '); // cleared
        assert_eq!(s.get_row(4).text[0], 'E'); // outside scroll region, unchanged
    }

    #[test]
    fn test_screen_delete_lines() {
        let mut s = Screen::new(10, 5, 10);
        for r in 0..5 {
            s.get_row_mut(r).set_char(0, (b'A' + r as u8) as u32, 0);
        }

        // delete 2 lines at row 1, bottom=4 (scroll region [1, 4))
        // to_delete = min(2, 4-1) = 2, to_move = 3 - 2 = 1
        // row 1 ← row 3 ('D'), then clear rows 3, 2
        s.delete_lines(1, 4, 2, 0);

        assert_eq!(s.get_row(0).text[0], 'A'); // unchanged
        assert_eq!(s.get_row(1).text[0], 'D'); // shifted from row 3
        assert_eq!(s.get_row(2).text[0], ' '); // cleared (within scroll region)
        assert_eq!(s.get_row(3).text[0], ' '); // cleared
        assert_eq!(s.get_row(4).text[0], 'E'); // outside scroll region
    }

    #[test]
    fn test_screen_get_selected_text() {
        let mut s = Screen::new(5, 3, 3);
        // Row 0: "Hello" (exactly 5 chars)
        for (i, ch) in "Hello".chars().enumerate() {
            s.get_row_mut(0).set_char(i, ch as u32, 0);
        }
        // Row 1: "World"
        for (i, ch) in "World".chars().enumerate() {
            s.get_row_mut(1).set_char(i, ch as u32, 0);
        }

        let text = s.get_selected_text(0, 0, 4, 1);
        // Each row is padded to full width (5 chars) + newline
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_screen_resize_columns_only() {
        // Verify the resize happens without panic and dimensions are correct.
        // The slow path reflow behavior is complex; just check dimensions.
        let mut s = Screen::new(10, 3, 5);
        for i in 0..10 {
            s.get_row_mut(0).set_char(i, (b'A' + i as u8) as u32, 0);
        }

        let (_new_cx, _new_cy) = s.resize_with_reflow(5, 3, 0, 0, 0);
        assert_eq!(s.cols, 5);
        assert_eq!(s.rows, 3);
        assert_eq!(s.get_row(0).text[0], 'A');
    }

    #[test]
    fn test_screen_resize_rows_only_fast_path() {
        // Fast path: only rows change (columns unchanged)
        let mut s = Screen::new(10, 3, 5);
        for i in 0..10 {
            s.get_row_mut(0).set_char(i, (b'A' + i as u8) as u32, 0);
        }

        let (new_cx, new_cy) = s.resize_with_reflow(10, 5, 0, 0, 0);
        assert_eq!(s.cols, 10);
        assert_eq!(s.rows, 5);
        // Content should be preserved
        assert_eq!(s.get_row(0).text[0], 'A');
        assert_eq!(s.get_row(0).text[9], 'J');
        assert_eq!(new_cx, 0);
        assert_eq!(new_cy, 0);
    }
}
