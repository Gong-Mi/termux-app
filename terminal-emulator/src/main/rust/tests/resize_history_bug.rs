// 精确验证 resize 历史管理问题
// 运行：cargo test --test resize_history_bug -- --nocapture

use termux_rust::TerminalEngine;

#[test]
fn test_history_bug_explicit() {
    println!("\n=== 历史管理问题详细分析 ===");
    
    // 1. 创建 80x24 引擎，写入 30 行
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    for i in 1..=30 {
        let line = format!("Line {:02}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    let initial_history = engine.state.main_screen.active_transcript_rows;
    let initial_first_row = engine.state.main_screen.first_row;
    println!("初始状态:");
    println!("  屏幕: 80x24");
    println!("  first_row: {}", initial_first_row);
    println!("  active_transcript_rows: {}", initial_history);
    println!("  预期: 30 - 24 = 6 行历史");
    
    // 2. 放大到 18 行（减少 6 行）
    engine.state.resize(80, 18);
    let zoomed_history = engine.state.main_screen.active_transcript_rows;
    let zoomed_first_row = engine.state.main_screen.first_row;
    println!("\n放大到 80x18:");
    println!("  first_row: {}", zoomed_first_row);
    println!("  active_transcript_rows: {}", zoomed_history);
    println!("  预期: 30 - 18 = 12 行历史");
    
    // 3. 缩小回 24 行
    engine.state.resize(80, 24);
    let final_history = engine.state.main_screen.active_transcript_rows;
    let final_first_row = engine.state.main_screen.first_row;
    println!("\n缩小回 80x24:");
    println!("  first_row: {}", final_first_row);
    println!("  active_transcript_rows: {}", final_history);
    println!("  预期: 30 - 24 = 6 行历史");
    
    // 验证
    println!("\n=== 验证结果 ===");
    if initial_history == final_history {
        println!("✅ 历史行数正确: {} -> {}", initial_history, final_history);
    } else {
        println!("❌ 历史行数错误: {} -> {} (预期 {})", initial_history, final_history, initial_history);
    }
    
    // 分析放大时的行为
    println!("\n=== 放大时行为分析 ===");
    println!("放大时 shift = 24 - 18 = 6 (正数 = shrinking)");
    println!("Java: active = max(0, {} + 6) = {}", initial_history, initial_history + 6);
    println!("Rust: active = {} + 6 = {}", initial_history, initial_history + 6);
    
    // 分析缩小时的行为
    println!("\n缩小时行为分析:");
    println!("缩小时 shift = 18 - 24 = -6 (负数 = expanding)");
    println!("Java: active = max(0, {} + (-6)) = max(0, {}) = ?", zoomed_history, zoomed_history as i32 - 6);
    println!("Rust: active = {}.saturating_sub(6) = ?", zoomed_history);
    
    assert!(initial_history > 0, "初始应该有历史行");
}

#[test]
fn test_java_vs_rust_resize_logic() {
    println!("\n=== Java vs Rust resize 逻辑对比 ===");
    
    // 模拟 Java 逻辑
    fn java_resize(active: usize, shift: i32) -> usize {
        if shift > 0 {
            // Shrinking: increase transcript
            active + shift as usize
        } else {
            // Expanding: decrease transcript
            (active as i32 + shift).max(0) as usize
        }
    }
    
    // 模拟 Rust 逻辑
    fn rust_resize(active: usize, shift: i32) -> usize {
        if shift > 0 {
            active + shift as usize
        } else {
            active.saturating_sub((-shift) as usize)
        }
    }
    
    // 测试场景 1: 初始 24 行，放大到 18 行 (shift = 6)
    let active1 = 6;
    let shift1 = 6; // shrinking
    let java1 = java_resize(active1, shift1);
    let rust1 = rust_resize(active1, shift1);
    println!("场景 1: 放大 (shift = {})", shift1);
    println!("  Java: {} -> {}", active1, java1);
    println!("  Rust: {} -> {}", active1, rust1);
    assert_eq!(java1, rust1, "shrinking 时 Java 和 Rust 应该相同");
    
    // 测试场景 2: 从 18 行缩小回 24 行 (shift = -6)
    let active2 = java1; // 使用 Java 的结果
    let shift2 = -6; // expanding
    let java2 = java_resize(active2, shift2);
    let rust2 = rust_resize(active2, shift2);
    println!("\n场景 2: 缩小 (shift = {})", shift2);
    println!("  Java: {} -> {}", active2, java2);
    println!("  Rust: {} -> {}", active2, rust2);
    assert_eq!(java2, rust2, "expanding 时 Java 和 Rust 应该相同");
    
    println!("\n✅ Java vs Rust 逻辑对比通过");
}
