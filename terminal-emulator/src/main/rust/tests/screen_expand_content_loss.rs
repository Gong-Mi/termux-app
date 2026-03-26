// 测试屏幕扩大时内容丢失问题

use termux_rust::engine::TerminalEngine;

#[test]
fn test_screen_expand_content_loss() {
    println!("=== 测试屏幕扩大时内容丢失问题 ===\n");

    // 1. 创建 80x24 屏幕
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    println!("1. 创建 80x24 屏幕");

    // 2. 写入 100 行内容
    for i in 1..=100 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    println!("2. 写入 100 行内容");
    
    // 3. 检查 active_transcript_rows 和 first_row
    println!("3. 初始状态:");
    println!("   active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    println!("   first_row = {}", engine.state.main_screen.first_row);
    println!("   rows = {}", engine.state.main_screen.rows);
    
    // 4. 获取 transcript 文本
    let transcript_before = engine.state.get_current_screen().get_transcript_text();
    let line_count_before = transcript_before.lines().count();
    println!("   transcript 行数 = {}", line_count_before);
    
    // 5. 检查能否读取历史行
    println!("\n4. 读取历史行:");
    for row in -76..0 {
        let line_text = engine.state.get_current_screen().get_row(row);
        let text: String = line_text.text.iter()
            .take_while(|&&c| c != ' ' && c != '\0')
            .collect();
        if !text.is_empty() {
            println!("   行 {}: {}", row, text);
        }
    }
    
    // 6. 缩小到 12 行
    println!("\n5. 缩小到 80x12...");
    let (new_cx, new_cy) = engine.state.main_screen.resize_with_reflow(
        80, 12, 
        0, // current_style
        engine.state.cursor.x, 
        engine.state.cursor.y
    );
    engine.state.main_screen.rows = 12;
    engine.state.cursor.x = new_cx;
    engine.state.cursor.y = new_cy;
    
    println!("   active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    println!("   first_row = {}", engine.state.main_screen.first_row);
    
    let transcript_after_shrink = engine.state.get_current_screen().get_transcript_text();
    println!("   transcript 行数 = {}", transcript_after_shrink.lines().count());
    
    // 7. 扩大到 48 行
    println!("\n6. 扩大到 80x48...");
    let (new_cx, new_cy) = engine.state.main_screen.resize_with_reflow(
        80, 48, 
        0, // current_style
        engine.state.cursor.x, 
        engine.state.cursor.y
    );
    engine.state.main_screen.rows = 48;
    engine.state.cursor.x = new_cx;
    engine.state.cursor.y = new_cy;
    
    println!("   active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    println!("   first_row = {}", engine.state.main_screen.first_row);
    
    let transcript_after_expand = engine.state.get_current_screen().get_transcript_text();
    println!("   transcript 行数 = {}", transcript_after_expand.lines().count());
    
    // 8. 检查扩大后的内容
    println!("\n7. 扩大后的 transcript 内容 (前 20 行):");
    for (i, line) in transcript_after_expand.lines().take(20).enumerate() {
        println!("   {}: {}", i, line.chars().take(50).collect::<String>());
    }
    
    // 9. 检查是否有内容丢失
    println!("\n8. 内容完整性检查:");
    let expected_lines: usize = 100;
    let actual_lines = transcript_after_expand.lines().count();
    println!("   预期行数：{}", expected_lines);
    println!("   实际行数：{}", actual_lines);
    println!("   丢失行数：{}", expected_lines.saturating_sub(actual_lines));
    
    // 10. 检查特定行是否存在
    println!("\n9. 检查特定行:");
    let target_lines = vec![1, 10, 25, 50, 75, 100];
    for target in target_lines {
        let target_str = format!("Line {}", target);
        let found = transcript_after_expand.lines().any(|line| line.contains(&target_str));
        println!("   '{}' - {}", target_str, if found { "✓ 存在" } else { "✗ 丢失" });
    }
    
    // 断言：不应该丢失内容
    assert!(actual_lines >= 90, "丢失了太多行：预期至少 90 行，实际{}行", actual_lines);
}
