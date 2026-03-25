use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

#[test]
fn test_reflow_stress_600_lines() {
    // 1. 初始化一个足够大的引擎，总行数 5000
    // 因为 600 个长行在 15 宽时约占 3600 物理行
    let mut engine = TerminalEngine::new(80, 24, 5000, 10, 20);
    
    println!("--- Step 1: Writing 600 lines of numbered content ---");
    for i in 1..=600 {
        // 写入带编号的行，并手动换行
        let content = format!("Test Content Row {:03} - This is a long line for testing reflow stability.\r\n", i);
        engine.process_bytes(content.as_bytes());
    }

    // 2. 检查缩放前的状态
    println!("Before Resize: Cols={}, Rows={}, Active History={}",
             engine.state.cols, engine.state.rows, engine.state.main_screen.active_transcript_rows);

    // 打印缩小前的历史行样本
    println!("--- Sample rows BEFORE shrink ---");
    let before_history = engine.state.main_screen.active_transcript_rows as i32;
    for i in (-before_history..0).step_by(500) {
        let text = get_row_text(&engine, i);
        if !text.trim().is_empty() {
            println!("Before[{}]: '{}'", i, text.trim());
        } else {
            println!("Before[{}]: (empty)", i);
        }
    }
    
    // 3. 执行极端缩小测试 (80 -> 15)
    println!("--- Step 2: Extreme Shrinking (80 -> 15) ---");
    engine.state.resize(15, 50); // 缩小宽度，增加高度

    // 打印缩小后的历史行样本
    println!("--- Sample rows AFTER shrink ---");
    let after_history = engine.state.main_screen.active_transcript_rows as i32;
    for i in (-after_history..0).step_by(500) {
        let text = get_row_text(&engine, i);
        if !text.trim().is_empty() {
            println!("After[{}]: '{}'", i, text.trim());
        } else {
            println!("After[{}]: (empty)", i);
        }
    }
    
    println!("New State: Cols={}, Rows={}, Active History={}", 
             engine.state.cols, engine.state.rows, engine.state.main_screen.active_transcript_rows);

    // 打印逻辑行末尾的 10 行 (此时应该在屏幕范围内)
    println!("--- Logic Rows Near End ---");
    for i in (engine.state.rows - 10)..engine.state.rows {
        let t = get_row_text(&engine, i);
        println!("Logic Row[{}]: '{}'", i, t.trim());
    }

    // 验证重排后的内容：拼接所有行以应对内容被拆分的情况
    let mut all_text = String::new();
    let start_row = -(engine.state.main_screen.active_transcript_rows as i32);
    for i in start_row..engine.state.rows {
        all_text.push_str(&get_row_text(&engine, i));
    }

    let found_600 = all_text.contains("Row 600");
    assert!(found_600, "Content 'Row 600' must exist somewhere in the combined reflowed text");
    println!("SUCCESS: 'Row 600' found in combined text after extreme shrink.");

    // 4. 执行极端放大测试 (15 -> 150)
    println!("--- Step 3: Extreme Expanding (15 -> 150) ---");
    engine.state.resize(150, 24);

    // 打印放大后的状态
    println!("After Expand: Cols={}, Rows={}, Active History={}",
             engine.state.cols, engine.state.rows, engine.state.main_screen.active_transcript_rows);

    // 5. 验证内容是否重新合并
    let mut all_text_expanded = String::new();
    let start_row_exp = -(engine.state.main_screen.active_transcript_rows as i32);
    for i in start_row_exp..engine.state.rows {
        all_text_expanded.push_str(&get_row_text(&engine, i));
    }

    assert!(all_text_expanded.contains("Row 600"), "Content 'Row 600' must exist after expanding");
    println!("SUCCESS: 'Row 600' preserved after re-expanding.");

    // 验证中间内容 (比如第 300 行)
    // 注意：由于 resize 会导致历史行偏移，我们寻找包含 "Row 300" 的行
    let mut found_300 = false;
    let total_active = engine.state.main_screen.active_transcript_rows as i32;
    println!("Searching for 'Row 300' in range [{}, {})", -total_active, engine.state.rows);
    
    // 打印最后 10 行和中间几行用于调试
    println!("--- Last 10 rows ---");
    for i in (engine.state.rows - 10)..engine.state.rows {
        let text = get_row_text(&engine, i);
        if !text.trim().is_empty() {
            println!("Row[{}]: '{}'", i, text.trim());
        }
    }
    
    println!("--- Sample rows from history ---");
    for i in (-total_active..0).step_by(300) {
        let text = get_row_text(&engine, i);
        if !text.trim().is_empty() {
            println!("Row[{}]: '{}'", i, text.trim());
        }
    }
    
    for i in -(total_active)..engine.state.rows {
        if get_row_text(&engine, i).contains("Row 300") {
            found_300 = true;
            println!("Found 'Row 300' at row {}", i);
            break;
        }
    }
    assert!(found_300, "Content 'Row 300' should still exist in memory after massive reflows");
    
    println!("Test passed: 600 lines successfully handled during massive screen resize/reflow.");
}
