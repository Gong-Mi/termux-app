// 测试列变化时的 resize 问题
// 运行：cargo test --test resize_column_change -- --nocapture

use termux_rust::TerminalEngine;

#[test]
fn test_column_change_history_loss() {
    println!("\n=== 列变化导致历史丢失测试 ===");
    
    // 1. 创建 80x24 引擎，写入 30 行
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    for i in 1..=30 {
        let line = format!("Line {:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    let initial_history = engine.state.main_screen.active_transcript_rows;
    let initial_first_row = engine.state.main_screen.first_row;
    println!("初始状态 (80x24):");
    println!("  first_row: {}", initial_first_row);
    println!("  active_transcript_rows: {}", initial_history);
    
    // 2. 改变列数 + 行数（触发慢路径）
    engine.state.resize(60, 18);
    let zoomed_history = engine.state.main_screen.active_transcript_rows;
    let zoomed_first_row = engine.state.main_screen.first_row;
    println!("\n改变列数到 60x18 (慢路径):");
    println!("  first_row: {}", zoomed_first_row);
    println!("  active_transcript_rows: {}", zoomed_history);
    
    // 3. 恢复原始尺寸
    engine.state.resize(80, 24);
    let final_history = engine.state.main_screen.active_transcript_rows;
    let final_first_row = engine.state.main_screen.first_row;
    println!("\n恢复到 80x24:");
    println!("  first_row: {}", final_first_row);
    println!("  active_transcript_rows: {}", final_history);
    
    // 验证
    println!("\n=== 验证结果 ===");
    if initial_history == final_history {
        println!("✅ 历史行数正确: {} -> {}", initial_history, final_history);
    } else {
        println!("❌ 历史行数错误: {} -> {} (预期 {})", initial_history, final_history, initial_history);
        panic!("列变化时历史丢失！");
    }
}

#[test]
fn test_row_only_vs_column_change() {
    println!("\n=== 仅行变化 vs 列变化对比 ===");
    
    // 测试 1: 仅行变化（快路径）
    let mut engine1 = TerminalEngine::new(80, 24, 100, 10, 20);
    for i in 1..=30 {
        let line = format!("Line {:02}\r\n", i);
        engine1.process_bytes(line.as_bytes());
    }
    let initial1 = engine1.state.main_screen.active_transcript_rows;
    
    engine1.state.resize(80, 18); // 快路径
    engine1.state.resize(80, 24);
    let final1 = engine1.state.main_screen.active_transcript_rows;
    
    println!("仅行变化 (快路径):");
    println!("  初始: {}, 最终: {}", initial1, final1);
    assert_eq!(initial1, final1, "快路径应该保留历史");
    
    // 测试 2: 列变化（慢路径）
    let mut engine2 = TerminalEngine::new(80, 24, 100, 10, 20);
    for i in 1..=30 {
        let line = format!("Line {:02}\r\n", i);
        engine2.process_bytes(line.as_bytes());
    }
    let initial2 = engine2.state.main_screen.active_transcript_rows;
    
    engine2.state.resize(60, 18); // 慢路径
    engine2.state.resize(80, 24);
    let final2 = engine2.state.main_screen.active_transcript_rows;
    
    println!("\n列变化 (慢路径):");
    println!("  初始: {}, 最终: {}", initial2, final2);
    
    // 这个测试会失败，显示慢路径的问题
    if initial2 != final2 {
        println!("  ❌ 慢路径丢失了 {} 行历史", initial2 - final2);
    }
}
