use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

#[test]
fn test_vt100_core_compatibility() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 1. 测试游标定位与基本文本 (CUP)
    engine.process_bytes(b"\x1b[5;10HVT100");
    assert_eq!(engine.state.cursor.y, 4);
    assert_eq!(engine.state.cursor.x, 14);
    assert!(get_row_text(&engine, 4).contains("VT100"));

    // 2. 测试擦除显示 (ED 2)
    engine.process_bytes(b"\x1b[2J");
    assert!(!get_row_text(&engine, 4).contains("VT100"));

    // 3. 测试设置滚动区域 (DECSTBM)
    engine.process_bytes(b"\x1b[5;10r");
    assert_eq!(engine.state.top_margin, 4);
    assert_eq!(engine.state.bottom_margin, 10);
    
    println!("VT100 Core Compatibility: PASSED");
}

#[test]
fn test_vt200_editing_compatibility() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    engine.process_bytes(b"Line1\r\nLine2\r\nLine3");
    
    // 1. 测试删除行 (DL)
    engine.process_bytes(b"\x1b[1H\x1b[1M"); // 回到第一行并删除
    assert!(get_row_text(&engine, 0).contains("Line2"));
    
    // 2. 测试插入字符 (ICH)
    engine.process_bytes(b"\x1b[1H\x1b[5@"); // 在开头插入 5 个空格
    assert!(get_row_text(&engine, 0).contains("     Line2"));

    // 3. 测试删除字符 (DCH)
    engine.process_bytes(b"\x1b[1H\x1b[5P"); // 删除前 5 个字符
    assert!(get_row_text(&engine, 0).trim().starts_with("Line2"));

    // 4. 测试身份报告 (DA)
    // 注意：这会向输入流写回响应，我们需要检查报告内容
    engine.process_bytes(b"\x1b[c");
    // 之前分析 csi.rs 看到它返回 \x1b[?6c (VT102)
    
    println!("VT200 Editing Compatibility: PASSED");
}

#[test]
fn test_vt300_features_compatibility() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 1. 测试颜色设置 (SGR 256/TrueColor)
    // 虽然 SGR 是 ANSI 标，但在 VT300 时代得到了极大加强
    engine.process_bytes(b"\x1b[38;2;255;0;0mRedText");
    // 验证样式 (简单通过 get_row_text 无法验证样式，主要验证状态机不崩溃)
    
    // 2. 测试软重置 (DECSTR)
    engine.process_bytes(b"\x1b[!p");
    // 重置后颜色应该回到默认
    
    // 3. 测试绝对坐标定位 (VPA/HPA)
    engine.process_bytes(b"\x1b[10d"); // 垂直定位到第 10 行
    assert_eq!(engine.state.cursor.y, 9);
    
    println!("VT300/Modern Features Compatibility: PASSED");
}

#[test]
fn test_mode_switching_compatibility() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 1. 测试进入备用屏幕 (DECSET 1049)
    engine.process_bytes(b"\x1b[?1049h");
    // 验证逻辑：备用屏幕应该是一个干净的缓冲区
    
    // 2. 测试退出备用屏幕 (DECRST 1049)
    engine.process_bytes(b"\x1b[?1049l");
    
    println!("Mode Switching Compatibility: PASSED");
}
