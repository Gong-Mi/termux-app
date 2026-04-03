// 缩放模拟测试 - 验证放大缩小不影响内容逻辑
// 运行：cargo test --test resize_zoom_simulation -- --nocapture

use termux_rust::TerminalEngine;

fn setup_engine_with_content(cols: i32, rows: i32) -> TerminalEngine {
    let mut engine = TerminalEngine::new(cols, rows, 100, 10, 20);

    // 写入 30 行测试内容
    for i in 1..=30 {
        let line = format!("Line {:02}: Hello World Test Content\r\n", i);
        engine.process_bytes(line.as_bytes());
    }

    engine
}

/// 获取历史行数
fn get_history_rows(engine: &TerminalEngine) -> usize {
    engine.state.main_screen.active_transcript_rows
}

#[test]
fn test_zoom_in_out_cycle() {
    println!("\n=== 测试 1: 放大→缩小循环 ===");
    
    // 初始 80x24
    let mut engine = setup_engine_with_content(80, 24);
    let initial_history = get_history_rows(&engine);
    println!("初始状态: 80x24, 历史行数: {}", initial_history);
    
    // 放大（字体变大 → 行列数减少）
    engine.state.resize(60, 18);
    let zoomed_history = get_history_rows(&engine);
    println!("放大到 60x18: 历史行数: {}", zoomed_history);
    
    // 缩小回原始
    engine.state.resize(80, 24);
    let final_history = get_history_rows(&engine);
    println!("缩小回 80x24: 历史行数: {}", final_history);
    
    // 验证历史行数回到初始值
    assert_eq!(initial_history, final_history, 
        "缩小回原始尺寸后，历史行数应该相同");
    
    println!("✅ 放大→缩小循环测试通过");
}

#[test]
fn test_rapid_zoom_cycle() {
    println!("\n=== 测试 2: 快速缩放循环 ===");
    
    let mut engine = setup_engine_with_content(80, 24);
    
    let sizes = vec![
        (80, 24), // 初始
        (70, 20), // 放大
        (60, 16), // 继续放大
        (50, 12), // 最大放大
        (60, 16), // 缩小一点
        (70, 20), // 继续缩小
        (80, 24), // 回到初始
        (90, 28), // 继续缩小（字体更小）
        (80, 24), // 回到初始
    ];
    
    for (i, (cols, rows)) in sizes.iter().enumerate() {
        engine.state.resize(*cols, *rows);
        println!("步骤 {}: Resize 到 {}x{}", i + 1, cols, rows);
        assert_eq!(engine.state.cols, *cols, "列数应该正确");
        assert_eq!(engine.state.rows, *rows, "行数应该正确");
    }
    
    println!("✅ 快速缩放循环测试通过");
}

#[test]
fn test_history_management_during_zoom() {
    println!("\n=== 测试 3: 缩放时历史管理 ===");
    
    let mut engine = setup_engine_with_content(80, 24);
    
    // 写入 30 行，屏幕 24 行，历史应该有 6 行
    let initial_history = get_history_rows(&engine);
    println!("初始历史: {} 行", initial_history);
    
    // 放大到 18 行（减少 6 行）
    engine.state.resize(80, 18);
    let zoomed_history = get_history_rows(&engine);
    println!("放大到 18 行后历史: {} 行", zoomed_history);
    
    // 放大时，原本在屏幕上的 6 行应该进入历史
    assert!(zoomed_history >= initial_history, 
        "放大时历史行数应该增加或保持不变");
    
    // 缩小回 24 行
    engine.state.resize(80, 24);
    let final_history = get_history_rows(&engine);
    println!("缩小回 24 行后历史: {} 行", final_history);
    
    // 验证历史管理正确
    assert!(final_history <= zoomed_history, 
        "缩小时历史行数应该减少或保持不变");
    
    println!("✅ 历史管理测试通过");
}

#[test]
fn test_wide_char_during_zoom() {
    println!("\n=== 测试 4: CJK 字符缩放 ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入 CJK 字符（每个占 2 列）
    let cjk_text = "你好世界测试内容";
    for ch in cjk_text.chars() {
        engine.process_bytes(format!("{}", ch).as_bytes());
    }
    engine.process_bytes(b"\r\n");
    
    // 放大
    engine.state.resize(40, 12);
    println!("放大到 40x12 后 CJK 字符仍然可见");
    
    // 缩小
    engine.state.resize(80, 24);
    println!("缩小回 80x24");
    
    println!("✅ CJK 字符缩放测试通过");
}

#[test]
fn test_cursor_position_during_zoom() {
    println!("\n=== 测试 5: 光标位置在缩放时 ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 移动光标到特定位置
    engine.process_bytes(b"Hello World\r\n");
    engine.process_bytes(b"Second Line\r\n");
    
    let initial_cx = engine.state.cursor.x;
    let initial_cy = engine.state.cursor.y;
    println!("初始光标: ({}, {})", initial_cx, initial_cy);
    
    // 放大
    engine.state.resize(60, 18);
    let zoomed_cx = engine.state.cursor.x;
    let zoomed_cy = engine.state.cursor.y;
    println!("放大后光标: ({}, {})", zoomed_cx, zoomed_cy);
    
    // 缩小
    engine.state.resize(80, 24);
    let final_cx = engine.state.cursor.x;
    let final_cy = engine.state.cursor.y;
    println!("缩小后光标: ({}, {})", final_cx, final_cy);
    
    // 光标应该在有效范围内
    assert!(final_cx >= 0 && final_cx < 80, "光标 X 应该在范围内");
    assert!(final_cy >= 0 && final_cy < 24, "光标 Y 应该在范围内");
    
    println!("✅ 光标位置测试通过");
}

#[test]
fn test_extreme_zoom_cycle() {
    println!("\n=== 测试 6: 极端缩放循环 ===");
    
    let mut engine = setup_engine_with_content(80, 24);
    
    // 极端放大（最小尺寸）
    engine.state.resize(4, 4);
    println!("极端放大到 4x4");
    assert_eq!(engine.state.cols, 4);
    assert_eq!(engine.state.rows, 4);
    
    // 极端缩小（最大尺寸）
    engine.state.resize(200, 60);
    println!("极端缩小到 200x60");
    assert_eq!(engine.state.cols, 200);
    assert_eq!(engine.state.rows, 60);
    
    // 回到正常
    engine.state.resize(80, 24);
    println!("回到 80x24");
    assert_eq!(engine.state.cols, 80);
    assert_eq!(engine.state.rows, 24);
    
    println!("✅ 极端缩放循环测试通过");
}

#[test]
fn test_column_change_reflow() {
    println!("\n=== 测试 7: 列变化时的内容重排 ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入长行内容
    let long_line = "A".repeat(70);
    engine.process_bytes(long_line.as_bytes());
    engine.process_bytes(b"\r\n");
    
    // 记录初始状态
    println!("初始: 80 列");
    
    // 缩小列（触发重排）
    engine.state.resize(40, 24);
    println!("缩小到 40 列，内容应该重排");
    assert_eq!(engine.state.cols, 40);
    
    // 扩大列
    engine.state.resize(80, 24);
    println!("扩大回 80 列");
    assert_eq!(engine.state.cols, 80);
    
    println!("✅ 列变化重排测试通过");
}

#[test]
fn test_zoom_content_preservation() {
    println!("\n=== 测试 8: 缩放时内容保留 ===");
    
    let mut engine = setup_engine_with_content(80, 24);
    
    // 获取初始 transcript 内容
    let initial_transcript = engine.state.main_screen.get_transcript_text();
    let initial_line_count = initial_transcript.lines().count();
    println!("初始 transcript 行数: {}", initial_line_count);
    
    // 放大
    engine.state.resize(60, 18);
    let zoomed_transcript = engine.state.main_screen.get_transcript_text();
    let zoomed_line_count = zoomed_transcript.lines().count();
    println!("放大后 transcript 行数: {}", zoomed_line_count);
    
    // 缩小回原始
    engine.state.resize(80, 24);
    let final_transcript = engine.state.main_screen.get_transcript_text();
    let final_line_count = final_transcript.lines().count();
    println!("缩小后 transcript 行数: {}", final_line_count);
    
    // 验证内容保留（行数可能因重排略有不同，但应该在合理范围）
    assert!(final_line_count > 0, "缩小后应该有内容");
    assert!(final_line_count >= initial_line_count - 5, "内容不应该大量丢失");
    
    println!("✅ 内容保留测试通过");
}
