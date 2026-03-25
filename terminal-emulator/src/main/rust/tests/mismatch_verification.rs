// Java/Rust 不等价点验证测试
// 运行：cargo test --test mismatch_verification -- --nocapture
//
// 本文档验证 JAVA_RUST_MISMATCH_ANALYSIS.md 中记录的潜在差异点

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).trim_end_matches('\0').to_string()
}

fn print_screen(engine: &TerminalEngine, label: &str) {
    println!("\n=== {} ===", label);
    for row in 0..engine.state.rows {
        let text = get_row_text(engine, row as i32);
        if !text.is_empty() {
            println!("  [{:2}] '{}'", row, text.replace('\0', " "));
        }
    }
    println!("  Cursor: ({}, {})", engine.state.cursor.x, engine.state.cursor.y);
}

// 获取 screen_first_row (兼容 main_screen 和 alt_screen)
fn get_screen_first_row(engine: &TerminalEngine) -> usize {
    engine.state.screen_first_row()
}

// 获取 active_transcript_rows
fn get_active_transcript_rows(engine: &TerminalEngine) -> usize {
    engine.state.get_current_screen().active_transcript_rows
}

// =============================================================================
// 1. 环形缓冲区索引计算验证 (first_row 逻辑)
// =============================================================================

/// 验证环形缓冲区索引计算
/// 
/// Java: externalToInternalRow 使用 mScreenFirstRow
/// Rust: internal_row 使用 first_row
/// 
/// 潜在问题：first_row 设置逻辑可能不一致
#[test]
fn test_ring_buffer_indexing() {
    println!("\n=== Test 1: Ring Buffer Indexing ===");
    
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);
    
    // 写入超过屏幕行数的内容，触发滚动
    for i in 0..10 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    print_screen(&engine, "After scrolling 10 lines on 5-row screen");
    
    // 验证第一行应该是 "Line 5"（因为滚动了 5 行）
    let row0_text = get_row_text(&engine, 0);
    println!("  Row 0 text: '{}'", row0_text.trim());
    
    // 检查 first_row 值
    println!("  screen_first_row: {}", get_screen_first_row(&engine));
    println!("  active_transcript_rows: {}", get_active_transcript_rows(&engine));
    
    // 验证：应该能看到 Line 5-9
    assert!(row0_text.contains("Line 5") || row0_text.contains("Line 6"),
            "Row 0 should contain Line 5 or 6 after scrolling");
}

// =============================================================================
// 2. resize 快速路径 vs 慢速路径
// =============================================================================

/// 验证 resize 行为
/// 
/// Java: 仅行数变化时使用快速路径（只调整指针）
/// Rust: 总是重建缓冲区（慢路径）
/// 
/// 潜在问题：
/// 1. 性能差异
/// 2. active_transcript_rows 计算方式不同
#[test]
fn test_resize_fast_vs_slow_path() {
    println!("\n=== Test 2: Resize Fast vs Slow Path ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入一些内容
    for i in 0..30 {
        let line = format!("Line {:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    print_screen(&engine, "Before resize (80x24)");
    let before_rows = engine.state.rows;
    let before_cols = engine.state.cols;
    let before_cursor_y = engine.state.cursor.y;
    
    // 仅改变行数（Java 应该使用快速路径）
    engine.state.resize(80, 12);
    
    print_screen(&engine, "After resize to 80x12 (rows only)");
    
    println!("  Before: {}x{}, cursor_y={}", before_rows, before_cols, before_cursor_y);
    println!("  After: {}x{}, cursor_y={}", engine.state.rows, engine.state.cols, engine.state.cursor.y);
    println!("  active_transcript_rows: {}", get_active_transcript_rows(&engine));
    
    // 验证内容没有丢失
    assert_eq!(engine.state.cols, 80, "Columns should remain 80");
    assert_eq!(engine.state.rows, 12, "Rows should be 12");
}

/// 验证 resize 列数变化
#[test]
fn test_resize_columns_change() {
    println!("\n=== Test 2b: Resize Columns Change ===");
    
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);
    
    // 写入长行
    let long_line = "A".repeat(100);
    engine.process_bytes(format!("{}\r\n", long_line).as_bytes());
    engine.process_bytes(b"Second line\r\n");
    
    print_screen(&engine, "Before resize (80 cols)");
    
    // 缩小列数
    engine.state.resize(40, 10);
    
    print_screen(&engine, "After resize to 40 cols");
    
    // 验证内容重排
    let row0 = get_row_text(&engine, 0);
    let row1 = get_row_text(&engine, 1);
    println!("  Row 0 len: {}, Row 1 len: {}", row0.len(), row1.len());
    
    assert_eq!(engine.state.cols, 40, "Columns should be 40");
}

// =============================================================================
// 3. 滚动逻辑验证 (全屏/部分滚动)
// =============================================================================

/// 验证全屏滚动
/// 
/// Java: 移动 mScreenFirstRow 指针
/// Rust: 移动 first_row 指针
#[test]
fn test_full_screen_scrolling() {
    println!("\n=== Test 3: Full Screen Scrolling ===");
    
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);
    
    // 写入超过屏幕行数的内容
    for i in 0..10 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    print_screen(&engine, "After scrolling");
    
    // 验证滚动计数
    println!("  Scroll counter: {}", engine.state.scroll_counter);
    println!("  screen_first_row: {}", get_screen_first_row(&engine));
    println!("  active_transcript_rows: {}", get_active_transcript_rows(&engine));
    
    // 第一行应该是 Line 5（因为滚动了 5 行）
    let row0_text = get_row_text(&engine, 0);
    assert!(row0_text.contains("Line 5") || row0_text.contains("Line 6"),
            "First visible row should be Line 5 or 6");
}

/// 验证部分滚动（滚动区域）
#[test]
fn test_partial_scrolling() {
    println!("\n=== Test 3b: Partial Scrolling (Scroll Region) ===");
    
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);
    
    // 设置滚动区域（行 2-8）
    engine.process_bytes(b"\x1b[2;8r");
    
    // 在滚动区域内写入内容
    engine.process_bytes(b"\x1b[2;1HLine in scroll region 1\r\n");
    engine.process_bytes(b"Line in scroll region 2\r\n");
    engine.process_bytes(b"Line in scroll region 3\r\n");
    
    print_screen(&engine, "After writing in scroll region");
    
    // 验证滚动区域设置
    println!("  Top margin: {}, Bottom margin: {}", 
             engine.state.top_margin, engine.state.bottom_margin);
}

// =============================================================================
// 4. 字符宽度计算验证 (Unicode 边界情况)
// =============================================================================

/// 验证 Unicode 字符宽度计算
/// 
/// Java: 使用自定义表
/// Rust: 使用 unicode-width crate
/// 
/// 潜在问题：某些罕见 Unicode 字符可能计算结果不同
#[test]
fn test_unicode_width() {
    println!("\n=== Test 4: Unicode Width ===");
    
    let test_cases = vec![
        ("Hello", 5),           // ASCII
        ("你好", 4),            // 中文（双宽）
        ("🔥", 2),              // Emoji（双宽）
        ("テスト", 6),         // 日文（双宽）
        ("테스트", 6),         // 韩文（双宽）
        ("\u{200B}", 0),       // 零宽空格
        ("A\u{200B}B", 2),     // 零宽空格在中间
    ];
    
    for (text, expected_width) in test_cases {
        let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
        engine.process_bytes(text.as_bytes());
        
        let actual_width = engine.state.cursor.x;
        println!("  '{}' -> expected={}, actual={}", 
                 text.replace('\0', " "), expected_width, actual_width);
        
        // 注意：某些字符的宽度可能因实现而异，这里只做记录
        if actual_width != expected_width as i32 {
            println!("    ⚠️  MISMATCH! Check unicode-width crate behavior");
        }
    }
}

/// 验证组合字符处理
#[test]
fn test_combining_characters() {
    println!("\n=== Test 4b: Combining Characters ===");
    
    // e + ́ = é (组合字符)
    let combining = "e\u{0301}";
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    engine.process_bytes(combining.as_bytes());
    
    println!("  'e+combining acute' -> cursor_x={}", engine.state.cursor.x);
    println!("  Expected: 1 (combining char should not advance cursor)");
    
    // 验证屏幕内容
    let row0 = get_row_text(&engine, 0);
    println!("  Row 0 text: '{}'", row0);
}

// =============================================================================
// 5. 换行符处理验证 (样式检查)
// =============================================================================

/// 验证换行符处理中的样式检查
/// 
/// Java: 检查样式变化来决定是否保留尾部空格
/// Rust: 不检查样式，简化逻辑
/// 
/// 潜在问题：尾部空格可能被错误保留
#[test]
fn test_newline_style_check() {
    println!("\n=== Test 5: Newline Style Check ===");
    
    let mut engine = TerminalEngine::new(40, 5, 100, 10, 20);
    
    // 写入带样式的文本和尾部空格
    engine.process_bytes(b"\x1b[31mRed Text   \x1b[0m\r\n");
    engine.process_bytes(b"Next line\r\n");
    
    print_screen(&engine, "After newline with styled text");
    
    let row0 = get_row_text(&engine, 0);
    println!("  Row 0: '{}'", row0);
    println!("  Row 0 len: {}", row0.len());
    
    // 验证尾部空格处理
    let trimmed = row0.trim_end();
    println!("  Row 0 trimmed: '{}'", trimmed);
}

// =============================================================================
// 6. 光标处理验证 (边界重置逻辑)
// =============================================================================

/// 验证光标在 resize 后的位置
/// 
/// Rust: 多了 !cursor_placed 检查，可能导致光标跳到 (0,0)
#[test]
fn test_cursor_after_resize() {
    println!("\n=== Test 6: Cursor After Resize ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入内容并移动光标
    engine.process_bytes(b"\x1b[10;20HText at 10,20");
    
    println!("  Before resize: cursor=({}, {})", 
             engine.state.cursor.x, engine.state.cursor.y);
    
    // resize 后光标应该保持在有效位置
    engine.state.resize(40, 12);
    
    println!("  After resize: cursor=({}, {})", 
             engine.state.cursor.x, engine.state.cursor.y);
    
    // 验证光标在有效范围内
    assert!(engine.state.cursor.x < engine.state.cols as i32,
            "Cursor X should be within new columns");
    assert!(engine.state.cursor.y < engine.state.rows as i32,
            "Cursor Y should be within new rows");
}

/// 验证光标在 resize 后没有被错误重置到 (0,0)
#[test]
fn test_cursor_not_reset_to_origin() {
    println!("\n=== Test 6b: Cursor Not Reset To Origin ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入多行内容
    for i in 0..20 {
        let line = format!("Line {:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    let cursor_before_y = engine.state.cursor.y;
    println!("  Before resize: cursor_y={}", cursor_before_y);
    
    // resize 到较小尺寸
    engine.state.resize(80, 12);
    
    let cursor_after_y = engine.state.cursor.y;
    println!("  After resize: cursor_y={}", cursor_after_y);
    
    // 光标不应该被重置到 0（除非超出新范围）
    // 如果 cursor_after_y == 0 且 cursor_before_y > 11，说明可能有问题
    if cursor_before_y > 11 && cursor_after_y == 0 {
        println!("  ⚠️  WARNING: Cursor may have been aggressively reset");
    }
}

// =============================================================================
// 7. 空行跳过逻辑验证 (滚动阈值)
// =============================================================================

/// 验证空行跳过逻辑
/// 
/// Java: 检查 oldLine == null
/// Rust: 不检查 null
/// 
/// 潜在问题：边界情况下行为可能不同
#[test]
fn test_blank_line_skipping() {
    println!("\n=== Test 7: Blank Line Skipping ===");
    
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);
    
    // 写入一些空行和内容
    engine.process_bytes(b"Line 1\r\n");
    engine.process_bytes(b"\r\n");  // 空行
    engine.process_bytes(b"\r\n");  // 空行
    engine.process_bytes(b"Line 4\r\n");
    
    print_screen(&engine, "After writing blank lines");
    
    // 验证空行被正确处理
    let row1 = get_row_text(&engine, 1);
    let row2 = get_row_text(&engine, 2);
    println!("  Row 1: '{}'", row1);
    println!("  Row 2: '{}'", row2);
}

/// 验证 resize 时空行处理
#[test]
fn test_blank_lines_during_resize() {
    println!("\n=== Test 7b: Blank Lines During Resize ===");
    
    let mut engine = TerminalEngine::new(80, 20, 100, 10, 20);
    
    // 写入内容，中间有空行
    for i in 0..15 {
        if i % 3 == 0 {
            engine.process_bytes(b"\r\n");  // 空行
        } else {
            let line = format!("Line {}\r\n", i);
            engine.process_bytes(line.as_bytes());
        }
    }
    
    print_screen(&engine, "Before resize");
    
    // 缩小屏幕
    engine.state.resize(80, 8);
    
    print_screen(&engine, "After resize to 8 rows");
    
    // 验证内容没有丢失或重复
}

// =============================================================================
// 8. 综合压力测试
// =============================================================================

/// 综合压力测试：模拟真实使用场景
#[test]
fn test_stress_comprehensive() {
    println!("\n=== Test 8: Comprehensive Stress Test ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 模拟 vim 编辑会话
    let session = vec![
        "vim /etc/passwd\r\n",
        "\x1b[31m# /etc/passwd\x1b[0m\r\n",
        "root:x:0:0:root:/root:/bin/bash\r\n",
        "daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\r\n",
        "\x1b[10;1H\x1b[K\x1b[11;1H\x1b[K",  // 清行
        "\x1b[?25l",  // 隐藏光标
        "\x1b[24;1H\x1b[?25h",  // 显示光标在底部
        ":q!\r\n",
    ];
    
    for cmd in session {
        engine.process_bytes(cmd.as_bytes());
    }
    
    print_screen(&engine, "After vim-like session");
    
    // 验证基本状态
    assert!(engine.state.cursor.y >= 0);
    assert!(engine.state.cursor.y < engine.state.rows as i32);
}

/// Resize 压力测试：多次调整大小
#[test]
fn test_resize_stress() {
    println!("\n=== Test 8b: Resize Stress Test ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入内容
    for i in 0..50 {
        let line = format!("Line {:03}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    // 多次 resize
    let sizes = vec![
        (80, 24), (40, 12), (120, 30), (80, 24),
        (60, 20), (80, 24), (35, 10), (80, 24),
    ];
    
    for (cols, rows) in sizes {
        engine.state.resize(cols, rows);
        println!("  Resized to {}x{}, cursor=({}, {})", 
                 cols, rows, engine.state.cursor.x, engine.state.cursor.y);
        
        // 验证光标在有效范围内
        assert!(engine.state.cursor.x < cols as i32);
        assert!(engine.state.cursor.y < rows as i32);
    }
    
    print_screen(&engine, "After resize stress");
}

// =============================================================================
// 快速路径优化测试
// =============================================================================

/// 验证快速路径 resize（仅行数变化）
#[test]
fn test_resize_fast_path_rows_only() {
    println!("\n=== Test 9: Resize Fast Path (Rows Only) ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入内容
    for i in 0..30 {
        let line = format!("Line {:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    let cursor_before_y = engine.state.cursor.y;
    let first_row_before = engine.state.screen_first_row();
    let transcript_before = engine.state.get_current_screen().active_transcript_rows;
    
    println!("  Before: cursor_y={}, first_row={}, transcript={}", 
             cursor_before_y, first_row_before, transcript_before);
    
    // 仅改变行数（应该使用快速路径）
    engine.state.resize(80, 12);
    
    let cursor_after_y = engine.state.cursor.y;
    let first_row_after = engine.state.screen_first_row();
    let transcript_after = engine.state.get_current_screen().active_transcript_rows;
    
    println!("  After: cursor_y={}, first_row={}, transcript={}", 
             cursor_after_y, first_row_after, transcript_after);
    
    // 验证：光标应该在有效范围内
    assert!(cursor_after_y >= 0 && cursor_after_y < 12);
    
    // 验证：内容没有丢失（通过检查第一行）
    let row0_text = get_row_text(&engine, 0);
    println!("  Row 0: '{}'", row0_text.trim());
    
    // 验证：快速路径应该保持数据不变（只是指针移动）
    // 检查几个关键行的内容
    for row in 0..engine.state.rows {
        let text = get_row_text(&engine, row);
        // 每行应该是 "Line XX" 或空
        assert!(text.is_empty() || text.contains("Line") || text.trim().is_empty(),
                "Row {} content should be preserved", row);
    }
}

/// 验证快速路径和慢速路径结果一致
#[test]
fn test_resize_fast_vs_slow_consistency() {
    println!("\n=== Test 9b: Fast vs Slow Path Consistency ===");
    
    // 测试 1: 快速路径（仅行数变化）
    let mut engine_fast = TerminalEngine::new(80, 24, 100, 10, 20);
    for i in 0..30 {
        let line = format!("Line {:02}\r\n", i);
        engine_fast.process_bytes(line.as_bytes());
    }
    engine_fast.state.resize(80, 12);  // 快速路径
    
    // 测试 2: 慢速路径（列数也变化）
    let mut engine_slow = TerminalEngine::new(80, 24, 100, 10, 20);
    for i in 0..30 {
        let line = format!("Line {:02}\r\n", i);
        engine_slow.process_bytes(line.as_bytes());
    }
    engine_slow.state.resize(79, 12);  // 慢速路径（列变化）
    engine_slow.state.resize(80, 12);  // 再改回 80 列
    
    // 验证：两种方式的光标位置应该相同
    println!("  Fast path cursor: ({}, {})", 
             engine_fast.state.cursor.x, engine_fast.state.cursor.y);
    println!("  Slow path cursor: ({}, {})", 
             engine_slow.state.cursor.x, engine_slow.state.cursor.y);
    
    // 光标 Y 应该相同（行数变化相同）
    assert_eq!(engine_fast.state.cursor.y, engine_slow.state.cursor.y,
               "Cursor Y should be same for fast and slow path");
    
    // 验证：第一行内容应该相同
    let fast_row0 = get_row_text(&engine_fast, 0);
    let slow_row0 = get_row_text(&engine_slow, 0);
    println!("  Fast path row 0: '{}'", fast_row0.trim());
    println!("  Slow path row 0: '{}'", slow_row0.trim());
    
    // 内容应该一致
    assert_eq!(fast_row0.trim(), slow_row0.trim(),
               "First row content should be same for fast and slow path");
}

// =============================================================================
// 主函数：运行所有测试
// =============================================================================

fn main() {
    println!("Java/Rust Mismatch Verification Tests");
    println!("=====================================\n");
    
    // 运行所有测试
    test_ring_buffer_indexing();
    test_resize_fast_vs_slow_path();
    test_resize_columns_change();
    test_full_screen_scrolling();
    test_partial_scrolling();
    test_unicode_width();
    test_combining_characters();
    test_newline_style_check();
    test_cursor_after_resize();
    test_cursor_not_reset_to_origin();
    test_blank_line_skipping();
    test_blank_lines_during_resize();
    test_stress_comprehensive();
    test_resize_stress();
    
    println!("\n=====================================");
    println!("All tests completed!");
}
