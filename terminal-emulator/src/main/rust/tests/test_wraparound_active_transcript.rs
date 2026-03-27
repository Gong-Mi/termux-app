// 测试 active_transcript_rows 在环形缓冲区绕回时的计算

use termux_rust::engine::TerminalEngine;

#[test]
fn test_active_transcript_rows_wraparound() {
    println!("=== 测试环形缓冲区绕回时的 active_transcript_rows ===\n");

    // 1. 创建小缓冲区测试绕回
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);  // 只有 100 行缓冲区
    
    // 2. 写入超过缓冲区大小的内容，强制绕回
    let mut scroll_count = 0;
    for i in 1..=150 {
        let content = format!("{:x}", md5::compute(format!("Line_{}", i).as_bytes()));
        let line = format!("L{:03}:{}\r\n", i, content);
        engine.process_bytes(line.as_bytes());
        
        // 当写入超过 10 行后，每次写入都会触发 scroll_up
        if i > 10 {
            scroll_count += 1;
        }
    }
    println!("1. 写入 150 行内容 (缓冲区只有 100 行)");
    println!("   预期 scroll_up 次数：{}", scroll_count);
    
    // 3. 检查状态
    let active = engine.state.main_screen.active_transcript_rows;
    let first = engine.state.main_screen.first_row;
    let rows = engine.state.main_screen.rows;
    let buffer_len = engine.state.main_screen.buffer.len();
    
    println!("2. 状态检查:");
    println!("   active_transcript_rows = {}", active);
    println!("   first_row = {}", first);
    println!("   rows = {}", rows);
    println!("   buffer.len() = {}", buffer_len);
    
    // 4. 验证 active_transcript_rows 应该 = 150 - 10 = 140，但受限于缓冲区大小
    let expected_active = (150 - 10).min(buffer_len - 10);
    println!("   预期 active_transcript_rows ≈ {}", expected_active);
    
    // 5. 检查能否正确读取第一行历史
    let min_row = -(active as i32);
    println!("\n3. 读取历史行 {}: ", min_row);
    let first_history = engine.state.get_current_screen().get_row(min_row);
    let text: String = first_history.text.iter()
        .take(50)
        .filter(|&&c| c != '\0')
        .collect();
    println!("   内容：'{}'", text.trim());
    
    // 6. 检查 transcript 文本
    let transcript = engine.state.get_current_screen().get_transcript_text();
    let line_count = transcript.lines().count();
    println!("\n4. get_transcript_text 返回 {} 行", line_count);
    
    // 7. 验证第一行和最后一行
    println!("\n5. Transcript 首尾内容:");
    if let Some(first_line) = transcript.lines().next() {
        println!("   第一行：'{}'", first_line.chars().take(50).collect::<String>());
    }
    if let Some(last_line) = transcript.lines().last() {
        println!("   最后一行：'{}'", last_line.chars().take(50).collect::<String>());
    }
    
    // 8. 检查是否有内容丢失
    let expected_line_100 = format!("L100: {:x}", md5::compute("Line_100".as_bytes()));
    let has_line_100 = transcript.contains(&expected_line_100);
    println!("\n6. 验证 Line 100: {}", if has_line_100 { "✓ 存在" } else { "✗ 丢失" });
    
    let expected_line_150 = format!("L150: {:x}", md5::compute("Line_150".as_bytes()));
    let has_line_150 = transcript.contains(&expected_line_150);
    println!("   验证 Line 150: {}", if has_line_150 { "✓ 存在" } else { "✗ 丢失" });
}
