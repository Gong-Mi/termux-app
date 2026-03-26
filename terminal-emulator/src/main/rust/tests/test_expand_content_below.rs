// 测试屏幕扩大时下方内容丢失问题

use termux_rust::engine::TerminalEngine;
use md5::compute;

#[test]
fn test_expand_shows_content_below_viewport() {
    println!("=== 测试屏幕扩大时下方内容显示 ===\n");

    // 1. 创建 80x10 小屏幕
    let mut engine = TerminalEngine::new(80, 10, 1000, 10, 20);
    println!("1. 创建 80x10 屏幕");

    // 2. 写入 20 行内容（超过屏幕高度）
    // 使用 MD5 哈希作为填充内容，每行唯一标识
    for i in 1..=20 {
        let content = format!("{:x}", md5::compute(format!("Line_{}", i).as_bytes()));
        let line = format!("Line {:02}: {}\r\n", i, content);  // 8 + 32 = 40 字符
        engine.process_bytes(line.as_bytes());
    }
    println!("2. 写入 20 行内容 (每行含 MD5 哈希)");
    
    // 3. 检查状态
    println!("3. 初始状态:");
    println!("   active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    println!("   first_row = {}", engine.state.main_screen.first_row);
    println!("   rows = {}", engine.state.main_screen.rows);
    println!("   cursor_y = {}", engine.state.cursor.y);
    
    // 4. 检查当前可见内容（10 行）
    println!("\n4. 当前可见内容 (10 行):");
    for row in 0..engine.state.main_screen.rows {
        let line_text = engine.state.get_current_screen().get_row(row);
        // 获取整行内容
        let text: String = line_text.text.iter()
            .take(80)  // 取 80 个字符
            .filter(|&&c| c != '\0')
            .collect();
        println!("   行 {}: '{}'", row, text.trim_end());
    }
    
    // 5. 检查 transcript 全部内容
    println!("\n5. Transcript 全部内容:");
    let transcript = engine.state.get_current_screen().get_transcript_text();
    for (i, line) in transcript.lines().enumerate() {
        println!("   {}: {}", i, line);
    }
    
    // 6. 扩大到 80x18
    println!("\n6. 扩大到 80x18...");
    let old_active = engine.state.main_screen.active_transcript_rows;
    let old_first = engine.state.main_screen.first_row;
    let old_rows = engine.state.main_screen.rows;
    
    let (new_cx, new_cy) = engine.state.main_screen.resize_with_reflow(
        80, 18, 
        0, // current_style
        engine.state.cursor.x, 
        engine.state.cursor.y
    );
    engine.state.main_screen.rows = 18;
    engine.state.cursor.x = new_cx;
    engine.state.cursor.y = new_cy;
    
    println!("   扩大后状态:");
    println!("   active_transcript_rows = {} (之前：{})", engine.state.main_screen.active_transcript_rows, old_active);
    println!("   first_row = {} (之前：{})", engine.state.main_screen.first_row, old_first);
    println!("   rows = {} (之前：{})", engine.state.main_screen.rows, old_rows);
    println!("   cursor_y = {}", engine.state.cursor.y);
    
    // 7. 检查扩大后可见内容（18 行）
    println!("\n7. 扩大后可见内容 (18 行):");
    for row in 0..engine.state.main_screen.rows {
        let line_text = engine.state.get_current_screen().get_row(row);
        let text: String = line_text.text.iter()
            .take(80)
            .filter(|&&c| c != '\0')
            .collect();
        println!("   行 {}: '{}'", row, text.trim_end());
    }
    
    // 8. 验证扩大后应该能看到 Line 04-Line 20 (因为 cursor 在底部，扩大后向上显示历史)
    println!("\n8. 验证内容完整性 (使用 MD5 哈希):");
    // 扩大后，active_transcript_rows = 3，所以历史有 3 行 (Line 01-Line 03)
    // 可见区域从 Line 04 开始，到 Line 20，共 17 行有内容 + 1 行空
    let mut all_present = true;
    for i in 1..=17 {  // 只验证 17 行有内容的
        let expected_num = i + 3;  // Line 04 to Line 20
        let expected_prefix = format!("Line {:02}:", expected_num);
        let expected_md5 = format!("{:x}", compute(format!("Line_{}", expected_num).as_bytes()));
        
        let row = (i - 1) as i32;
        let row_text = engine.state.get_current_screen().get_row(row);
        let text: String = row_text.text.iter()
            .take(80)
            .filter(|&&c| c != '\0')
            .collect();
        let trimmed = text.trim_end();
        
        let expected_full = format!("{} {}", expected_prefix, expected_md5);
        if trimmed != expected_full {
            println!("   ✗ 行 {} 应该是 '{}'", row, expected_full.chars().take(50).collect::<String>());
            println!("      但实际是 '{}'", trimmed.chars().take(50).collect::<String>());
            all_present = false;
        }
    }
    
    // 验证第 18 行是空的
    let row_17_text = engine.state.get_current_screen().get_row(17);
    let row_17: String = row_17_text.text.iter().take(80).filter(|&&c| c != '\0').collect();
    if !row_17.trim().is_empty() {
        println!("   ✗ 行 17 应该是空的，但实际是 '{}'", row_17.trim());
        all_present = false;
    }
    
    if all_present {
        println!("   ✓ 所有 17 行内容 + 1 行空都正确 (MD5 验证通过)");
    }
    
    // 验证历史行仍然存在
    println!("\n9. 验证历史行:");
    for row in -3..0 {
        let line_text = engine.state.get_current_screen().get_row(row);
        let text: String = line_text.text.iter()
            .take(30)
            .filter(|&&c| c != '\0')
            .collect();
        let trimmed = text.trim_end();
        println!("   历史行 {}: '{}'", row, trimmed);
    }
    
    // 断言：扩大后应该能看到 Line 01-Line 18
    assert!(all_present, "扩大后应该能看到 Line 01-Line 18");
}
