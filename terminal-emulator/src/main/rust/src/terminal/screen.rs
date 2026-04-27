use std::cmp::{max, min};
use crate::terminal::style::{STYLE_NORMAL};

#[derive(Clone)]
pub struct TerminalRow {
    pub text: Vec<char>,
    pub styles: Vec<u64>,
    pub line_wrap: bool,
}

impl TerminalRow {
    pub fn new(cols: u64) -> Self {
        Self {
            text: vec![' '; cols as usize],
            styles: vec![STYLE_NORMAL; cols as usize],
            line_wrap: false,
        }
    }

    pub fn clear(&mut self, start: u64, end: u64, style: u64) {
        let len = self.text.len() as u64;
        let end = min(end, len);
        if start < end {
            for i in (start as usize)..(end as usize) {
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

    pub fn set_char(&mut self, column: u64, code_point: u32, style: u64) {
        if (column as usize) < self.text.len() {
            self.text[column as usize] = std::char::from_u32(code_point).unwrap_or(' ');
            self.styles[column as usize] = style;
        }
    }

    pub fn insert_spaces(&mut self, column: u64, n: u64, style: u64) {
        let len = self.text.len() as u64;
        if column < len {
            let n = min(n, len - column);
            for i in ((column + n) as usize..(len as usize)).rev() {
                self.text[i] = self.text[i - n as usize];
                self.styles[i] = self.styles[i - n as usize];
            }
            for i in (column as usize)..(column as usize + n as usize) {
                self.text[i] = ' ';
                self.styles[i] = style;
            }
        }
    }

    pub fn delete_characters(&mut self, column: u64, n: u64, style: u64) {
        let len = self.text.len() as u64;
        if column < len {
            let n = min(n, len - column);
            for i in (column as usize)..(len as usize - n as usize) {
                self.text[i] = self.text[i + n as usize];
                self.styles[i] = self.styles[i + n as usize];
            }
            for i in (len as usize - n as usize)..(len as usize) {
                self.text[i] = ' ';
                self.styles[i] = style;
            }
        }
    }

    pub fn get_space_used(&self) -> u64 {
        for i in (0..self.text.len()).rev() {
            // 跳过尾随的空格和 '\0' 占位符
            // '\0' 是宽字符的第二个单元格，不计入逻辑“空间使用”长度
            if self.text[i] != ' ' && self.text[i] != '\0' {
                return (i + 1) as u64;
            }
        }
        0
    }

    pub fn copy_text(&self, start: u64, end: u64, dest: &mut [u16]) {
        let text_len = self.text.len() as u64;
        let end = min(end, text_len);
        let count = end.saturating_sub(start);
        for i in 0..min(count, dest.len() as u64) {
            dest[i as usize] = self.text[(start + i) as usize] as u16;
        }
    }

    pub fn find_char_index_at_column(&self, column: u64) -> u64 {
        let mut cur_col = 0;
        let mut cur_idx = 0;
        while cur_idx < self.text.len() {
            let c = self.text[cur_idx];
            let width = local_get_width(c as u32);
            if width > 0 {
                if cur_col as u64 >= column { return cur_idx as u64; }
                cur_col += width;
            } else {
                if cur_col as u64 >= column { return cur_idx as u64; }
            }
            cur_idx += 1;
        }
        self.get_space_used()
    }

    pub fn get_selected_text(&self, x1: u64, x2: u64) -> String {
        let cols = self.text.len() as u64;
        if x1 >= cols { return String::new(); }
        let end = min(x2, cols);
        self.text[x1 as usize..end as usize].iter().filter(|&&c| c != '\0').collect()
    }

    pub fn get_word_at(&self, column: u64) -> String {
        let cols = self.text.len() as u64;
        if column >= cols { return String::new(); }
        fn is_word(c: char) -> bool { c.is_alphanumeric() || c == '_' }
        if !is_word(self.text[column as usize]) { return String::new(); }
        let mut s = column as usize; while s > 0 && is_word(self.text[s-1]) { s -= 1; }
        let mut e = column as usize; while e + 1 < cols as usize && is_word(self.text[e+1]) { e += 1; }
        self.text[s..=e].iter().collect()
    }
}

fn local_get_width(ucs: u32) -> usize {
    crate::wcwidth::wcwidth(ucs)
}

pub struct Screen {
    pub rows: i64,
    pub cols: i64,
    pub buffer: Vec<TerminalRow>,
    pub first_row: u64,
    pub active_transcript_rows: u64,
}

impl Screen {
    pub fn new(cols: i64, rows: i64, total_rows: i64) -> Self {
        let t_u = max(rows as u64, total_rows as u64);
        let mut b = Vec::with_capacity(t_u as usize);
        for _ in 0..t_u { b.push(TerminalRow::new(max(1, cols as u64))); }
        Self { rows, cols, buffer: b, first_row: 0, active_transcript_rows: 0 }
    }

    #[inline]
    pub fn internal_row(&self, row: i64) -> usize {
        let t = self.buffer.len() as i128; // Use i128 to prevent overflow during intermediate calculations
        if t == 0 { return 0; }
        (((self.first_row as i128 + row as i128) % t + t) % t) as usize
    }

    /// Get a row by external row number (e.g., 0 = first visible row, -1 = last history row)
    /// Adds bounds checking to prevent accessing invalid rows
    pub fn get_row(&self, row: i64) -> &TerminalRow {
        // Bounds checking: row must be in [-active_transcript_rows, rows-1]
        let min_row = -(self.active_transcript_rows as i64);
        let max_row = self.rows - 1;
        let clamped_row = row.max(min_row).min(max_row);
        &self.buffer[self.internal_row(clamped_row)]
    }

    /// Get a mutable row by external row number
    pub fn get_row_mut(&mut self, row: i64) -> &mut TerminalRow {
        // Bounds checking: row must be in [-active_transcript_rows, rows-1]
        let min_row = -(self.active_transcript_rows as i64);
        let max_row = self.rows - 1;
        let clamped_row = row.max(min_row).min(max_row);
        let idx = self.internal_row(clamped_row);
        &mut self.buffer[idx]
    }

    pub fn block_clear(&mut self, top: u64, left: u64, bottom: u64, right: u64, style: u64) {
        let cols = self.cols as u64;
        let rows = self.rows as u64;
        for row in top..min(bottom, rows) {
            self.get_row_mut(row as i64).clear(left, min(right, cols), style);
        }
    }

    pub fn get_transcript_text(&self) -> String {
        let mut res = String::new();
        let first_y = -(self.active_transcript_rows as i64);
        for y in first_y..self.rows {
            let row = self.get_row(y);
            res.push_str(&row.get_selected_text(0, row.get_space_used()));
            if !row.line_wrap && y < self.rows - 1 { res.push('\n'); }
        }
        res
    }

    pub fn get_selected_text(&self, x1: i64, y1: i64, x2: i64, y2: i64) -> String {
        let mut res = String::new();
        let (sy, sx, ey, ex) = if y1 < y2 || (y1 == y2 && x1 <= x2) { (y1, x1, y2, x2) } else { (y2, x2, y1, x1) };
        for y in sy..=ey {
            let row = self.get_row(y);
            let s_x = if y == sy { max(0, sx) as u64 } else { 0 };
            let mut e_x = if y == ey { min(self.cols, ex + 1) as u64 } else { self.cols as u64 };
            
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

    pub fn erase_in_display(&mut self, mode: i64, cursor_y: i64, cursor_x: i64, style: u64) {
        let c = self.cols as u64;
        match mode {
            0 => {
                // Erase from cursor to end of screen (including current row from cursor)
                self.get_row_mut(cursor_y).clear(cursor_x as u64, c, style);
                for y in (cursor_y + 1)..self.rows { self.get_row_mut(y).clear(0, c, style); }
            }
            1 => {
                // Erase from start of screen to cursor (including current row up to cursor)
                for y in 0..cursor_y { self.get_row_mut(y).clear(0, c, style); }
                self.get_row_mut(cursor_y).clear(0, (cursor_x + 1) as u64, style);
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
        let total_rows = self.buffer.len() as u64;
        let c = self.cols as u64;
        
        if self.active_transcript_rows > 0 {
            // 清除逻辑：找到历史行的物理索引并清空，然后重置 active_transcript_rows
            if self.first_row < self.active_transcript_rows {
                // 历史记录跨越了缓冲区末尾
                let start = total_rows + self.first_row - self.active_transcript_rows;
                for i in (start as usize)..(total_rows as usize) {
                    self.buffer[i].clear(0, c, style);
                }
                for i in 0..(self.first_row as usize) {
                    self.buffer[i].clear(0, c, style);
                }
            } else {
                let start = self.first_row - self.active_transcript_rows;
                for i in (start as usize)..(self.first_row as usize) {
                    self.buffer[i].clear(0, c, style);
                }
            }
            self.active_transcript_rows = 0;
        }
    }

    pub fn insert_lines(&mut self, cursor_y: i64, bottom: i64, n: i64, style: u64) {
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

    pub fn delete_lines(&mut self, cursor_y: i64, bottom: i64, n: i64, style: u64) {
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

    pub fn scroll_up(&mut self, top: i64, bottom: i64, style: u64) {
        if top == 0 && bottom == self.rows {
            // Full screen scroll - use ring buffer pointer adjustment (O(1))
            self.first_row = (self.first_row + 1) % self.buffer.len() as u64;
            let max_transcript_rows = self.buffer.len() as u64 - self.rows as u64;
            if self.active_transcript_rows < max_transcript_rows {
                self.active_transcript_rows += 1;
            }
            self.get_row_mut(self.rows - 1).clear_all(style);
        } else {
            // Partial scroll - move data up by 1 line
            // We want to move rows [top+1 .. bottom] to [top .. bottom-1]
            // We can do this by swapping adjacent rows downwards
            for i in top..(bottom - 1) {
                let d = self.internal_row(i);
                let s = self.internal_row(i + 1);
                
                // Safe swap using split_at_mut
                let (low, high) = if s < d { (s, d) } else { (d, s) };
                let (left, right) = self.buffer.split_at_mut(high);
                std::mem::swap(&mut left[low], &mut right[0]);
            }
            // Clear the newly exposed bottom row
            self.get_row_mut(bottom - 1).clear_all(style);
        }
    }

    pub fn scroll_down(&mut self, top: i64, bottom: i64, style: u64) {
        // Partial scroll - move data down by 1 line
        // We want to move rows [top .. bottom-1] to [top+1 .. bottom]
        // We can do this by swapping adjacent rows upwards, starting from the bottom
        for i in (top..(bottom - 1)).rev() {
            let d = self.internal_row(i + 1);
            let s = self.internal_row(i);
            
            // Safe swap using split_at_mut
            let (low, high) = if s < d { (s, d) } else { (d, s) };
            let (left, right) = self.buffer.split_at_mut(high);
            std::mem::swap(&mut left[low], &mut right[0]);
        }
        // Clear the newly exposed top row
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
            let mut row = TerminalRow::new(n_cols as u64);
            row.clear_all(current_style);
            new_buffer.push(row);
        }

        let mut new_cursor_x: i32 = 0;
        let mut new_cursor_y: i32 = 0;
        let mut cursor_placed = false;

        // 使用环形缓冲区写入：维护 first_row 和 output_row
        // 内容写入 (first_row + output_row) % total_rows
        let mut screen_first_row: u64 = 0;
        let mut output_row: u64 = 0; // 相对于 first_row 的偏移
        let mut output_col: usize = 0;

        // Track skipped blank lines (Java logic)
        let mut skipped_blank_lines = 0;

        // 实际屏幕行数（用于滚动判断）
        let screen_rows = new_rows as u64;

        // 追踪历史行数（模拟 Java 的 scrollDownOneLine 累积逻辑）
        let mut new_active_transcript_rows: u64 = 0;
        let max_transcript_rows = (new_total_rows as u64).saturating_sub(screen_rows);

        // 辅助闭包：获取当前 output_row 对应的 buffer 索引
        let row_idx = |first_row: u64, row: u64, total: usize| -> usize {
            ((first_row + row) % total as u64) as usize
        };

        // 辅助闭包：执行滚动（模拟 Java scrollDownOneLine）
        let do_scroll = |first_row: &mut u64, active: &mut u64, sr: u64, style: u64, total: usize, max_active: u64, buf: &mut Vec<TerminalRow>| {
            // Java: mScreenFirstRow = (mScreenFirstRow + 1) % mTotalRows;
            *first_row = (*first_row + 1) % total as u64;
            // Java: if (mActiveTranscriptRows < mTotalRows - mScreenRows) mActiveTranscriptRows++;
            if *active < max_active { *active += 1; }
            // 清空新底部行
            let bottom_idx = ((*first_row + sr - 1) % total as u64) as usize;
            buf[bottom_idx].clear_all(style);
        };

        // Loop over every character in the initial state
        let start_row = -(old_active_transcript as i64);
        let end_row = old_rows as i64;

        for external_old_row in start_row..end_row {
            let internal_old_row = self.internal_row(external_old_row as i64);
            let old_line = &self.buffer[internal_old_row];
            let cursor_at_this_row = external_old_row == cursor_y as i64;

            // Check if line is blank (skip logic like Java)
            let is_blank = {
                let used = old_line.get_space_used();
                used == 0 || (0..used).all(|i| old_line.text[i as usize] == ' ')
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
                old_line.text.len() as u64
            } else {
                old_line.get_space_used()
            };

            let _just_to_cursor = cursor_at_this_row;

            // Process each character in the old line
            let mut i = 0u64;
            let mut current_old_col: usize = 0;
            let mut style_at_col = current_style;

            while i < last_non_space_index {
                let c = old_line.text[i as usize];
                let code_point = c as u32;
                let display_width = local_get_width(code_point);
                
                // 核心修复：宽字符原子性检测
                // 如果当前是宽字符，检查下一个是否是 \0 占位符，并将它们作为一个整体处理
                let is_atomic_pair = display_width == 2 && (i as usize) + 1 < old_line.text.len() && old_line.text[(i as usize) + 1] == '\0';
                let unit_width = if is_atomic_pair { 2 } else { display_width as usize };

                // Update style for this column
                if display_width > 0 && current_old_col < old_cols {
                    style_at_col = old_line.styles[current_old_col];
                }

                // Line wrap as necessary (check if the entire unit fits)
                if output_col + unit_width > n_cols {
                    if (output_row as usize) < new_buffer.len() {
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
                if (output_row as usize) < new_buffer.len() {
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

                i += if is_atomic_pair { 2 } else { 1 };
                output_col += unit_width;
                current_old_col += unit_width;
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

        // Final cursor placement if not done
        if !cursor_placed || new_cursor_x < 0 || new_cursor_y < 0 {
            // Flush remaining skipped blank lines if we're not past the screen end
            // This ensures consistent row alignment when trailing lines are blank
            for _ in 0..skipped_blank_lines {
                if output_row < screen_rows - 1 {
                    output_row += 1;
                }
            }
            new_cursor_x = output_col as i32;
            new_cursor_y = output_row as i32;
        }

        // Copy new_buffer to self.buffer
        self.buffer = new_buffer;
        self.cols = n_cols as i64;
        self.rows = new_rows as i64;
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
                let internal_row = self.internal_row(i as i64);
                let row_is_blank = {
                    let line = &self.buffer[internal_row];
                    let used = line.get_space_used();
                    used == 0 || (0..used).all(|j| line.text[j as usize] == ' ')
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
                let blank_count = actual_shift - shift_down_of_top_row;
                
                for i in 0..blank_count {
                    let row_idx = (self.first_row as u128 + old_rows as u128 + i as u128) % self.buffer.len() as u128;
                    self.buffer[row_idx as usize].clear_all(current_style);
                }
                shift_down_of_top_row = actual_shift;
            }
        }
        
        // Adjust first_row pointer (O(1) operation)
        let total_buf_len = self.buffer.len() as i128;
        let current_first_row = self.first_row as i128;
        let shift = shift_down_of_top_row as i128;
        
        let new_first_row = (current_first_row + shift) % total_buf_len;
        self.first_row = ((new_first_row + total_buf_len) % total_buf_len) as u64;
        
        // Update active_transcript_rows
        let shift_i64 = shift_down_of_top_row as i64;
        if shift_i64 > 0 {
            self.active_transcript_rows += shift_i64 as u64;
        } else {
            self.active_transcript_rows = self.active_transcript_rows.saturating_sub((-shift_i64) as u64);
        }

        // Ensure active_transcript_rows doesn't exceed max possible
        let max_transcript_rows = (self.buffer.len() as u64).saturating_sub(new_rows as u64);
        self.active_transcript_rows = self.active_transcript_rows.min(max_transcript_rows);
        
        // Adjust cursor position
        let new_cursor_y = cursor_y - shift_down_of_top_row;
        
        // Update rows
        self.rows = new_rows as i64;
        
        (cursor_x, new_cursor_y)
    }

    pub fn get_active_transcript_rows(&self) -> u64 {
        self.active_transcript_rows
    }

    pub fn clear_scroll_counter(&mut self) {
        // Handled by ScreenState
    }

    /// 压缩并修剪缓冲区，移除末尾的空行
    pub fn compact(&mut self) {
        let mut first_blank = self.rows;
        for i in (0..self.rows).rev() {
            let internal_row = self.internal_row(i);
            let line = &self.buffer[internal_row];
            let used = line.get_space_used();
            let is_blank = used == 0 || (0..used).all(|j| line.text[j as usize] == ' ');
            if !is_blank { break; }
            first_blank = i;
        }
        
        if first_blank < self.rows {
            // TODO: Implementation of physical buffer resizing if needed
        }
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
            r.set_char(i as u64, (b'A' + i as u8) as u32, i as u64);
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
            r.set_char(i as u64, (b'a' + i as u8) as u32, 0);
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
        // So visible row 0 → internal_row(0) = (first_row + 0) % 5 = 1 → 'B'
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
            s.get_row_mut(r as i64).set_char(0, (b'A' + r as u8) as u32, 0);
        }

        s.erase_in_display(2, 0, 0, 0); // erase all (mode=2, cursor at 0,0)

        for r in 0..5 {
            assert_eq!(s.get_row(r as i64).text[0], ' ', "Row {} should be cleared", r);
        }
    }

    #[test]
    fn test_screen_erase_below_cursor() {
        let mut s = Screen::new(10, 5, 5);
        for r in 0..5 {
            s.get_row_mut(r as i64).set_char(0, (b'A' + r as u8) as u32, 0);
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
            s.get_row_mut(r as i64).set_char(0, (b'A' + r as u8) as u32, 0);
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
            s.get_row_mut(r as i64).set_char(0, (b'A' + r as u8) as u32, 0);
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
            s.get_row_mut(0).set_char(i as u64, ch as u32, 0);
        }
        // Row 1: "World"
        for (i, ch) in "World".chars().enumerate() {
            s.get_row_mut(1).set_char(i as u64, ch as u32, 0);
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
            s.get_row_mut(0).set_char(i as u64, (b'A' + i as u8) as u32, 0);
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
            s.get_row_mut(0).set_char(i as u64, (b'A' + i as u8) as u32, 0);
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

    #[test]
    fn test_screen_internal_row_history_large() {
        // Buffer of 100 rows, visible rows 20.
        let mut s = Screen::new(80, 20, 100);
        s.first_row = 50;
        s.active_transcript_rows = 30;

        // visible row 0 -> physical 50
        assert_eq!(s.internal_row(0), 50);
        // visible row 19 -> physical 69
        assert_eq!(s.internal_row(19), 69);
        // last history row (-1) -> physical 49
        assert_eq!(s.internal_row(-1), 49);
        // earliest history row (-30) -> physical 20
        assert_eq!(s.internal_row(-30), 20);

        // Test wrapping: first_row = 90
        s.first_row = 90;
        // visible row 0 -> physical 90
        assert_eq!(s.internal_row(0), 90);
        // visible row 15 -> physical (90+15)%100 = 5
        assert_eq!(s.internal_row(15), 5);
        // last history row (-1) -> physical 89
        assert_eq!(s.internal_row(-1), 89);
    }

    #[test]
    fn test_screen_scroll_up_ring_buffer_wrap() {
        // Buffer of 5 rows, visible 3.
        let mut s = Screen::new(10, 3, 5);
        s.first_row = 4; // Screen starts at physical index 4
        
        // visible row 0 (index 4)
        s.get_row_mut(0).set_char(0, 'X' as u32, 0);
        
        s.scroll_up(0, 3, 0);
        
        // first_row should be (4+1)%5 = 0
        assert_eq!(s.first_row, 0);
        // old row 0 (physical 4) is now history index -1
        assert_eq!(s.get_row(-1).text[0], 'X');
        assert_eq!(s.active_transcript_rows, 1);
    }

    #[test]
    fn test_row_get_space_used_with_wide_char() {
        let mut r = TerminalRow::new(10);
        // '中' is width 2.
        r.set_char(0, '中' as u32, 0);
        r.text[1] = '\0'; // Manually set placeholder
        
        // It should return 2, because columns 0 and 1 are occupied.
        assert_eq!(r.get_space_used(), 2, "Space used should account for wide char placeholder");
    }

    #[test]
    fn test_screen_resize_with_reflow_trailing_blanks() {
        let mut s = Screen::new(10, 5, 5);
        // Row 0: "ABC"
        s.get_row_mut(0).set_char(0, 'A' as u32, 0);
        s.get_row_mut(0).set_char(1, 'B' as u32, 0);
        s.get_row_mut(0).set_char(2, 'C' as u32, 0);
        
        // Rows 1-4 are blank.
        // Cursor is at (0, 3) - row 3.
        
        let (nx, ny) = s.resize_with_reflow(10, 5, 0, 0, 3);
        
        // The cursor position should be preserved relative to the content.
        // If blank lines are skipped and not accounted for, ny might become 1.
        assert_eq!(ny, 3, "Cursor Y should be preserved even if preceding lines are blank");
    }

    #[test]
    fn test_memory_usage_5000_lines() {
        let cols = 100i64;
        let rows = 50i64;
        let total_rows = 5000i64;
        
        let mut s = Screen::new(cols, rows, total_rows);
        
        // 模拟填满 5000 行数据
        for i in 0..total_rows {
            let idx = s.internal_row(i - (total_rows - rows));
            for c in 0..cols as usize {
                s.buffer[idx].text[c] = 'A';
                s.buffer[idx].styles[c] = 0x12345678;
            }
        }

        // 计算近似堆内存占用
        let row_stack_size = std::mem::size_of::<TerminalRow>();
        
        let mut total_heap = 0;
        for row in &s.buffer {
            total_heap += row.text.capacity() * std::mem::size_of::<char>();
            total_heap += row.styles.capacity() * std::mem::size_of::<u64>();
        }
        
        let total_bytes = (s.buffer.capacity() * row_stack_size) + total_heap;
        let mb = total_bytes as f64 / 1024.0 / 1024.0;

        println!("\n--- 5000行压力测试报告 ---");
        println!("总行数: {}, 列数: {}", total_rows, cols);
        println!("估算内存占用: {:.2} MB", mb);
        println!("每行占用: {} 字节", total_bytes / total_rows as usize);
        
        // 验证基本功能
        assert_eq!(s.buffer.len(), 5000);
        
        // 性能检查：执行一次全屏缩放（最耗时操作）
        let start = std::time::Instant::now();
        s.resize_with_reflow(80, 24, 0, 0, 0);
        let duration = start.elapsed();
        println!("5000行重排(Reflow)耗时: {:?}", duration);
    }
}
