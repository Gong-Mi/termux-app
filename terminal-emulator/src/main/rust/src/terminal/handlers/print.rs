use crate::engine::ScreenState;
use crate::utils::map_line_drawing;
use crate::terminal::modes;

pub fn handle_print(state: &mut ScreenState, c: char) {
    handle_print_internal(state, c);
}

/// 批量打印字符流入口 - 精简版（消除预扫描开销）
pub fn handle_print_str(state: &mut ScreenState, s: &str) {
    // 记录最后一个字符用于状态追踪（只需一次迭代）
    let mut last_c = None;
    for c in s.chars() {
        handle_print_internal(state, c);
        last_c = Some(c);
    }
    if let Some(c) = last_c {
        state.last_printed_char = Some(c);
    }
}

fn handle_print_internal(state: &mut ScreenState, c: char) {
    // 1. 字符映射
    let c = if (c as u32) >= 0x20 && (c as u32) <= 0x7E {
        if state.use_line_drawing_uses_g0 && state.use_line_drawing_g0 {
            map_line_drawing(c as u8)
        } else if !state.use_line_drawing_uses_g0 && state.use_line_drawing_g1 {
            map_line_drawing(c as u8)
        } else {
            c
        }
    } else {
        c
    };

    let ucs = c as u32;
    let char_width = crate::utils::get_char_width(ucs) as i32;
    if char_width <= 0 {
        return;
    }

    // 2. 处理自动换行 (Auto-Wrap)
    // 如果光标已经在最后一列之后，且当前又要打印字符，则触发换行
    if state.auto_wrap() {
        let columns = state.cols;
        let mut wrap_needed = state.cursor.about_to_wrap;
        
        // 核心修复：宽字符在最后一列时必须提前换行
        if char_width == 2 && state.cursor.x >= columns - 1 {
            wrap_needed = true;
        }

        if wrap_needed {
            let y = state.cursor.y;
            {
                let screen = state.get_current_screen_mut();
                let y_wrapped = screen.internal_row(y);
                screen.buffer[y_wrapped].line_wrap = true;
            }

            state.cursor.x = state.left_margin;
            if state.cursor.y < state.bottom_margin - 1 {
                state.cursor.y += 1;
            } else {
                state.scroll_up();
            }
            state.cursor.about_to_wrap = false;
        }
    }

    // 3. 处理插入模式 (Insert Mode - CSI 4 h)
    // 如果开启了插入模式，新字符会把当前行后面的内容往右推
    if state.modes.is_enabled(modes::MODE_INSERT) {
        state.insert_characters(char_width);
    }

    // 4. 写入缓冲区
    // 确保光标在有效范围内
    let columns = state.cols;
    if state.cursor.x >= columns {
        state.cursor.x = columns - 1;
    }

    let y = state.cursor.y;
    let x = state.cursor.x as usize;
    let style = state.current_style;
    let right_margin = state.right_margin;

    {
        let screen = state.get_current_screen_mut();
        let y_internal = screen.internal_row(y);
        let row = &mut screen.buffer[y_internal];
        
        // 如果是宽字符，且会超出右边界，则不打印（或截断）
        if char_width == 2 && (x as i32) >= right_margin - 1 {
            // 在 Java 版中，这会强制触发换行，我们这里的逻辑已在上方处理
        }

        // 执行写入
        if x < row.text.len() {
            row.text[x] = c;
            row.styles[x] = style;
            if char_width == 2 && x + 1 < row.text.len() {
                row.text[x + 1] = '\0'; // 宽字符占位符
                row.styles[x + 1] = style;
            }
        }
    }

    // 5. 更新光标位置
    if state.cursor.x + char_width >= state.right_margin {
        // 到达或超过边界：停留在最后一列并标记 about_to_wrap
        state.cursor.x = state.right_margin - char_width;
        state.cursor.about_to_wrap = true;
    } else {
        state.cursor.x += char_width;
        state.cursor.about_to_wrap = false;
    }
}

