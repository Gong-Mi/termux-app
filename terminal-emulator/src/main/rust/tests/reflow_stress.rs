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
    assert!(row0.starts_with("This is a"));
}

#[test]
fn test_wide_char_reflow_stress() {
    let mut engine = TerminalEngine::new(20, 10, 100, 10, 20);
    engine.process_bytes("你好世界".as_bytes());
    
    engine.state.resize(5, 10);
    
    let row0 = get_row_text(&engine, 0);
    // 宽字符在存储中带有占位空格，因此匹配单个字符或去掉空格后匹配
    let clean_row0 = row0.replace(" ", "");
    assert!(clean_row0.contains("你好"));
    
    let row1 = get_row_text(&engine, 1);
    let clean_row1 = row1.replace(" ", "");
    assert!(clean_row1.contains("世界"));
}

#[test]
fn test_rapid_resize_bounce() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    engine.process_bytes(b"Consistent Content");
    
    for i in 1..20 {
        engine.state.resize(10 + i, 40 - i);
        engine.state.resize(150 - i, 5 + i);
    }
    
    engine.state.resize(80, 24);
    let row0 = get_row_text(&engine, 0);
    assert!(row0.contains("Consistent Content"));
}

#[test]
fn test_reflow_with_full_scrollback() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    for i in 0..150 {
        engine.process_bytes(format!("Line {:03}\r\n", i).as_bytes());
    }
    
    engine.state.resize(40, 30);
    
    let row_m1 = get_row_text(&engine, -1);
    assert!(!row_m1.trim().is_empty(), "Scrollback row -1 should not be empty");
}

#[test]
fn test_reflow_empty_lines() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    engine.process_bytes(b"Start\r\n\r\n\r\n\r\nEnd");
    
    engine.state.resize(10, 24);
    
    let row0 = get_row_text(&engine, 0);
    assert!(row0.trim() == "Start");
    
    let row4 = get_row_text(&engine, 4);
    assert!(row4.trim() == "End");
}
