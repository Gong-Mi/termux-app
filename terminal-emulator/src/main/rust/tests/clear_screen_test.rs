// 测试 CSI 3J (清除历史行) 命令
// 运行：cargo test --test clear_screen_test -- --nocapture

use termux_rust::TerminalEngine;

fn get_screen_text(engine: &TerminalEngine) -> String {
    let mut result = String::new();
    for row in 0..engine.state.rows {
        let r = engine.state.main_screen.get_row(row);
        let text: String = r.text.iter().filter(|&&c| c != ' ' && c != '\0').collect();
        if !text.is_empty() {
            result.push_str(&text);
            result.push('\n');
        }
    }
    result
}

fn get_history_text(engine: &TerminalEngine) -> String {
    let mut result = String::new();
    let screen = &engine.state.main_screen;
    for row in -(screen.active_transcript_rows as i32)..0 {
        let r = screen.get_row(row);
        let text: String = r.text.iter().filter(|&&c| c != ' ' && c != '\0').collect();
        if !text.is_empty() {
            result.push_str(&text);
            result.push('\n');
        }
    }
    result
}

#[test]
fn test_ed_mode_2_clears_screen_keeps_history() {
    println!("\n=== 测试 CSI 2J (清屏) ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入 30 行
    for i in 1..=30 {
        engine.process_bytes(format!("Line {:02}\r\n", i).as_bytes());
    }
    
    let screen_before = get_screen_text(&engine);
    let history_before = get_history_text(&engine);
    let history_rows_before = engine.state.main_screen.active_transcript_rows;
    
    println!("清屏前:");
    println!("  屏幕内容行数: {}", screen_before.lines().count());
    println!("  历史行数: {}", history_rows_before);
    
    // CSI 2J - 清除整个屏幕
    engine.process_bytes(b"\x1b[2J");
    
    let screen_after = get_screen_text(&engine);
    let history_rows_after = engine.state.main_screen.active_transcript_rows;
    
    println!("清屏后 (2J):");
    println!("  屏幕内容行数: {}", screen_after.lines().count());
    println!("  历史行数: {}", history_rows_after);
    
    // 2J 应该清除屏幕内容但保留历史
    assert_eq!(screen_after.lines().count(), 0, "屏幕应该被清空");
    assert_eq!(history_rows_after, history_rows_before, "历史行应该保留");
    
    println!("✅ CSI 2J 测试通过");
}

#[test]
fn test_ed_mode_3_clears_history_keeps_screen() {
    println!("\n=== 测试 CSI 3J (清除历史) ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入 30 行
    for i in 1..=30 {
        engine.process_bytes(format!("Line {:02}\r\n", i).as_bytes());
    }
    
    let screen_before = get_screen_text(&engine);
    let history_before = get_history_text(&engine);
    let history_rows_before = engine.state.main_screen.active_transcript_rows;
    
    println!("清除前:");
    println!("  屏幕内容: {} 行", screen_before.lines().count());
    println!("  历史内容: {} 行", history_before.lines().count());
    println!("  历史行数: {}", history_rows_before);
    
    // CSI 3J - 只清除历史行，保留屏幕内容
    engine.process_bytes(b"\x1b[3J");
    
    let screen_after = get_screen_text(&engine);
    let history_after = get_history_text(&engine);
    let history_rows_after = engine.state.main_screen.active_transcript_rows;
    
    println!("清除后 (3J):");
    println!("  屏幕内容: {} 行", screen_after.lines().count());
    println!("  历史内容: {} 行", history_after.lines().count());
    println!("  历史行数: {}", history_rows_after);
    
    // 3J 应该清除历史但保留屏幕内容
    assert_eq!(history_rows_after, 0, "历史行应该被清空");
    assert!(screen_after.lines().count() > 0, "屏幕内容应该保留");
    
    // 验证屏幕内容没有变化
    assert_eq!(screen_before, screen_after, "屏幕内容应该完全相同");
    
    println!("✅ CSI 3J 测试通过");
}

#[test]
fn test_ed_mode_0_from_cursor() {
    println!("\n=== 测试 CSI 0J (从光标到末尾) ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入内容
    for i in 1..=10 {
        engine.process_bytes(format!("Line {:02}\r\n", i).as_bytes());
    }
    
    // 移动光标到第 5 行第 10 列
    engine.process_bytes(b"\x1b[5;10H");
    
    println!("光标位置: ({}, {})", engine.state.cursor.x, engine.state.cursor.y);
    
    // CSI 0J - 从光标到末尾
    engine.process_bytes(b"\x1b[J");
    
    // 验证前 4 行保留，第 5 行前 10 个字符保留
    let screen = get_screen_text(&engine);
    println!("清除后屏幕内容:\n{}", screen);
    
    assert!(screen.contains("Line01"), "Line01 应该保留");
    assert!(screen.contains("Line04"), "Line04 应该保留");
    
    println!("✅ CSI 0J 测试通过");
}

#[test]
fn test_ed_mode_1_from_start() {
    println!("\n=== 测试 CSI 1J (从开头到光标) ===");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入内容
    for i in 1..=10 {
        engine.process_bytes(format!("Line {:02}\r\n", i).as_bytes());
    }
    
    // 移动光标到第 5 行
    engine.process_bytes(b"\x1b[5;10H");
    
    println!("光标位置: ({}, {})", engine.state.cursor.x, engine.state.cursor.y);
    
    // CSI 1J - 从开头到光标
    engine.process_bytes(b"\x1b[1J");
    
    let screen = get_screen_text(&engine);
    println!("清除后屏幕内容:\n{}", screen);
    
    // 验证第 6-10 行保留
    assert!(screen.contains("Line06"), "Line06 应该保留");
    assert!(screen.contains("Line10"), "Line10 应该保留");
    
    println!("✅ CSI 1J 测试通过");
}
