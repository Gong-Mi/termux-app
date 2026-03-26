// 测试 get_row 边界检查

use termux_rust::engine::TerminalEngine;

#[test]
fn test_get_row_bounds_checking() {
    println!("=== 测试 get_row 边界检查 ===\n");

    // 1. 创建 80x24 屏幕，写入 50 行内容
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    for i in 1..=50 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    let active = engine.state.main_screen.active_transcript_rows;
    let rows = engine.state.main_screen.rows;
    
    println!("active_transcript_rows = {}", active);
    println!("rows = {}", rows);
    
    // 2. 测试有效范围内的行
    println!("\n1. 测试有效范围内的行:");
    for row in vec![-(active as i32), -1, 0, rows as i32 - 1] {
        let line_text = engine.state.get_current_screen().get_row(row);
        let text: String = line_text.text.iter()
            .take_while(|&&c| c != ' ' && c != '\0')
            .collect();
        println!("   行 {}: {}", row, if text.is_empty() { "(空)" } else { &text });
    }
    
    // 3. 测试超出范围的历史行（应该被钳制到有效范围）
    println!("\n2. 测试超出范围的历史行:");
    let out_of_bounds_rows = vec![-(active as i32) - 1, -(active as i32) - 10, -1000];
    for row in out_of_bounds_rows {
        let line_text = engine.state.get_current_screen().get_row(row);
        let text: String = line_text.text.iter()
            .take_while(|&&c| c != ' ' && c != '\0')
            .collect();
        println!("   行 {} (超出范围): {} -> 应该返回第一行历史", row, if text.is_empty() { "(空)" } else { &text });
    }
    
    // 4. 测试超出范围的屏幕行（应该被钳制到有效范围）
    println!("\n3. 测试超出范围的屏幕行:");
    let out_of_bounds_screen_rows = vec![rows as i32, rows as i32 + 1, 1000];
    for row in out_of_bounds_screen_rows {
        let line_text = engine.state.get_current_screen().get_row(row);
        let text: String = line_text.text.iter()
            .take_while(|&&c| c != ' ' && c != '\0')
            .collect();
        println!("   行 {} (超出范围): {} -> 应该返回最后一行屏幕", row, if text.is_empty() { "(空)" } else { &text });
    }
    
    // 5. 验证 get_transcript_text 不受影响
    println!("\n4. 验证 get_transcript_text:");
    let transcript = engine.state.get_current_screen().get_transcript_text();
    let line_count = transcript.lines().count();
    println!("   transcript 行数：{}", line_count);
    assert!(line_count >= 50, "transcript 应该包含至少 50 行，实际{}行", line_count);
    
    // 6. 验证 get_selected_text 不受影响
    println!("\n5. 验证 get_selected_text:");
    let selected = engine.state.get_current_screen().get_selected_text(0, -(active as i32), 79, rows as i32 - 1);
    let selected_line_count = selected.lines().count();
    println!("   selected 行数：{}", selected_line_count);
    assert!(selected_line_count >= 50, "selected 应该包含至少 50 行，实际{}行", selected_line_count);
    
    println!("\n✓ 边界检查测试通过");
}
