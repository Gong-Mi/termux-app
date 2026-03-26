// 测试：验证内容是否正确写入历史缓冲区
use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ").trim().to_string()
}

#[test]
fn test_content_goes_beyond_screen() {
    // 创建一个 80x24 的终端，总缓冲区 1000 行
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    println!("Initial state:");
    println!("  cols={}, rows={}, active_transcript={}", 
             engine.state.cols, engine.state.rows, engine.state.main_screen.active_transcript_rows);
    
    // 写入 100 行内容（超过屏幕的 24 行）
    for i in 1..=100 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    println!("\nAfter writing 100 lines:");
    println!("  active_transcript={}", engine.state.main_screen.active_transcript_rows);
    
    // 检查屏幕内容（行 0-23）
    println!("\nScreen content (rows 0-23):");
    for i in 0..engine.state.rows {
        let text = get_row_text(&engine, i);
        if !text.is_empty() {
            println!("  [{}]: {}", i, text);
        }
    }
    
    // 检查历史内容（行 -active_transcript 到 -1）
    println!("\nHistory content (sample):");
    let active = engine.state.main_screen.active_transcript_rows as i32;
    for i in (-active..0).step_by(10) {
        let text = get_row_text(&engine, i);
        if !text.is_empty() {
            println!("  [{}]: {}", i, text);
        }
    }
    
    // 验证：应该能看到 Line 1 在历史中
    let mut found_line_1 = false;
    for i in (-active..engine.state.rows) {
        if get_row_text(&engine, i).contains("Line 1") {
            found_line_1 = true;
            println!("\n✓ Found 'Line 1' at row {}", i);
            break;
        }
    }
    
    // 验证：最后几行应该在屏幕上
    // 由于有 77 行历史和 24 行屏幕，Line 100 应该在屏幕底部附近
    let mut found_line_100 = false;
    for i in 0..engine.state.rows {
        if get_row_text(&engine, i).contains("Line 100") {
            found_line_100 = true;
            println!("Found 'Line 100' at screen row {}", i);
            break;
        }
    }
    
    println!("\nVerification:");
    println!("  Line 1 in history: {}", found_line_1);
    println!("  Line 100 on screen: {}", found_line_100);
    
    assert!(found_line_1, "Line 1 should be in history");
    assert!(found_line_100, "Line 100 should be on screen");
}

#[test]
fn test_resize_preserves_history() {
    // 创建一个 80x24 的终端
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    // 写入 50 行
    for i in 1..=50 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    
    let history_before = engine.state.main_screen.active_transcript_rows;
    println!("Before resize: active_transcript={}", history_before);
    
    // 缩小到 12 行
    engine.state.resize(80, 12);
    
    let history_after = engine.state.main_screen.active_transcript_rows;
    println!("After resize to 12 rows: active_transcript={}", history_after);
    
    // 验证历史内容仍然存在
    let mut found_line_1 = false;
    let active = engine.state.main_screen.active_transcript_rows as i32;
    for i in (-active..engine.state.rows) {
        if get_row_text(&engine, i).contains("Line 1") {
            found_line_1 = true;
            println!("Found 'Line 1' at row {}", i);
            break;
        }
    }
    
    assert!(found_line_1, "Line 1 should still exist after resize");
}
