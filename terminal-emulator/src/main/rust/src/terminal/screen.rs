use crate::terminal::style::STYLE_NORMAL;
use std::cmp::{max, min};

#[derive(Clone)]
pub struct TerminalRow {
    pub text: Vec<char>,
    pub styles: Vec<u64>,
    pub line_wrap: bool,
}

impl TerminalRow {
    pub fn new(cols: u64) -> Self {
        Self::new_with_style(cols, STYLE_NORMAL)
    }

    pub fn new_with_style(cols: u64, style: u64) -> Self {
        Self {
            text: vec![' '; cols as usize],
            styles: vec![style; cols as usize],
            line_wrap: false,
        }
    }

    pub fn clear(&mut self, start: u64, end: u64, style: u64) {
        let len = self.text.len() as u64;
        let end = min(end, len);
        if start < end {
            self.text[(start as usize)..(end as usize)].fill(' ');
            self.styles[(start as usize)..(end as usize)].fill(style);
        }
    }

    /// 清空整行，对齐官方 Java TerminalRow.clear() 方法
    pub fn clear_all(&mut self, style: u64) {
        self.text.fill(' ');
        self.styles.fill(style);
    }

    pub fn set_char(&mut self, column: u64, code_point: u32, style: u64) {
        if (column as usize) < self.text.len() {
            self.text[column as usize] = std::char::from_u32(code_point).unwrap_or(' ');
            self.styles[column as usize] = style;
        }
    }

    pub fn insert_spaces(&mut self, column: u64, n: u64, style: u64) {
        let len = self.text.len();
        let col = column as usize;
        if col < len {
            let n = min(n as usize, len - col);
            self.text.copy_within(col..len - n, col + n);
            self.styles.copy_within(col..len - n, col + n);
            self.text[col..col + n].fill(' ');
            self.styles[col..col + n].fill(style);
        }
    }

    pub fn delete_characters(&mut self, column: u64, n: u64, style: u64) {
        let len = self.text.len();
        let col = column as usize;
        if col < len {
            let n = min(n as usize, len - col);
            self.text.copy_within(col + n..len, col);
            self.styles.copy_within(col + n..len, col);
            self.text[len - n..len].fill(' ');
            self.styles[len - n..len].fill(style);
        }
    }

    pub fn get_space_used(&self) -> u64 {
        // 使用 rfind 替代手写 reverse 循环，编译器更容易优化（可能自动向量化）
        self.text
            .iter()
            .rposition(|&c| c != ' ')
            .map(|i| (i + 1) as u64)
            .unwrap_or(0)
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
                if cur_col as u64 >= column {
                    return cur_idx as u64;
                }
                cur_col += width;
            } else {
                if cur_col as u64 >= column {
                    return cur_idx as u64;
                }
            }
            cur_idx += 1;
        }
        self.get_space_used()
    }

    pub fn get_selected_text(&self, x1: u64, x2: u64) -> String {
        let cols = self.text.len() as u64;
        if x1 >= cols {
            return String::new();
        }
        let end = min(x2, cols);
        self.text[x1 as usize..end as usize]
            .iter()
            .filter(|&&c| c != '\0')
            .collect()
    }

    pub fn get_word_at(&self, column: u64) -> String {
        let cols = self.text.len() as u64;
        if column >= cols {
            return String::new();
        }
        fn is_word(c: char) -> bool {
            c.is_alphanumeric() || c == '_'
        }
        if !is_word(self.text[column as usize]) {
            return String::new();
        }
        let mut s = column as usize;
        while s > 0 && is_word(self.text[s - 1]) {
            s -= 1;
        }
        let mut e = column as usize;
        while e + 1 < cols as usize && is_word(self.text[e + 1]) {
            e += 1;
        }
        self.text[s..=e].iter().collect()
    }
}

pub fn local_get_width(ucs: u32) -> usize {
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
        for _ in 0..t_u {
            b.push(TerminalRow::new_with_style(
                max(1, cols as u64),
                STYLE_NORMAL,
            ));
        }
        Self {
            rows,
            cols,
            buffer: b,
            first_row: 0,
            active_transcript_rows: 0,
        }
    }

    #[inline]
    pub fn internal_row(&self, row: i64) -> usize {
        let t = self.buffer.len() as i64;
        if t == 0 {
            return 0;
        }
        // Fast path: i64 checked_add avoids overflow while keeping native register width.
        let first = self.first_row as i64;
        if let Some(sum) = first.checked_add(row) {
            if sum >= 0 && sum < t {
                return sum as usize;
            }
        }
        // Slow path: i128 for absolute safety with extreme inputs.
        let t128 = self.buffer.len() as i128;
        let sum128 = self.first_row as i128 + row as i128;
        let idx = sum128 % t128;
        if idx < 0 {
            (idx + t128) as usize
        } else {
            idx as usize
        }
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
        let right = min(right, cols);
        for row in top..min(bottom, rows) {
            let idx = self.internal_row(row as i64);
            self.buffer[idx].clear(left, right, style);
        }
    }

    pub fn get_transcript_text(&self) -> String {
        let mut res = String::new();
        let first_y = -(self.active_transcript_rows as i64);
        for y in first_y..self.rows {
            let row = self.get_row(y);
            res.push_str(&row.get_selected_text(0, row.get_space_used()));
            if !row.line_wrap && y < self.rows - 1 {
                res.push('\n');
            }
        }
        res
    }

    pub fn get_selected_text(&self, x1: i64, y1: i64, x2: i64, y2: i64) -> String {
        let mut res = String::new();
        let (sy, sx, ey, ex) = if y1 < y2 || (y1 == y2 && x1 <= x2) {
            (y1, x1, y2, x2)
        } else {
            (y2, x2, y1, x1)
        };
        for y in sy..=ey {
            let row = self.get_row(y);
            let s_x = if y == sy { max(0, sx) as u64 } else { 0 };
            let mut e_x = if y == ey {
                min(self.cols, ex + 1) as u64
            } else {
                self.cols as u64
            };

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
            if y < ey && !row.line_wrap {
                res.push('\n');
            }
        }
        res
    }

    pub fn erase_in_display(&mut self, mode: i64, cursor_y: i64, cursor_x: i64, style: u64) {
        let c = self.cols as u64;
        match mode {
            0 => {
                // Erase from cursor to end of screen (including current row from cursor)
                self.get_row_mut(cursor_y).clear(cursor_x as u64, c, style);
                for y in (cursor_y + 1)..self.rows {
                    let idx = self.internal_row(y);
                    self.buffer[idx].clear(0, c, style);
                }
            }
            1 => {
                // Erase from start of screen to cursor (including current row up to cursor)
                for y in 0..cursor_y {
                    let idx = self.internal_row(y);
                    self.buffer[idx].clear(0, c, style);
                }
                self.get_row_mut(cursor_y)
                    .clear(0, (cursor_x + 1) as u64, style);
            }
            2 => {
                // Full screen clear - bypass clamping since y is always in [0, rows)
                for y in 0..self.rows {
                    let idx = self.internal_row(y);
                    self.buffer[idx].clear(0, c, style);
                }
            }
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
        for i in 0..to_insert {
            self.get_row_mut(cursor_y + i).clear_all(style);
        }
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
        for i in 0..to_delete {
            self.get_row_mut(bottom - i - 1).clear_all(style);
        }
    }

    pub fn scroll_up(&mut self, top: i64, bottom: i64, style: u64) {
        let total_rows = self.buffer.len();

        let block_copy_lines_down =
            |buf: &mut Vec<TerminalRow>, src_internal: usize, len: usize| {
                if len == 0 {
                    return;
                }
                let start = len - 1;
                for i in (0..=start).rev() {
                    let d = (src_internal + i + 1) % total_rows;
                    let s = (src_internal + i) % total_rows;
                    if s == d {
                        continue;
                    }
                    let (low, high) = if s < d { (s, d) } else { (d, s) };
                    let (left, right) = buf.split_at_mut(high);
                    std::mem::swap(&mut left[low], &mut right[0]);
                }
            };

        // Copy the fixed top margin lines one line down
        let top_margin_len = top as usize;
        block_copy_lines_down(&mut self.buffer, self.first_row as usize, top_margin_len);

        // Copy the fixed bottom margin lines one line down
        let bottom_margin_len = (self.rows - bottom) as usize;
        let bottom_src = self.internal_row(bottom);
        block_copy_lines_down(&mut self.buffer, bottom_src, bottom_margin_len);

        // Update the screen location in the ring buffer
        self.first_row = (self.first_row + 1) % (total_rows as u64);

        // Note that the history has grown if not already full
        let max_transcript_rows = total_rows as u64 - self.rows as u64;
        if self.active_transcript_rows < max_transcript_rows {
            self.active_transcript_rows += 1;
        }

        // Blank the newly revealed line above the bottom margin
        self.get_row_mut(bottom - 1).clear_all(style);
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
    pub fn resize_with_reflow(
        &mut self,
        new_cols: i32,
        new_rows: i32,
        current_style: u64,
        cursor_x: i32,
        cursor_y: i32,
    ) -> (i32, i32) {
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
            new_buffer.push(TerminalRow::new_with_style(n_cols as u64, current_style));
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
        let do_scroll = |first_row: &mut u64,
                         active: &mut u64,
                         sr: u64,
                         style: u64,
                         total: usize,
                         max_active: u64,
                         buf: &mut Vec<TerminalRow>| {
            // Java: mScreenFirstRow = (mScreenFirstRow + 1) % mTotalRows;
            *first_row = (*first_row + 1) % total as u64;
            // Java: if (mActiveTranscriptRows < mTotalRows - mScreenRows) mActiveTranscriptRows++;
            if *active < max_active {
                *active += 1;
            }
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
            // get_space_used() == 0 means entirely blank; > 0 guarantees at least one non-space
            let is_blank = old_line.get_space_used() == 0;

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
                        do_scroll(
                            &mut screen_first_row,
                            &mut new_active_transcript_rows,
                            screen_rows,
                            current_style,
                            new_total_rows,
                            max_transcript_rows,
                            &mut new_buffer,
                        );
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

            let just_to_cursor = cursor_at_this_row;

            // Process each character in the old line
            let mut i = 0u64;
            let mut current_old_col: usize = 0;
            let mut style_at_col = current_style;

            while i < last_non_space_index {
                // BUG FIX: Emulate Java's justToCursor early break
                // Stop processing trailing spaces on the cursor row once we pass the cursor
                if just_to_cursor
                    && i > cursor_x as u64
                    && last_non_space_index == old_line.text.len() as u64
                {
                    // Check if the rest of the line is actually empty
                    if old_line.text[(i as usize)..(last_non_space_index as usize)]
                        .iter()
                        .all(|&c| c == ' ')
                    {
                        break;
                    }
                }

                let c = old_line.text[i as usize];
                let code_point = c as u32;
                let display_width = local_get_width(code_point);

                // 核心修复：宽字符原子性检测
                // 如果当前是宽字符，检查下一个是否是 \0 占位符，并将它们作为一个整体处理
                let is_atomic_pair = display_width == 2
                    && (i as usize) + 1 < old_line.text.len()
                    && old_line.text[(i as usize) + 1] == '\0';
                let unit_width = if is_atomic_pair {
                    2
                } else {
                    display_width as usize
                };

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
                        if cursor_placed && new_cursor_y > 0 {
                            new_cursor_y -= 1;
                        }
                        do_scroll(
                            &mut screen_first_row,
                            &mut new_active_transcript_rows,
                            screen_rows,
                            current_style,
                            new_total_rows,
                            max_transcript_rows,
                            &mut new_buffer,
                        );
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
                    do_scroll(
                        &mut screen_first_row,
                        &mut new_active_transcript_rows,
                        screen_rows,
                        current_style,
                        new_total_rows,
                        max_transcript_rows,
                        &mut new_buffer,
                    );
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
    fn resize_rows_only(
        &mut self,
        new_rows: i32,
        cursor_x: i32,
        cursor_y: i32,
        current_style: u64,
    ) -> (i32, i32) {
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
                let row_is_blank = self.buffer[internal_row].get_space_used() == 0;
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
            let actual_shift =
                std::cmp::max(shift_down_of_top_row, -(self.active_transcript_rows as i32));

            if shift_down_of_top_row != actual_shift {
                // The new lines revealed by resizing are not all from transcript.
                // Blank the below ones.
                let blank_count = actual_shift - shift_down_of_top_row;

                for i in 0..blank_count {
                    let row_idx = (self.first_row as u128 + old_rows as u128 + i as u128)
                        % self.buffer.len() as u128;
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
            self.active_transcript_rows = self
                .active_transcript_rows
                .saturating_sub((-shift_i64) as u64);
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
        for _i in (0..self.rows).rev() {
            let internal_row = self.internal_row(_i);
            let line = &self.buffer[internal_row];
            let used = line.get_space_used();
            let is_blank = used == 0;
            if !is_blank {
                break;
            }
        }

        // 物理缩容逻辑：缓解大吞吐量输出后的内存膨胀
        if self.buffer.capacity() > self.buffer.len() * 2 && self.buffer.capacity() > 1000 {
            // 只有当空闲空间超过一倍且基数较大时，才触发昂贵的 shrink 操作
            self.buffer.shrink_to_fit();
            crate::utils::android_log(
                crate::utils::LogPriority::INFO,
                &format!(
                    "[Screen] Physical buffer reclaimed. New capacity: {}",
                    self.buffer.capacity()
                ),
            );
        }

        // 同时对每一行执行缩容检测（如果列数曾发生剧烈变动）
        for row in self.buffer.iter_mut() {
            if row.text.capacity() > row.text.len() + 16 {
                row.text.shrink_to_fit();
                row.styles.shrink_to_fit();
            }
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
        r.text[0] = 'A';
        r.styles[0] = 1;
        r.text[2] = 'B';
        r.styles[2] = 2;

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
            s.get_row_mut(r as i64)
                .set_char(0, (b'A' + r as u8) as u32, 0);
        }

        s.erase_in_display(2, 0, 0, 0); // erase all (mode=2, cursor at 0,0)

        for r in 0..5 {
            assert_eq!(
                s.get_row(r as i64).text[0],
                ' ',
                "Row {} should be cleared",
                r
            );
        }
    }

    #[test]
    fn test_screen_erase_below_cursor() {
        let mut s = Screen::new(10, 5, 5);
        for r in 0..5 {
            s.get_row_mut(r as i64)
                .set_char(0, (b'A' + r as u8) as u32, 0);
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
            s.get_row_mut(r as i64)
                .set_char(0, (b'A' + r as u8) as u32, 0);
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
            s.get_row_mut(r as i64)
                .set_char(0, (b'A' + r as u8) as u32, 0);
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
            s.get_row_mut(0)
                .set_char(i as u64, (b'A' + i as u8) as u32, 0);
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
            s.get_row_mut(0)
                .set_char(i as u64, (b'A' + i as u8) as u32, 0);
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
        assert_eq!(
            r.get_space_used(),
            2,
            "Space used should account for wide char placeholder"
        );
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

        let (_nx, ny) = s.resize_with_reflow(10, 5, 0, 0, 3);

        // The cursor position should be preserved relative to the content.
        // If blank lines are skipped and not accounted for, ny might become 1.
        assert_eq!(
            ny, 3,
            "Cursor Y should be preserved even if preceding lines are blank"
        );
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

    // =====================================================================
    // 缩放/resize 正确性断言 — 针对 "反复缩放时内容堆叠" 的回归测试
    // =====================================================================

    /// 辅助：检查可见区域内是否存在两行映射到同一个物理 TerminalRow
    fn assert_no_duplicate_visible_rows(screen: &Screen) {
        let mut ptrs = std::collections::HashSet::new();
        for r in 0..screen.rows {
            let row_ref = screen.get_row(r);
            let ptr = row_ref as *const TerminalRow as usize;
            assert!(
                ptrs.insert(ptr),
                "可见行 {} 和 {} 映射到同一个物理行 (first_row={}, rows={}, active_transcript_rows={}, buf_len={})",
                r,
                ptrs.len() - 1,
                screen.first_row,
                screen.rows,
                screen.active_transcript_rows,
                screen.buffer.len()
            );
        }
    }

    /// 辅助：检查整个缓冲区范围内 (含历史) 的 get_row 唯一性
    fn assert_all_rows_unique(screen: &Screen) {
        let min_row = -(screen.active_transcript_rows as i64);
        let max_row = screen.rows - 1;
        let total = (max_row - min_row + 1) as usize;
        let mut ptrs = std::collections::HashSet::with_capacity(total);
        for r in min_row..=max_row {
            let row_ref = screen.get_row(r);
            let ptr = row_ref as *const TerminalRow as usize;
            assert!(
                ptrs.insert(ptr),
                "逻辑行 {} 映射到已存在的物理行 (first_row={}, rows={}, active={})",
                r,
                screen.first_row,
                screen.rows,
                screen.active_transcript_rows
            );
        }
    }

    #[test]
    fn test_repeated_resize_rows_only_no_duplicate_rows() {
        let mut s = Screen::new(80, 25, 100);
        // 给可见区域每行写入不同标记，方便识别
        for r in 0..25 {
            s.get_row_mut(r).set_char(0, (b'A' + r as u8) as u32, 0);
        }
        // 模拟反复缩放：先缩到 15 行，再扩到 30，再缩到 20，再扩到 25
        s.resize_rows_only(15, 0, 24, 0);
        assert_no_duplicate_visible_rows(&s);

        s.resize_rows_only(30, 0, 14, 0);
        assert_no_duplicate_visible_rows(&s);

        s.resize_rows_only(20, 0, 19, 0);
        assert_no_duplicate_visible_rows(&s);

        s.resize_rows_only(25, 0, 19, 0);
        assert_no_duplicate_visible_rows(&s);

        // 更激进的反复缩放
        for rows in [10, 40, 5, 50, 25] {
            s.resize_rows_only(rows, 0, (rows - 1).max(0), 0);
            assert_no_duplicate_visible_rows(&s);
            assert_all_rows_unique(&s);
        }
    }

    #[test]
    fn test_repeated_resize_reflow_no_duplicate_rows() {
        let mut s = Screen::new(80, 25, 100);
        for r in 0..25 {
            s.get_row_mut(r).set_char(0, (b'A' + r as u8) as u32, 0);
        }
        // 反复改变列数（触发慢路径 reflow）
        let sizes = [
            (40, 25),
            (80, 25),
            (20, 25),
            (100, 25),
            (80, 20),
            (80, 30),
            (80, 25),
        ];
        for (cols, rows) in sizes {
            s.resize_with_reflow(cols, rows, 0, 0, 0);
            assert_no_duplicate_visible_rows(&s);
            assert_all_rows_unique(&s);
        }
    }

    #[test]
    fn test_resize_rows_only_invariants() {
        let mut s = Screen::new(80, 10, 20);
        // 写满可见区并产生 5 行历史
        for r in 0..15 {
            s.get_row_mut(r)
                .set_char(0, (b'0' + (r % 10) as u8) as u32, 0);
            if r >= 10 {
                s.scroll_up(0, 10, 0);
            }
        }
        let orig_active = s.active_transcript_rows;
        assert!(orig_active > 0);

        // 缩到 5 行
        s.resize_rows_only(5, 0, 4, 0);
        assert!(
            s.active_transcript_rows >= orig_active,
            "缩小时历史行不应丢失"
        );
        assert!(s.active_transcript_rows <= (s.buffer.len() as u64).saturating_sub(s.rows as u64));
        assert_no_duplicate_visible_rows(&s);

        // 扩到 15 行
        s.resize_rows_only(15, 0, 4, 0);
        assert!(s.active_transcript_rows <= (s.buffer.len() as u64).saturating_sub(s.rows as u64));
        assert_no_duplicate_visible_rows(&s);
    }

    #[test]
    fn test_resize_reflow_tilde_dollar_content_integrity() {
        // 模拟 "~$" 提示符 + 一段长文本，resize 后不应出现重复行
        let mut s = Screen::new(40, 10, 20);
        let input = "~$ echo hello world";
        let mut col = 0;
        for c in input.chars() {
            s.get_row_mut(0).set_char(col, c as u32, 0);
            col += 1;
        }

        // 缩到 10 列，内容应该换行，但不应产生重复行
        s.resize_with_reflow(10, 10, 0, 0, 0);
        assert_no_duplicate_visible_rows(&s);

        // 再扩回 40 列
        s.resize_with_reflow(40, 10, 0, 0, 0);
        assert_no_duplicate_visible_rows(&s);

        // 验证首行内容未被截断或重复
        let first_row = s.get_row(0);
        let text: String = first_row.text.iter().take(input.len()).collect();
        assert!(
            text.starts_with("~$"),
            "resize 后首行应仍保留 ~$ 提示符，实际得到: {:?}",
            text
        );
    }

    #[test]
    fn test_resize_reflow_long_content_no_stacking() {
        // 扩大字数：写入多行长内容，模拟真实终端输出后反复缩放
        let mut s = Screen::new(80, 24, 100);
        let lines = [
            "~$ cargo build --release --verbose --target aarch64-linux-android",
            "   Compiling termux-rust v0.1.0 (/home/termux)",
            "    Finished release [optimized] target(s) in 11.46s",
            "~$ ./gradlew :termux-app:assembleDebug",
            "BUILD SUCCESSFUL in 40s",
            "~$ echo '这是一段中文测试内容，用于验证宽字符在 resize 后的完整性'",
            "这是一段中文测试内容，用于验证宽字符在 resize 后的完整性",
        ];
        for (row_idx, line) in lines.iter().enumerate() {
            let mut col = 0;
            for c in line.chars() {
                if col >= 80 {
                    break;
                }
                s.get_row_mut(row_idx as i64)
                    .set_char(col as u64, c as u32, 0);
                col += crate::wcwidth::wcwidth(c as u32) as usize;
            }
        }

        // 反复缩放列数，模拟用户双指缩放导致列数频繁变化
        let col_sizes = [80, 40, 20, 10, 5, 10, 20, 40, 80, 120, 80];
        for (i, &cols) in col_sizes.iter().enumerate() {
            s.resize_with_reflow(cols, 24, 0, 0, 0);
            assert_no_duplicate_visible_rows(&s);
            assert_all_rows_unique(&s);

            // 额外断言：检查可见区域内没有连续两行"同时为非空且完全相同"
            // 空白行连续是正常的，但非空内容重复才是真正的"堆叠"
            for r in 1..s.rows {
                let prev = s.get_row(r - 1);
                let curr = s.get_row(r);
                let prev_text: String = prev.text.iter().collect();
                let curr_text: String = curr.text.iter().collect();
                let prev_trimmed = prev_text.trim_end();
                let curr_trimmed = curr_text.trim_end();
                if !prev_trimmed.is_empty() && !curr_trimmed.is_empty() {
                    assert_ne!(
                        prev_trimmed,
                        curr_trimmed,
                        "第 {} 次 resize 后，行 {} 和行 {} 非空内容完全相同，疑似堆叠 (cols={})",
                        i,
                        r - 1,
                        r,
                        cols
                    );
                }
            }
        }
    }

    #[test]
    fn test_resize_rows_only_long_history() {
        // 扩大历史行数和内容长度，测试 resize_rows_only 在大量历史下的稳定性
        let mut s = Screen::new(80, 10, 50);
        // 写满 50 行，产生 40 行历史
        for r in 0..50 {
            let content = format!(
                "Line {:03}: This is a test line with enough length to fill most columns.",
                r
            );
            let mut col = 0;
            for c in content.chars() {
                if col >= 80 {
                    break;
                }
                s.get_row_mut(r).set_char(col as u64, c as u32, 0);
                col += 1;
            }
            if r >= 10 {
                s.scroll_up(0, 10, 0);
            }
        }

        assert!(s.active_transcript_rows > 0, "应产生历史行");

        // 反复改变行数
        for rows in [10, 5, 20, 50, 10, 30, 10] {
            s.resize_rows_only(rows, 0, (rows - 1).min(s.rows as i32 - 1).max(0), 0);
            assert_no_duplicate_visible_rows(&s);
            assert_all_rows_unique(&s);
            assert!(
                s.active_transcript_rows <= (s.buffer.len() as u64).saturating_sub(s.rows as u64),
                "rows={} 时 active_transcript_rows 越界",
                rows
            );
        }
    }

    #[test]
    fn test_resize_aggressive_stress() {
        // 压力测试：100 次随机 resize，每次检查不变量
        let mut s = Screen::new(80, 24, 100);
        for r in 0..24 {
            for c in 0..80 {
                s.get_row_mut(r)
                    .set_char(c as u64, ((c + r * 80) % 95 + 32) as u32, 0);
            }
        }
        let mut rng = 12345u32;
        let sizes = [(80, 24), (40, 12), (120, 48), (20, 6), (80, 50), (80, 24)];
        for i in 0..100 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let idx = (rng % sizes.len() as u32) as usize;
            let (cols, rows) = sizes[idx];
            if cols == 80 && s.rows == rows as i64 {
                // 快速路径
                s.resize_rows_only(rows, 0, (rows - 1).min(s.rows as i32 - 1).max(0), 0);
            } else {
                s.resize_with_reflow(cols, rows, 0, 0, 0);
            }
            assert_no_duplicate_visible_rows(&s);
            assert!(
                s.active_transcript_rows <= (s.buffer.len() as u64).saturating_sub(s.rows as u64),
                "第 {} 次 resize 后 active_transcript_rows 越界",
                i
            );
            assert!(s.rows > 0, "rows 不应为 0 或负数");
            assert!(s.cols > 0, "cols 不应为 0 或负数");
        }
    }
}
