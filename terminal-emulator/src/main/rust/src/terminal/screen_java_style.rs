// Java 风格的 resize_with_reflow 实现
// 复刻 TerminalBuffer.java 的逻辑

pub fn resize_with_reflow_java_style(
    &mut self,
    new_cols: i32,
    new_rows: i32,
    new_total_rows: usize,
    current_style: u64,
    cursor_x: i32,
    cursor_y: i32,
    alt_screen: bool,
) -> (i32, i32) {
    let old_cols = self.cols;
    let old_rows = self.rows;
    let old_total = self.buffer.len();
    let old_active_transcript = self.active_transcript_rows;
    let old_first_row = self.first_row;

    // ========== 快速路径：仅行数变化 ==========
    if new_cols == old_cols && new_rows <= old_total as i32 {
        // 计算顶部行的下移量（类似 Java 的 shiftDownOfTopRow）
        let mut shift_down = old_rows - new_rows;

        if shift_down > 0 && shift_down < old_rows {
            // 缩小：检查底部是否有空行可以跳过
            for i in (0..old_rows).rev() {
                if cursor_y >= i {
                    break;
                }
                let internal_row = self.internal_row(i as i32);
                if self.buffer[internal_row].is_blank() {
                    shift_down -= 1;
                    if shift_down == 0 {
                        break;
                    }
                }
            }
        } else if shift_down < 0 {
            // 扩展：只有当有历史记录时才移动
            let actual_shift = shift_down.max(-(old_active_transcript as i32));
            if shift_down != actual_shift {
                // 新暴露的行不全是历史记录，清空下面的行
                let blank_count = (actual_shift - shift_down) as usize;
                for i in 0..blank_count {
                    let row_idx = (old_first_row + old_rows + i) % old_total;
                    self.buffer[row_idx].clear_all(current_style);
                }
                shift_down = actual_shift;
            }
        }

        // 应用 shift
        let mut new_first_row = old_first_row as i32 + shift_down;
        if new_first_row < 0 {
            new_first_row += old_total as i32;
        } else {
            new_first_row %= old_total as i32;
        }

        self.first_row = new_first_row as usize;
        
        // 更新 active_transcript_rows（备用屏幕总是 0）
        self.active_transcript_rows = if alt_screen {
            0
        } else {
            (old_active_transcript as i32 + shift_down).max(0) as usize
        };

        // 调整光标位置
        let new_cursor_y = cursor_y - shift_down;

        // 更新屏幕行数
        self.rows = new_rows;

        return (cursor_x, new_cursor_y);
    }

    // ========== 慢速路径：列数变化，需要重排 ==========
    // 复刻 Java 的慢速路径逻辑

    // 创建新缓冲区
    let mut new_buffer: Vec<TerminalRow> = Vec::with_capacity(new_total_rows);
    for _ in 0..new_total_rows {
        new_buffer.push(TerminalRow::new(new_cols as usize));
    }

    let mut new_cursor_x: i32 = 0;
    let mut new_cursor_y: i32 = 0;
    let mut cursor_placed = false;

    // 输出位置追踪
    let mut output_row: i32 = 0;
    let mut output_col: i32 = 0;

    // 跳过的空行计数
    let mut skipped_blank_lines: i32 = 0;

    // 遍历旧缓冲区的每一行
    for external_old_row in -(old_active_transcript as i32)..old_rows {
        // 计算内部行索引（复刻 Java 逻辑）
        let mut internal_old_row = old_first_row as i32 + external_old_row;
        if internal_old_row < 0 {
            internal_old_row += old_total as i32;
        } else {
            internal_old_row %= old_total as i32;
        }
        let internal_old_row = internal_old_row as usize;

        let old_line = &self.buffer[internal_old_row];
        let cursor_at_this_row = external_old_row == cursor_y;

        // 检查是否为空行（复刻 Java 逻辑）
        let is_blank = old_line.is_blank();

        // 跳过空行（除非光标在这一行）
        if (old_line.text.is_empty() || is_blank) && !cursor_at_this_row {
            skipped_blank_lines += 1;
            continue;
        } else if skipped_blank_lines > 0 {
            // 遇到非空行，插入跳过的空行
            for _ in 0..skipped_blank_lines {
                if output_row == new_rows - 1 {
                    // 滚动（复刻 Java 的 scrollDownOneLine）
                    if cursor_placed && new_cursor_y > 0 {
                        new_cursor_y -= 1;
                    }
                    // 在环形缓冲区中滚动
                    let last_line = new_buffer[new_total_rows - 1].clone();
                    for i in (1..new_total_rows).rev() {
                        new_buffer[i] = new_buffer[i - 1].clone();
                    }
                    new_buffer[0] = last_line;
                    new_buffer[0].clear_all(current_style);
                } else {
                    output_row += 1;
                }
                output_col = 0;
            }
            skipped_blank_lines = 0;
        }

        // 确定要处理的字符范围
        let last_non_space_index = if cursor_at_this_row || old_line.line_wrap {
            old_line.text.len()
        } else {
            // 找到最后一个非空格字符
            old_line.get_space_used()
        };

        let just_to_cursor = cursor_at_this_row;

        // 处理每个字符
        let mut current_old_col: i32 = 0;
        let mut style_at_col = current_style;

        for i in 0..last_non_space_index {
            let c = old_line.text[i];
            let code_point = c as u32;
            let display_width = local_get_width(code_point) as i32;

            // 更新样式（仅对有宽度的字符）
            if display_width > 0 && current_old_col < old_cols {
                style_at_col = old_line.styles[current_old_col as usize];
            }

            // 处理换行
            if output_col + display_width > new_cols {
                if output_row < new_buffer.len() as i32 {
                    new_buffer[output_row as usize].line_wrap = true;
                }
                if output_row == new_rows - 1 {
                    // 滚动
                    if cursor_placed && new_cursor_y > 0 {
                        new_cursor_y -= 1;
                    }
                    let last_line = new_buffer[new_total_rows - 1].clone();
                    for i in (1..new_total_rows).rev() {
                        new_buffer[i] = new_buffer[i - 1].clone();
                    }
                    new_buffer[0] = last_line;
                    new_buffer[0].clear_all(current_style);
                } else {
                    output_row += 1;
                }
                output_col = 0;
            }

            // 处理组合字符
            let offset = if display_width <= 0 && output_col > 0 {
                1
            } else {
                0
            };
            let output_column = output_col - offset;

            // 设置字符
            if output_column >= 0 && output_column < new_cols && output_row >= 0 {
                new_buffer[output_row as usize].text[output_column as usize] = c;
                if current_old_col >= 0 && current_old_col < old_cols {
                    new_buffer[output_row as usize].styles[output_column as usize] = style_at_col;
                }
            }

            // 追踪光标位置
            if cursor_at_this_row 
                && current_old_col == cursor_x 
                && !cursor_placed 
            {
                new_cursor_x = output_col;
                new_cursor_y = output_row;
                cursor_placed = true;
            }

            if display_width > 0 {
                current_old_col += display_width;
                output_col += display_width;

                // 如果只需要复制到光标位置
                if just_to_cursor && cursor_placed {
                    break;
                }
            }
        }

        // 如果旧行没有换行，需要插入换行
        if external_old_row != (old_rows - 1) && !old_line.line_wrap {
            if output_row == new_rows - 1 {
                // 滚动
                if cursor_placed && new_cursor_y > 0 {
                    new_cursor_y -= 1;
                }
                let last_line = new_buffer[new_total_rows - 1].clone();
                for i in (1..new_total_rows).rev() {
                    new_buffer[i] = new_buffer[i - 1].clone();
                }
                new_buffer[0] = last_line;
                new_buffer[0].clear_all(current_style);
            } else {
                output_row += 1;
            }
            output_col = 0;
        }
    }

    // 处理光标超出屏幕
    if !cursor_placed || new_cursor_x < 0 || new_cursor_y < 0 {
        new_cursor_x = 0;
        new_cursor_y = 0;
    }

    // 应用新缓冲区
    self.buffer = new_buffer;
    self.cols = new_cols;
    self.rows = new_rows;
    self.first_row = 0;
    // 计算 active_transcript_rows（复刻 Java 逻辑）
    self.active_transcript_rows = (output_row as usize).saturating_sub(new_rows as usize);

    (new_cursor_x, new_cursor_y)
}
