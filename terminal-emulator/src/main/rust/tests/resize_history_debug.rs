// 精确定位历史行丢失问题
// 运行：cargo test --test resize_history_debug -- --nocapture

use termux_rust::TerminalEngine;

fn dump_screen_state(engine: &TerminalEngine, label: &str) {
    let screen = &engine.state.main_screen;
    println!("\n=== {} ===", label);
    println!("  cols={}, rows={}, buffer.len()={}", screen.cols, screen.rows, screen.buffer.len());
    println!("  first_row={}", screen.first_row);
    println!("  active_transcript_rows={}", screen.active_transcript_rows);
    println!("  cursor=({}, {})", engine.state.cursor.x, engine.state.cursor.y);
    
    // 打印可见内容
    println!("  可见内容:");
    for row in 0..screen.rows {
        let r = screen.get_row(row);
        let text: String = r.text.iter().filter(|&&c| c != ' ' && c != '\0').collect();
        if !text.is_empty() {
            println!("    row {}: '{}'", row, text);
        }
    }
    
    // 打印历史内容
    if screen.active_transcript_rows > 0 {
        println!("  历史内容 ({} 行):", screen.active_transcript_rows);
        for row in -(screen.active_transcript_rows as i32)..0 {
            let r = screen.get_row(row);
            let text: String = r.text.iter().filter(|&&c| c != ' ' && c != '\0').collect();
            if !text.is_empty() {
                println!("    history row {}: '{}'", row, text);
            }
        }
    }
}

#[test]
fn test_fast_path_history() {
    println!("\n========== 快路径测试 ==========");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入 30 行
    for i in 1..=30 {
        let line = format!("Line {:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    dump_screen_state(&engine, "初始 80x24 (写入30行)");
    let initial_history = engine.state.main_screen.active_transcript_rows;
    
    // 快路径：仅改变行数
    engine.state.resize(80, 18);
    dump_screen_state(&engine, "放大到 80x18 (快路径)");
    
    engine.state.resize(80, 24);
    dump_screen_state(&engine, "缩小回 80x24 (快路径)");
    
    let final_history = engine.state.main_screen.active_transcript_rows;
    println!("\n结果: 初始历史={}, 最终历史={}", initial_history, final_history);
    assert_eq!(initial_history, final_history, "快路径应该保留历史");
}

#[test]
fn test_slow_path_history() {
    println!("\n========== 慢路径测试 ==========");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入 30 行
    for i in 1..=30 {
        let line = format!("Line {:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    dump_screen_state(&engine, "初始 80x24 (写入30行)");
    let initial_history = engine.state.main_screen.active_transcript_rows;
    let initial_first_row = engine.state.main_screen.first_row;
    
    // 慢路径：改变列数 + 行数
    engine.state.resize(60, 18);
    dump_screen_state(&engine, "改变到 60x18 (慢路径)");
    
    // 恢复原始尺寸
    engine.state.resize(80, 24);
    dump_screen_state(&engine, "恢复 80x24 (慢路径)");
    
    let final_history = engine.state.main_screen.active_transcript_rows;
    println!("\n结果: 初始历史={}, 最终历史={}", initial_history, final_history);
    println!("  first_row: {} -> {} -> {}", initial_first_row, 
             engine.state.main_screen.first_row, engine.state.main_screen.first_row);
    
    if initial_history != final_history {
        println!("  ❌ 慢路径丢失了 {} 行历史", initial_history - final_history);
    }
}

#[test]
fn test_slow_path_content_check() {
    println!("\n========== 慢路径内容检查 ==========");
    
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入特定内容方便验证
    for i in 1..=30 {
        let line = format!("ROW{:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    // 记录初始 transcript
    let initial_transcript = engine.state.main_screen.get_transcript_text();
    let initial_lines: Vec<&str> = initial_transcript.lines().collect();
    println!("初始 transcript 行数: {}", initial_lines.len());
    for (i, line) in initial_lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            println!("  [{}] '{}'", i, trimmed);
        }
    }
    
    // 慢路径 resize
    engine.state.resize(60, 18);
    let resized_transcript = engine.state.main_screen.get_transcript_text();
    let resized_lines: Vec<&str> = resized_transcript.lines().collect();
    println!("\nresize 后 transcript 行数: {}", resized_lines.len());
    
    // 恢复
    engine.state.resize(80, 24);
    let final_transcript = engine.state.main_screen.get_transcript_text();
    let final_lines: Vec<&str> = final_transcript.lines().collect();
    println!("恢复后 transcript 行数: {}", final_lines.len());
    for (i, line) in final_lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            println!("  [{}] '{}'", i, trimmed);
        }
    }
    
    // 验证关键行是否存在
    for target in &["ROW01", "ROW05", "ROW10", "ROW20", "ROW30"] {
        let found_initial = initial_transcript.contains(target);
        let found_final = final_transcript.contains(target);
        println!("  {}: 初始={}, 最终={}", target, found_initial, found_final);
        assert!(found_final, "{} 应该在最终 transcript 中", target);
    }
}
