// active_transcript_rows 增量维护测试
// 验证 Rust 实现与 Java TerminalBuffer 的行为一致性
//
// 运行：cargo test --test test_active_transcript_rows --release -- --nocapture

use termux_rust::engine::TerminalEngine;

// =============================================================================
// Test 1: 滚动时 active_transcript_rows 增量增加
// =============================================================================

/// 测试全屏滚动时 active_transcript_rows 的增量维护
#[test]
fn test_active_transcript_rows_increment_on_scroll() {
    println!("=== Test 1: active_transcript_rows 增量维护 ===\n");
    
    // 创建 80x10 的终端，缓冲区大小为 20 行（10 行屏幕 + 10 行历史）
    let mut engine = TerminalEngine::new(80, 10, 20, 10, 20);
    
    // 初始状态：没有历史
    assert_eq!(engine.state.main_screen.active_transcript_rows, 0, "初始 active_transcript_rows 应为 0");
    println!("初始状态：active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    
    // 写入 9 行内容（不填满屏幕，确保不触发滚动）
    for i in 0..9 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    
    // 仍然没有历史（内容未填满屏幕）
    assert_eq!(engine.state.main_screen.active_transcript_rows, 0, "未填满屏幕时 active_transcript_rows 应为 0");
    println!("写入 9 行后：active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    
    // 再写入 1 行（第 10 行），填满屏幕但不换行
    engine.process_bytes(b"Line 9");
    assert_eq!(engine.state.main_screen.active_transcript_rows, 0, "填满屏幕但未换行时 active_transcript_rows 应为 0");
    println!("写入第 10 行（无换行）：active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    
    // 现在换行，这应该触发滚动
    engine.process_bytes(b"\r\n");
    assert_eq!(engine.state.main_screen.active_transcript_rows, 1, "换行后 active_transcript_rows 应为 1");
    println!("换行后：active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    
    // 继续写入 5 行，总共应该有 6 行历史
    for i in 10..15 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    assert_eq!(engine.state.main_screen.active_transcript_rows, 6, "继续写入 5 行后 active_transcript_rows 应为 6");
    println!("写入 15 行后：active_transcript_rows = {}", engine.state.main_screen.active_transcript_rows);
    
    println!("✅ Test 1 通过：active_transcript_rows 增量维护正确\n");
}

// =============================================================================
// Test 2: active_transcript_rows 不超过最大值
// =============================================================================

/// 测试 active_transcript_rows 不会超过缓冲区限制
#[test]
fn test_active_transcript_rows_max_limit() {
    println!("=== Test 2: active_transcript_rows 最大限制 ===\n");
    
    // 创建 80x5 的终端，缓冲区大小为 10 行（5 行屏幕 + 5 行历史最大）
    let mut engine = TerminalEngine::new(80, 5, 10, 10, 20);
    
    // 写入 20 行内容，触发 15 次滚动
    for i in 0..20 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    
    // 最大历史行数 = 缓冲区大小 - 屏幕行数 = 10 - 5 = 5
    let max_transcript = 10 - 5;
    assert_eq!(engine.state.main_screen.active_transcript_rows, max_transcript, 
               "active_transcript_rows 不应超过最大值 {}", max_transcript);
    println!("写入 20 行后：active_transcript_rows = {} (最大值：{})", 
             engine.state.main_screen.active_transcript_rows, max_transcript);
    
    // 继续写入，确认不会继续增加
    for i in 20..30 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    assert_eq!(engine.state.main_screen.active_transcript_rows, max_transcript, 
               "继续写入后 active_transcript_rows 仍应为最大值");
    println!("写入 30 行后：active_transcript_rows = {} (保持不变)", 
             engine.state.main_screen.active_transcript_rows);
    
    println!("✅ Test 2 通过：active_transcript_rows 正确限制在最大值\n");
}

// =============================================================================
// Test 3: Resize 时 active_transcript_rows 的正确更新
// =============================================================================

/// 测试 resize 时 active_transcript_rows 的更新
#[test]
fn test_active_transcript_rows_on_resize() {
    println!("=== Test 3: Resize 时 active_transcript_rows 更新 ===\n");
    
    // 创建 80x10 的终端，缓冲区大小为 20 行
    let mut engine = TerminalEngine::new(80, 10, 20, 10, 20);
    
    // 写入 14 行内容（9 行 + 1 行换行触发滚动 + 4 行 = 滚动 5 次）
    for i in 0..9 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    engine.process_bytes(b"Line 9\r\n"); // 这行触发第 1 次滚动
    for i in 10..14 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    
    let transcript_before = engine.state.main_screen.active_transcript_rows;
    println!("Resize 前：active_transcript_rows = {}", transcript_before);
    assert_eq!(transcript_before, 5, "resize 前应有 5 行历史");
    
    // 缩小屏幕到 8 行（触发快速路径）
    engine.state.main_screen.resize_with_reflow(80, 8, 0, 0, 7);
    
    // 缩小后，历史行数应该增加
    let transcript_after_shrink = engine.state.main_screen.active_transcript_rows;
    println!("缩小到 8 行后：active_transcript_rows = {}", transcript_after_shrink);
    assert!(transcript_after_shrink >= transcript_before, "缩小屏幕后历史行数应增加或保持不变");
    
    // 扩大屏幕到 12 行（触发快速路径）
    engine.state.main_screen.resize_with_reflow(80, 12, 0, 0, 7);
    
    // 扩大后，历史行数应该减少
    let transcript_after_expand = engine.state.main_screen.active_transcript_rows;
    println!("扩大到 12 行后：active_transcript_rows = {}", transcript_after_expand);
    assert!(transcript_after_expand <= transcript_after_shrink, "扩大屏幕后历史行数应减少或保持不变");
    
    println!("✅ Test 3 通过：Resize 时 active_transcript_rows 更新正确\n");
}

// =============================================================================
// Test 4: 备用屏幕时 active_transcript_rows 应为 0
// =============================================================================

/// 测试切换到备用屏幕时 active_transcript_rows 重置
#[test]
fn test_active_transcript_rows_alt_screen() {
    println!("=== Test 4: 备用屏幕 active_transcript_rows 重置 ===\n");
    
    let mut engine = TerminalEngine::new(80, 10, 20, 10, 20);
    
    // 写入一些内容到主屏幕（9 行 + 1 行换行触发滚动 + 4 行 = 滚动 5 次）
    for i in 0..9 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    engine.process_bytes(b"Line 9\r\n");
    for i in 10..14 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    
    let main_transcript = engine.state.main_screen.active_transcript_rows;
    println!("主屏幕：active_transcript_rows = {}", main_transcript);
    assert_eq!(main_transcript, 5, "主屏幕应有 5 行历史");
    
    // 切换到备用屏幕 (DECSET 1049)
    engine.process_bytes(b"\x1b[?1049h");
    
    // 备用屏幕的历史行数应为 0
    let alt_screen_transcript = engine.state.alt_screen.active_transcript_rows;
    println!("备用屏幕：active_transcript_rows = {}", alt_screen_transcript);
    assert_eq!(alt_screen_transcript, 0, "备用屏幕的 active_transcript_rows 应为 0");
    
    // 切回主屏幕
    engine.process_bytes(b"\x1b[?1049l");
    
    // 主屏幕的历史行数应保持不变
    let main_transcript_after = engine.state.main_screen.active_transcript_rows;
    println!("切回主屏幕：active_transcript_rows = {}", main_transcript_after);
    assert_eq!(main_transcript_after, 5, "切回主屏幕后历史行数应保持不变");
    
    println!("✅ Test 4 通过：备用屏幕 active_transcript_rows 正确重置\n");
}

// =============================================================================
// Test 5: 与 Java 行为对比
// =============================================================================

/// 对比 Rust 和 Java 的 active_transcript_rows 行为
#[test]
fn test_active_transcript_rows_java_comparison() {
    println!("=== Test 5: Rust vs Java active_transcript_rows 对比 ===\n");
    
    // Java 逻辑：
    // 1. 初始 mActiveTranscriptRows = 0
    // 2. 每次 scrollUp: if (mActiveTranscriptRows < mTotalRows - mScreenRows) mActiveTranscriptRows++
    // 3. Resize 快路径：mActiveTranscriptRows = max(0, mActiveTranscriptRows + shiftDownOfTopRow)
    // 4. Resize 慢路径：mActiveTranscriptRows = 0 (然后重新累积)
    
    let mut engine = TerminalEngine::new(80, 10, 20, 10, 20);
    
    // 模拟 Java 测试场景
    // 写入 25 行内容
    for i in 0..25 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    
    // Rust 计算：
    // - 缓冲区大小 = 20
    // - 屏幕行数 = 10
    // - 最大历史行数 = 20 - 10 = 10
    // - 写入 25 行，滚动 15 次，但受限于最大值，应为 10
    
    let expected_max = 20 - 10;
    let rust_result = engine.state.main_screen.active_transcript_rows;
    
    println!("Rust 结果：active_transcript_rows = {}", rust_result);
    println!("Java 预期：active_transcript_rows = {}", expected_max);
    
    assert_eq!(rust_result, expected_max, "Rust 应与 Java 行为一致");
    
    println!("✅ Test 5 通过：Rust 与 Java 行为一致\n");
}

// =============================================================================
// 主测试入口
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn run_all_active_transcript_tests() {
        test_active_transcript_rows_increment_on_scroll();
        test_active_transcript_rows_max_limit();
        test_active_transcript_rows_on_resize();
        test_active_transcript_rows_alt_screen();
        test_active_transcript_rows_java_comparison();
        
        println!("\n🎉 所有 active_transcript_rows 测试通过！");
    }
}
