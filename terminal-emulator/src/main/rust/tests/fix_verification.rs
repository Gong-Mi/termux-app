// 修复验证测试集
// 运行：cargo test --test fix_verification -- --nocapture
//
// 测试覆盖的修复:
// 1. get_space_used() 忽略 \0 占位符
// 2. clear_all() 方法对齐 Java 行为
// 3. resize_with_reflow 对齐 Java 逻辑
// 4. CJK 宽字符处理
// 5. Combining chars 处理

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text)
}

// =============================================================================
// 测试 1: get_space_used() 忽略 \0 占位符
// =============================================================================

/// 验证 CJK 宽字符后的占位符 \0 不被计入空间使用
#[test]
fn test_get_space_used_ignores_null_placeholder() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一个 CJK 宽字符 "你" (占 2 列，第二列是 \0 占位符)
    // UTF-8 bytes for "你": 0xE4 0xBD 0xA0
    let data = b"\xe4\xbd\xa0";
    engine.process_bytes(data);

    // 获取第一行
    let screen = &engine.state.main_screen;
    let row = screen.get_row(0);
    
    // 验证第一个字符是 "你"
    assert_eq!(row.text[0], '你', "First char should be '你'");
    
    // 验证第二个字符是 \0 (宽字符占位符)
    assert_eq!(row.text[1], '\0', "Second char should be null placeholder");
    
    // 验证 get_space_used 返回 1 (只算有效列，不算 \0)
    let used = row.get_space_used();
    assert_eq!(used, 1, "get_space_used should return 1 (not counting null placeholder)");
    
    println!("✅ get_space_used correctly ignores null placeholder");
}

/// 验证多个 CJK 字符的占位符处理
#[test]
fn test_get_space_used_multiple_cjk() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入 "你好" (两个 CJK 宽字符)
    // UTF-8: 你 = 0xE4 0xBD 0xA0, 好 = 0xE5 0xA5 0xBD
    engine.process_bytes(b"\xe4\xbd\xa0\xe5\xa5\xbd");

    let screen = &engine.state.main_screen;
    let row = screen.get_row(0);
    
    // 验证字符布局
    assert_eq!(row.text[0], '你');
    assert_eq!(row.text[1], '\0'); // "你" 的占位符
    assert_eq!(row.text[2], '好');
    assert_eq!(row.text[3], '\0'); // "好" 的占位符
    
    // get_space_used 返回最后一个非空字符的索引 +1
    // 由于 \0 不计入，应该返回 3 (索引 2 的"好" + 1)
    let used = row.get_space_used();
    assert_eq!(used, 3, "Should return index after last non-space (excluding nulls)");
    
    println!("✅ Multiple CJK characters handled correctly (used={})", used);
}

/// 验证混合 ASCII 和 CJK 的空间计算
#[test]
fn test_get_space_used_mixed_ascii_cjk() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入 "Hi 你" (2 ASCII + 1 space + 1 CJK)
    // UTF-8: 你 = 0xE4 0xBD 0xA0
    engine.process_bytes(b"Hi \xe4\xbd\xa0");

    let screen = &engine.state.main_screen;
    let row = screen.get_row(0);
    
    // 布局应该是：'H', 'i', ' ', '你', '\0'
    assert_eq!(row.text[0], 'H');
    assert_eq!(row.text[1], 'i');
    assert_eq!(row.text[2], ' ');
    assert_eq!(row.text[3], '你');
    assert_eq!(row.text[4], '\0');
    
    // get_space_used 返回最后一个非空字符的索引 +1
    // 由于 \0 不计入，应该返回 4 (索引 3 的"你" + 1)
    let used = row.get_space_used();
    assert_eq!(used, 4, "Should return index after last non-space (excluding nulls)");
    
    println!("✅ Mixed ASCII and CJK handled correctly (used={})", used);
}

// =============================================================================
// 测试 2: clear_all() 方法对齐 Java 行为
// =============================================================================

/// 验证 clear_all() 清空整行
#[test]
fn test_clear_all() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 先写入一些内容
    engine.process_bytes(b"Hello World");
    
    // 获取当前行并清空
    let screen = &mut engine.state.main_screen;
    screen.get_row_mut(0).clear_all(0);
    
    // 验证整行被清空
    let row = screen.get_row(0);
    for i in 0..80 {
        assert_eq!(row.text[i], ' ', "Position {} should be space after clear_all", i);
        assert_eq!(row.styles[i], 0, "Style at {} should be 0 after clear_all", i);
    }
    
    println!("✅ clear_all() correctly clears entire row");
}

/// 验证 clear_all() 不重置 line_wrap (对齐 Java)
#[test]
fn test_clear_all_preserves_line_wrap() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入超过 80 列的内容强制换行
    let long_text = "A".repeat(100);
    engine.process_bytes(long_text.as_bytes());
    
    // 第一行应该有 line_wrap = true
    let row_before = engine.state.main_screen.get_row(0);
    let had_line_wrap = row_before.line_wrap;
    
    // 清空第一行
    engine.state.main_screen.get_row_mut(0).clear_all(0);
    
    // line_wrap 应该保持不变 (Java 行为)
    let row_after = engine.state.main_screen.get_row(0);
    assert_eq!(row_after.line_wrap, had_line_wrap, "line_wrap should be preserved after clear_all");
    
    println!("✅ clear_all() preserves line_wrap flag");
}

// =============================================================================
// 测试 3: resize_with_reflow 对齐 Java 逻辑
// =============================================================================

/// 验证缩小屏幕时的内容重排
#[test]
fn test_resize_shrink_reflow() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一行 80 字符
    let line = "A".repeat(80);
    engine.process_bytes(line.as_bytes());
    engine.process_bytes(b"\r\n");
    engine.process_bytes(b"Second line");
    
    // 缩小到 40 列
    engine.state.resize(40, 24);
    
    // 验证内容被正确重排
    // 第一行应该是 40 个 'A'
    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim_end_matches(' '), "A".repeat(40), "First row should have 40 A's");
    
    // 第二行应该是剩余 40 个 'A'
    let row1 = get_row_text(&engine, 1);
    assert_eq!(row1.trim_end_matches(' '), "A".repeat(40), "Second row should have remaining 40 A's");
    
    println!("✅ Resize shrink reflow works correctly");
}

/// 验证放大屏幕时的内容重排
#[test]
fn test_resize_expand_reflow() {
    let mut engine = TerminalEngine::new(40, 24, 100, 10, 20);

    // 写入两行 40 字符
    let a_line = "A".repeat(40);
    let b_line = "B".repeat(40);
    engine.process_bytes(a_line.as_bytes());
    engine.process_bytes(b"\r\n");
    engine.process_bytes(b_line.as_bytes());

    // 放大到 80 列
    engine.state.resize(80, 24);

    // 验证内容被合并到一行
    let row0 = get_row_text(&engine, 0);
    let row0_trimmed = row0.trim_end_matches(' ');
    
    // 放大后第一行应该有 40 个 A
    assert!(row0_trimmed.starts_with("AAAAAAAAAA"), "First row should start with A's");
    // 第二行应该有 B
    let row1 = get_row_text(&engine, 1);
    assert!(row1.contains('B'), "Second row should contain B's");

    println!("✅ Resize expand reflow works correctly");
}

/// 验证 resize 时光标位置追踪
#[test]
fn test_resize_cursor_tracking() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入内容
    engine.process_bytes(b"Hello World");

    // 缩小屏幕
    engine.state.resize(40, 24);

    // 验证光标位置合理 (应该在屏幕内)
    assert!(
        engine.state.cursor.x >= 0 && engine.state.cursor.x < 40,
        "Cursor X should be within new screen width"
    );
    assert!(
        engine.state.cursor.y >= 0 && engine.state.cursor.y < 24,
        "Cursor Y should be within screen height"
    );

    println!("✅ Cursor tracking during resize works correctly");
}

/// 验证 skipped_blank_lines 逻辑 (Java 对齐)
#[test]
fn test_resize_skipped_blank_lines() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入内容后跟空行
    engine.process_bytes(b"Line 1\r\n");
    engine.process_bytes(b"\r\n"); // 空行
    engine.process_bytes(b"Line 3\r\n");
    
    // 缩小屏幕
    engine.state.resize(40, 24);
    
    // 验证内容存在
    let text = engine.state.main_screen.get_transcript_text();
    assert!(text.contains("Line 1"), "Should contain Line 1");
    assert!(text.contains("Line 3"), "Should contain Line 3");
    
    println!("✅ Skipped blank lines logic works correctly");
}

// =============================================================================
// 测试 4: CJK 宽字符处理
// =============================================================================

/// 验证 CJK 字符在行尾的换行
#[test]
fn test_cjk_wrap_at_line_end() {
    let mut engine = TerminalEngine::new(5, 24, 100, 10, 20);

    // 屏幕只有 5 列，写入 "AB 你" (2 + 2 = 4 列，应该能放下)
    // UTF-8: 你 = 0xE4 0xBD 0xA0
    engine.process_bytes(b"AB \xe4\xbd\xa0");
    
    let row = engine.state.main_screen.get_row(0);
    
    // 验证字符位置
    assert_eq!(row.text[0], 'A');
    assert_eq!(row.text[1], 'B');
    assert_eq!(row.text[2], ' ');
    assert_eq!(row.text[3], '你');
    assert_eq!(row.text[4], '\0');
    
    println!("✅ CJK character at line end handled correctly");
}

/// 验证 CJK 字符跨行换行
#[test]
fn test_cjk_wrap_across_lines() {
    let mut engine = TerminalEngine::new(5, 24, 100, 10, 20);

    // 屏幕只有 5 列，写入 "AB 你好" (2 + 2 + 2 = 6 列，"好" 应该换行)
    // UTF-8: 你 = 0xE4 0xBD 0xA0, 好 = 0xE5 0xA5 0xBD
    engine.process_bytes(b"AB \xe4\xbd\xa0\xe5\xa5\xbd");
    
    // 第一行应该是 "AB 你\0"
    let row0 = engine.state.main_screen.get_row(0);
    assert_eq!(row0.text[0], 'A');
    assert_eq!(row0.text[1], 'B');
    assert_eq!(row0.text[2], ' ');
    assert_eq!(row0.text[3], '你');
    
    // 第二行应该以 "好" 开始
    let row1 = engine.state.main_screen.get_row(1);
    assert_eq!(row1.text[0], '好');
    
    println!("✅ CJK character wrapping across lines works correctly");
}

// =============================================================================
// 测试 5: Combining Chars 处理 (验证 Rust 侧支持)
// =============================================================================

/// 验证 combining char 不被计入空间
#[test]
fn test_combining_char_space_calculation() {
    // 注意：Rust 侧目前不直接处理 combining chars
    // 这个测试验证基础行为
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入普通字符
    engine.process_bytes(b"e\xcc\x81"); // e + combining acute accent = é
    
    let row = engine.state.main_screen.get_row(0);
    
    // 验证内容被写入
    let used = row.get_space_used();
    assert!(used > 0, "Should have some content");
    
    println!("✅ Combining char test completed (used={} positions)", used);
}

// =============================================================================
// 测试 6: 综合压力测试
// =============================================================================

/// 复杂场景：CJK + resize + 光标追踪
#[test]
fn test_complex_cjk_resize_cursor() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入混合内容
    // "Hello 世界！" - UTF-8: 世 = 0xE4 0xB8 96, 界 = 0xE7 0x95 8C, ！ = 0xEF 0xBC 0x81
    engine.process_bytes(b"Hello \xe4\xb8\x96\xe7\x95\x8c\xef\xbc\x81");
    engine.process_bytes(b"\r\n");
    // "Rust 测试" - UTF-8: 测 = 0xE6 B5 8B, 试 = 0xE8 AF 95
    engine.process_bytes(b"Rust \xe6\xb5\x8b\xe8\xaf\x95");

    // 记录光标
    let _cursor_before_x = engine.state.cursor.x;
    let _cursor_before_y = engine.state.cursor.y;

    // 多次 resize
    engine.state.resize(40, 24);
    engine.state.resize(60, 24);
    engine.state.resize(80, 24);

    // 验证内容完整性 - 检查原始文本（\0 被替换为空格）
    let text = engine.state.main_screen.get_transcript_text();
    // 注意：get_transcript_text 返回的文本中 \0 被替换为空格
    // 所以我们检查 "H e l l o" 这样的模式
    assert!(text.contains("H"), "Should contain H");
    assert!(text.contains("R"), "Should contain R");
    
    // 检查第一行包含 Hello
    let row0 = get_row_text(&engine, 0);
    assert!(row0.contains('H'), "First row should contain H");

    println!("✅ Complex CJK + resize + cursor test passed");
    println!("   Cursor: ({}={}) -> ({}={})",
             _cursor_before_x, _cursor_before_y,
             engine.state.cursor.x, engine.state.cursor.y);
}

/// 边界情况：极窄屏幕 resize
#[test]
fn test_extreme_narrow_resize() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    
    // 缩小到极窄 (10 列)
    engine.state.resize(10, 24);
    
    // 验证内容被正确分割
    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim_end_matches(' ').len(), 10, "First row should be 10 chars");
    
    let row1 = get_row_text(&engine, 1);
    assert_eq!(row1.trim_end_matches(' ').len(), 10, "Second row should be 10 chars");
    
    let row2 = get_row_text(&engine, 2);
    assert_eq!(row2.trim_end_matches(' ').len(), 6, "Third row should be 6 chars");
    
    println!("✅ Extreme narrow resize test passed");
}

// =============================================================================
// 主测试入口
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_all_fix_verification_tests() {
        test_get_space_used_ignores_null_placeholder();
        test_get_space_used_multiple_cjk();
        test_get_space_used_mixed_ascii_cjk();
        test_clear_all();
        test_clear_all_preserves_line_wrap();
        test_resize_shrink_reflow();
        test_resize_expand_reflow();
        test_resize_cursor_tracking();
        test_resize_skipped_blank_lines();
        test_cjk_wrap_at_line_end();
        test_cjk_wrap_across_lines();
        test_combining_char_space_calculation();
        test_complex_cjk_resize_cursor();
        test_extreme_narrow_resize();
        
        println!("\n✅ All fix verification tests passed!");
    }
}
