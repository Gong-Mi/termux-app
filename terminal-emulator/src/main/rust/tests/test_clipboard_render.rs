// 测试剪贴板和渲染问题

use termux_rust::engine::TerminalEngine;

#[test]
fn test_clipboard_and_rendering() {
    println!("=== 测试剪贴板和渲染问题 ===\n");

    // 1. 创建 80x24 屏幕
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    // 2. 写入带 MD5 哈希的内容
    for i in 1..=50 {
        let content = format!("{:x}", md5::compute(format!("Line_{}", i).as_bytes()));
        let line = format!("Line {:02}: {}\r\n", i, content);
        engine.process_bytes(line.as_bytes());
    }
    println!("1. 写入 50 行内容");
    
    // 3. 检查 active_transcript_rows
    let active = engine.state.main_screen.active_transcript_rows;
    let rows = engine.state.main_screen.rows;
    println!("2. active_transcript_rows = {}, rows = {}", active, rows);
    
    // 4. 测试 get_transcript_text
    let transcript = engine.state.get_current_screen().get_transcript_text();
    let line_count = transcript.lines().count();
    println!("3. get_transcript_text 返回 {} 行", line_count);
    
    // 5. 检查前几行内容
    println!("\n4. Transcript 前 5 行:");
    for (i, line) in transcript.lines().take(5).enumerate() {
        println!("   {}: '{}'", i, line.chars().take(50).collect::<String>());
    }
    
    // 6. 测试 get_selected_text 选择历史行
    println!("\n5. 测试选择历史行:");
    let selected = engine.state.get_current_screen().get_selected_text(0, -26, 79, -1);
    let selected_lines = selected.lines().count();
    println!("   选择行 -26 到 -1, 返回 {} 行", selected_lines);
    
    // 7. 检查选择的内容
    println!("   选择内容前 3 行:");
    for (i, line) in selected.lines().take(3).enumerate() {
        println!("   {}: '{}'", i, line.chars().take(50).collect::<String>());
    }
    
    // 8. 验证内容是否正确
    println!("\n6. 验证内容:");
    let expected_line_1 = format!("Line 01: {:x}", md5::compute("Line_1".as_bytes()));
    let has_line_1 = transcript.contains(&expected_line_1);
    println!("   Transcript 包含 Line 01: {}", if has_line_1 { "✓" } else { "✗" });
    
    // 9. 测试可见区域
    println!("\n7. 可见区域内容 (行 0-23):");
    for row in 0..rows {
        let row_data = engine.state.get_current_screen().get_row(row);
        let text: String = row_data.text.iter()
            .take(50)
            .filter(|&&c| c != '\0')
            .collect();
        if !text.trim().is_empty() {
            println!("   行 {}: '{}'", row, text.trim());
        }
    }
}
