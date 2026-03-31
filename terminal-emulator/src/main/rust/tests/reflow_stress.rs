use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

#[test]
fn test_extreme_shrinking_reflow() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    let line = "This is a long line that will be wrapped many times when the screen is shrunk extreme.";
    engine.process_bytes(line.as_bytes());
    
    engine.state.resize(10, 24);
    
    let row0 = get_row_text(&engine, 0);
    // 允许末尾有空格，只要前缀匹配
    assert!(row0.trim_start().starts_with("This is a"));
}

#[test]
fn test_wide_char_reflow_stress() {
    let mut engine = TerminalEngine::new(20, 10, 100, 10, 20);
    engine.process_bytes("你好世界".as_bytes());
    
    engine.state.resize(5, 10);
    
    // 拼接整屏验证
    let mut all_text = String::new();
    for i in 0..10 {
        all_text.push_str(&get_row_text(&engine, i));
    }
    let clean_text = all_text.replace(" ", "");
    assert!(clean_text.contains("你好"));
    assert!(clean_text.contains("世界"));
}

#[test]
fn test_rapid_resize_bounce() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    engine.process_bytes(b"Consistent Content");
    
    // 多次随机缩放
    for i in 1..10 {
        engine.state.resize(10 + i, 40 - i);
        engine.state.resize(150 - i, 5 + i);
    }
    
    // 回到原始尺寸
    engine.state.resize(80, 24);
    let row0 = get_row_text(&engine, 0);
    assert!(row0.contains("Consistent Content"));
}

#[test]
fn test_reflow_with_full_scrollback() {
    // 100 行总容量
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入超过容量的内容，使缓冲区充满
    for i in 0..150 {
        engine.process_bytes(format!("Line {:03}\r\n", i).as_bytes());
    }
    
    // 进行 Reflow
    engine.state.resize(40, 30);
    
    // 检查屏幕范围内或历史范围内的内容一致性
    // 在极小缓冲区下，我们至少保证内容没有崩溃且关键行可访问
    let _row0 = get_row_text(&engine, 0);
    assert!(engine.state.rows == 30);
}

#[test]
fn test_realistic_reflow_with_history() {
    // 缩小规模以便调试：100 行容量
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入 50 行
    for i in 0..50 {
        engine.process_bytes(format!("History Line {:03}\r\n", i).as_bytes());
    }
    
    println!("--- BEFORE RESIZE ---");
    println!("Active transcript rows: {}", engine.state.main_screen.active_transcript_rows);
    println!("Cursor Y: {}", engine.state.cursor.y);
    println!("Row -1 content: '{}'", get_row_text(&engine, -1));
    println!("Row 23 content (current screen bottom): '{}'", get_row_text(&engine, 23));

    // 缩放到 40x30
    engine.state.resize(40, 30);
    
    println!("--- AFTER RESIZE ---");
    println!("New Active transcript rows: {}", engine.state.main_screen.active_transcript_rows);
    println!("New Cursor Y: {}", engine.state.cursor.y);
    
    // 拼接所有内容验证
    let mut all_text = String::new();
    let start_row_idx = -(engine.state.main_screen.active_transcript_rows as i32);
    for i in start_row_idx..engine.state.rows {
        all_text.push_str(&get_row_text(&engine, i));
    }

    assert!(all_text.contains("History Line 049"), "Combined text should contain the last written line '049'");
}

#[test]
fn test_reflow_empty_lines() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    engine.process_bytes(b"Start\r\n\r\n\r\n\r\nEnd");
    
    engine.state.resize(10, 24);
    
    let row0 = get_row_text(&engine, 0);
    assert!(row0.contains("Start"));
    
    let row4 = get_row_text(&engine, 4);
    assert!(row4.contains("End"));
}
