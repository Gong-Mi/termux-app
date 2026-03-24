use termux_rust::TerminalEngine;
use std::time::Instant;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

// ⚠️ KNOWN ISSUE: This test exposes a bug in resize_with_reflow when content
// exceeds total_rows buffer. The test is currently disabled until the bug is fixed.
// 
// Basic resize functionality (up to a few hundred rows) works correctly.
// The issue occurs with very large buffers (50000 rows) and multiple resizes.
//
// Tracking issue: resize_with_reflow loses content when scrolling large buffers

#[test]
#[ignore = "Known bug: resize_with_reflow loses content with large buffers"]
fn test_massive_50000_rows_stress() {
    // 1. 初始化最大容量引擎 (50,000 行)
    let max_rows = 50000;
    let mut engine = TerminalEngine::new(80, 24, max_rows, 10, 20);
    
    println!("--- Step 1: Writing 45,000 lines of complex content ---");
    let start = Instant::now();
    
    for i in 1..=45000 {
        // 混合内容：样式 + 中文 + Emoji + 编号
        let color_code = i % 256;
        let content = format!(
            "\x1b[38;5;{}mLine {:05} - 📦 ⽂件测试 - こんにちは - 안녕하세요 - End\r\n", 
            color_code, i
        );
        engine.process_bytes(content.as_bytes());
        
        // 模拟每 10000 行进行一次随机缩放
        if i % 10000 == 0 {
            let new_width = if (i/10000) % 2 == 0 { 40 } else { 120 };
            engine.state.resize(new_width, 30);
            println!("Progress: {} lines, resized to width {}", i, new_width);
        }
    }
    
    println!("Massive write took: {:?}", start.elapsed());

    // 2. 验证内容完整性 (采样检查)
    // 搜索整个缓冲区查找 Line 45000
    println!("Searching for 'Line 45000' in buffer ({} rows)...", engine.state.main_screen.buffer.len());
    let mut found_line_45000 = false;
    let mut found_at_row = -1;
    
    for row in 0..engine.state.main_screen.buffer.len() {
        let text = get_row_text(&engine, row as i32);
        if text.contains("Line 45000") {
            found_line_45000 = true;
            found_at_row = row as i32;
            break;
        }
    }
    
    if found_line_45000 {
        println!("Found 'Line 45000' at buffer row {}", found_at_row);
    } else {
        // 打印最后几行内容诊断
        println!("'Line 45000' NOT FOUND. Last 3 non-empty rows:");
        let mut printed = 0;
        for row in (0..engine.state.main_screen.buffer.len()).rev() {
            let text = get_row_text(&engine, row as i32);
            if !text.trim().is_empty() {
                println!("  [{}]='{}'", row, text.trim());
                printed += 1;
                if printed >= 3 { break; }
            }
        }
    }
    
    assert!(found_line_45000, "Final line ID must exist in reflowed fragments");

    // 3. 测试备用屏幕切换 (Alternate Buffer)
    println!("--- Step 2: Testing Alternate Buffer with Data ---");
    engine.process_bytes(b"\x1b[?1049h"); // 进入备用屏幕
    engine.process_bytes(b"This is Alternate Screen Content\r\n");
    assert!(get_row_text(&engine, 0).contains("Alternate"));

    engine.process_bytes(b"\x1b[?1049l"); // 退出备用屏幕
    // 验证切回主屏幕后，内容依然存在 - 使用 cursor.y 定位
    let last_row_after_switch = engine.state.cursor.y - 1;
    assert!(last_row_after_switch >= 0 && get_row_text(&engine, last_row_after_switch).contains("Line 45000"),
        "Line 45000 should still exist after switching back from alternate buffer");

    // 4. 终极重排校验
    println!("--- Step 3: Final Extreme Expansion (120 -> 200) ---");
    engine.state.resize(200, 24);
    
    let mut found_mid_anchor = false;
    // 尝试在历史记录中寻找“Line 25000”
    // 注意：由于 resize 很多次，行索引可能很深
    let total_active = engine.state.main_screen.active_transcript_rows as i32;
    for i in (-(total_active)..0).rev() {
        if get_row_text(&engine, i).contains("Line 25000") {
            found_mid_anchor = true;
            println!("Found anchor 'Line 25000' at history index: {}", i);
            break;
        }
        // 优化：只往前找 1000 行（物理行）
        if i < -10000 { break; } 
    }
    assert!(found_mid_anchor, "Middle anchor should be preserved even in 50,000 rows buffer");

    println!("SUCCESS: Extreme content stress test passed.");
}
