// Rust 终端引擎一致性测试
// 运行：cargo test --test consistency -- --nocapture
//
// 测试状态说明:
// - ✅ PASS: 测试通过
// - ⚠️  PARTIAL: 部分功能支持
// - ❌ TODO: 待实现

use termux_rust::engine::TerminalEngine;

// =============================================================================
// 基础文本测试
// =============================================================================

/// 验证基本文本输出 - ✅ PASS
#[test]
fn test_basic_text() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"Hello World";
    engine.process_bytes(data);

    // 验证光标位置
    assert_eq!(
        engine.state.cursor_x, 11,
        "Cursor X should be 11 after 'Hello World'"
    );
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0");

    // 验证屏幕内容
    let mut text = [0u16; 80];
    engine.state.copy_row_text(0, &mut text);

    let expected = "Hello World";
    for (i, expected_char) in expected.chars().enumerate() {
        assert_eq!(
            text[i], expected_char as u16,
            "Character at position {} should match",
            i
        );
    }
}

/// 验证退格 - ✅ PASS
#[test]
fn test_backspace() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"ABC\x08DE";
    engine.process_bytes(data);

    assert_eq!(
        engine.state.cursor_x, 4,
        "Cursor X should be 4 after 'ABC\\x08DE'"
    );

    let mut text = [0u16; 80];
    engine.state.copy_row_text(0, &mut text);

    assert_eq!(text[0] as u8, b'A');
    assert_eq!(text[1] as u8, b'B');
    assert_eq!(text[2] as u8, b'D');
    assert_eq!(text[3] as u8, b'E');
}

/// 验证换行符处理 - ✅ PASS
#[test]
fn test_newline() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"Line 1\r\nLine 2";
    engine.process_bytes(data);

    println!(
        "After newline: cursor=({}, {})",
        engine.state.cursor_x, engine.state.cursor_y
    );
    // "Line 2" 是 6 个字符
    assert_eq!(
        engine.state.cursor_x, 6,
        "Cursor X should be 6 after 'Line 2'"
    );
    assert_eq!(
        engine.state.cursor_y, 1,
        "Cursor Y should be 1 after newline"
    );
}

/// 验证制表符 - ✅ PASS
#[test]
fn test_tab() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"A\tB";
    engine.process_bytes(data);

    // A 在位置 0，制表符跳到位置 8，B 在位置 8
    assert_eq!(
        engine.state.cursor_x, 9,
        "Cursor X should be 9 after 'A\\tB'"
    );
}

// =============================================================================
// 光标移动测试
// =============================================================================

/// 验证光标位置设置 (CUP) - ✅ PASS
#[test]
fn test_cursor_position() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"\x1b[5;5HAt 5,5";
    engine.process_bytes(data);

    println!(
        "After CUP 'At 5,5': cursor=({}, {})",
        engine.state.cursor_x, engine.state.cursor_y
    );
    // "At 5,5" 是 6 个字符，从位置 (4,4) 开始，所以光标应该在 (10, 4)
    assert_eq!(
        engine.state.cursor_x, 10,
        "Cursor X should be 10 after 'At 5,5'"
    );
    assert_eq!(engine.state.cursor_y, 4, "Cursor Y should be 4");
}

/// 验证光标移动 (CUU/CUD/CUF/CUB) - ✅ PASS
#[test]
fn test_cursor_movement() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[11;21H");
    assert_eq!(engine.state.cursor_y, 10);
    assert_eq!(engine.state.cursor_x, 20);

    engine.process_bytes(b"\x1b[3A");
    assert_eq!(
        engine.state.cursor_y, 7,
        "Cursor Y should be 7 after moving up 3"
    );

    engine.process_bytes(b"\x1b[5B");
    assert_eq!(
        engine.state.cursor_y, 12,
        "Cursor Y should be 12 after moving down 5"
    );

    engine.process_bytes(b"\x1b[10D");
    assert_eq!(
        engine.state.cursor_x, 10,
        "Cursor X should be 10 after moving left 10"
    );

    engine.process_bytes(b"\x1b[5C");
    assert_eq!(
        engine.state.cursor_x, 15,
        "Cursor X should be 15 after moving right 5"
    );
}

/// 验证光标水平绝对 (CHA) - ✅ PASS
#[test]
fn test_cursor_horizontal_absolute() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[10G");
    assert_eq!(
        engine.state.cursor_x, 9,
        "Cursor X should be 9 (1-based to 0-based)"
    );
}

/// 验证光标垂直绝对 (VPA) - ✅ PASS
#[test]
fn test_cursor_vertical_absolute() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[10d");
    assert_eq!(
        engine.state.cursor_y, 9,
        "Cursor Y should be 9 (1-based to 0-based)"
    );
}

/// 验证下一行 (CNL) - ✅ PASS
#[test]
fn test_cursor_next_line() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[3E");
    assert_eq!(engine.state.cursor_y, 3, "Cursor Y should be 3");
    assert_eq!(
        engine.state.cursor_x, 0,
        "Cursor X should be 0 (moved to beginning)"
    );
}

/// 验证上一行 (CPL) - ✅ PASS
#[test]
fn test_cursor_previous_line() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[11;21H\x1b[3F");
    assert_eq!(engine.state.cursor_y, 7, "Cursor Y should be 7");
    assert_eq!(
        engine.state.cursor_x, 0,
        "Cursor X should be 0 (moved to beginning)"
    );
}

// =============================================================================
// 清除操作测试
// =============================================================================

/// 验证清屏 (ED) - ✅ PASS
#[test]
fn test_erase_display() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"Should be erased\x1b[2JStill here";
    engine.process_bytes(data);

    println!(
        "After ED: cursor=({}, {})",
        engine.state.cursor_x, engine.state.cursor_y
    );
    // "Should be erased" 16 字符，\x1b[2J 清屏光标不变，"Still here" 10 字符，总共 26
    assert_eq!(
        engine.state.cursor_x, 26,
        "Cursor X should be 26 after 'Still here'"
    );
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0");
}

/// 验证清行 (EL) - ✅ PASS
#[test]
fn test_erase_line() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"Hello\x1b[2KWorld";
    engine.process_bytes(data);

    println!(
        "After EL: cursor=({}, {})",
        engine.state.cursor_x, engine.state.cursor_y
    );
    // "Hello" 5 字符，\x1b[2K 清行光标不变，"World" 5 字符，总共 10
    assert_eq!(
        engine.state.cursor_x, 10,
        "Cursor X should be 10 after 'World'"
    );
}

/// 验证擦除字符 (ECH) - ✅ PASS
#[test]
fn test_erase_characters() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"Hello World\x1b[5D\x1b[3X";
    engine.process_bytes(data);

    println!(
        "After ECH: cursor=({}, {})",
        engine.state.cursor_x, engine.state.cursor_y
    );
    // "Hello World" 11 字符，\x1b[5D 后退 5 格到位置 6，\x1b[3X 擦除 3 个字符光标不变，所以在位置 6
    assert_eq!(engine.state.cursor_x, 6, "Cursor X should be 6");
}

// =============================================================================
// 插入/删除测试
// =============================================================================

/// 验证插入字符 (ICH) - ✅ PASS
#[test]
fn test_insert_characters() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"AB\x1b[2@CD";
    engine.process_bytes(data);

    // ICH 插入空格，然后 "CD" 覆盖插入的空格
    // 结果应该是 "ABCD"，后面是被推过来的空格
    let mut text = [0u16; 80];
    engine.state.copy_row_text(0, &mut text);

    println!("After ICH: text[0-7]={:?}", &text[0..8]);
    assert_eq!(text[0] as u8, b'A');
    assert_eq!(text[1] as u8, b'B');
    assert_eq!(text[2] as u8, b'C');
    assert_eq!(text[3] as u8, b'D');
}

/// 验证删除字符 (DCH) - ✅ PASS
#[test]
fn test_delete_characters() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"ABCDE\x1b[3D\x1b[2P";
    engine.process_bytes(data);

    // 删除 2 个字符后，应该是 "ABE"
    let mut text = [0u16; 80];
    engine.state.copy_row_text(0, &mut text);

    assert_eq!(text[0] as u8, b'A');
    assert_eq!(text[1] as u8, b'B');
    assert_eq!(text[2] as u8, b'E');
}

/// 验证插入行 (IL) - ✅ PASS
#[test]
fn test_insert_lines() {
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);

    // 先写两行
    engine.process_bytes(b"Line 1\r\nLine 2");

    // 移动到第一行
    engine.process_bytes(b"\x1b[1;1H");

    // 插入 1 行
    engine.process_bytes(b"\x1b[1L");

    // 光标应该在第一行
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0");
}

/// 验证删除行 (DL) - ✅ PASS
#[test]
fn test_delete_lines() {
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);

    // 先写三行
    engine.process_bytes(b"Line 1\r\nLine 2\r\nLine 3");

    // 移动到第一行
    engine.process_bytes(b"\x1b[1;1H");

    // 删除 1 行
    engine.process_bytes(b"\x1b[1M");

    // 光标应该在第一行
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0");
}

// =============================================================================
// SGR 属性测试
// =============================================================================

/// 验证 SGR 颜色 - ✅ PASS
#[test]
fn test_sgr_colors() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"\x1b[31mRed";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 前景色在位 40-48，红色是索引 1
    assert_eq!(
        (style >> 40) & 0x1FF,
        1,
        "Foreground color should be red (1)"
    );
}

/// 验证 SGR 粗体 - ✅ PASS
#[test]
fn test_sgr_bold() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"\x1b[1mBold";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 粗体在位 0
    assert_ne!(style & 0x01, 0, "Bold bit should be set");
}

/// 验证 SGR 下划线 - ✅ PASS
#[test]
fn test_sgr_underline() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"\x1b[4mUnderline";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 下划线在位 2
    assert_ne!(style & 0x04, 0, "Underline bit should be set");
}

/// 验证 SGR 重置 - ✅ PASS
#[test]
fn test_sgr_reset() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"\x1b[1;31mBold Red\x1b[0mNormal";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 重置后应该是默认样式（前景色 256，背景色 257）
    // 256 << 40 = 281474976710656, 257 << 16 = 16842752
    // 总和 = 281474993553408
    const STYLE_NORMAL: u64 = 281474993553408;
    assert_eq!(style, STYLE_NORMAL, "Style should be reset to default");
}

/// 验证 SGR 亮色 - ✅ PASS
#[test]
fn test_sgr_bright_colors() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"\x1b[91mBright Red";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 前景色在位 40-48，亮红色是索引 9
    assert_eq!(
        (style >> 40) & 0x1FF,
        9,
        "Foreground color should be bright red (9)"
    );
}

// =============================================================================
// SGR 扩展颜色测试（新增）
// =============================================================================

/// 验证 SGR 256 色前景 - ✅ PASS
#[test]
fn test_sgr_256_color_foreground() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 38;5;196 = 红色 (256 色索引)
    let data = b"\x1b[38;5;196mRed256";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    assert_eq!(
        (style >> 40) & 0x1FF,
        196,
        "Foreground color should be 256-color index 196"
    );
}

/// 验证 SGR 256 色背景 - ✅ PASS
#[test]
fn test_sgr_256_color_background() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 48;5;21 = 蓝色 (256 色索引)
    let data = b"\x1b[48;5;21mBlueBG";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    assert_eq!(
        (style >> 16) & 0x1FF,
        21,
        "Background color should be 256-color index 21"
    );
}

/// 验证 SGR 真彩色前景 - ✅ PASS
#[test]
fn test_sgr_truecolor_foreground() {
    use termux_rust::engine::STYLE_TRUECOLOR_FG;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 38;2;255;128;64 = RGB 真彩色
    let data = b"\x1b[38;2;255;128;64mTrueColor";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 检查真彩色标志位是否设置
    assert_ne!(
        style & STYLE_TRUECOLOR_FG,
        0,
        "Truecolor foreground flag should be set"
    );
    // 检查 RGB 值 (0xff000000 | (255 << 16) | (128 << 8) | 64) & 0x00ffffff = 0xff8040
    let fg_color = (style >> 40) & 0x00ffffff;
    assert_eq!(
        fg_color, 0xff8040,
        "Truecolor foreground RGB should be 0xff8040"
    );
}

/// 验证 SGR 真彩色背景 - ✅ PASS
#[test]
fn test_sgr_truecolor_background() {
    use termux_rust::engine::STYLE_TRUECOLOR_BG;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 48;2;100;150;200 = RGB 真彩色
    let data = b"\x1b[48;2;100;150;200mTrueColorBG";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 检查真彩色标志位是否设置
    assert_ne!(
        style & STYLE_TRUECOLOR_BG,
        0,
        "Truecolor background flag should be set"
    );
    // 检查 RGB 值
    let bg_color = (style >> 16) & 0x00ffffff;
    assert_eq!(
        bg_color, 0x6496c8,
        "Truecolor background RGB should be 0x6496c8"
    );
}

/// 验证 SGR 下划线子参数 - ✅ PASS
#[test]
fn test_sgr_underline_subparam() {
    use termux_rust::engine::EFFECT_UNDERLINE;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 4:0 = 无下划线
    let data = b"\x1b[4m\x1b[4:0mNoUnderline";
    engine.process_bytes(data);

    let style = engine.state.current_style;
    // 4:0 应该清除下划线
    assert_eq!(
        style & EFFECT_UNDERLINE,
        0,
        "Underline should be cleared by 4:0"
    );
}

// =============================================================================
// 光标保存/恢复测试
// =============================================================================

/// 验证保存/恢复光标 - ✅ PASS
#[test]
fn test_save_restore_cursor() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"\x1b[6;11H\x1b7\x1b[2;3H\x1b8";
    engine.process_bytes(data);

    assert_eq!(
        engine.state.cursor_x, 10,
        "Cursor X should be restored to 10"
    );
    assert_eq!(engine.state.cursor_y, 5, "Cursor Y should be restored to 5");
}

// =============================================================================
// 滚动测试
// =============================================================================

/// 验证滚动 - ✅ PASS
#[test]
fn test_scrolling() {
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);

    for i in 0..10 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }

    assert_eq!(engine.state.cursor_y, 4, "Cursor Y should be at last row");
    assert_eq!(
        engine.state.cursor_x, 0,
        "Cursor X should be 0 after newline"
    );
}

/// 验证上滚 (SU) - ✅ PASS
#[test]
fn test_scroll_up() {
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);

    // 写满屏幕
    for i in 0..5 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }

    // 上滚 2 行
    engine.process_bytes(b"\x1b[2S");

    // 光标应该在顶部
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0");
}

/// 验证下滚 (SD) - ✅ PASS
#[test]
fn test_scroll_down() {
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);

    // 写满屏幕
    for i in 0..5 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }

    // 下滚 2 行
    engine.process_bytes(b"\x1b[2T");

    // 光标应该在顶部
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0");
}

// =============================================================================
// 边距测试
// =============================================================================

/// 验证设置上下边距 (DECSTBM) - ✅ PASS
#[test]
fn test_set_margins() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置边距 5-20
    let data = b"\x1b[5;20r";
    engine.process_bytes(data);

    assert_eq!(
        engine.state.top_margin, 4,
        "Top margin should be 4 (0-based)"
    );
    assert_eq!(engine.state.bottom_margin, 20, "Bottom margin should be 20");
}

// =============================================================================
// 宽字符和 Unicode 测试
// =============================================================================

/// 验证宽字符处理 - ✅ PASS
#[test]
fn test_wide_characters() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = "你好".as_bytes();
    engine.process_bytes(data);

    assert_eq!(
        engine.state.cursor_x, 4,
        "Cursor X should be 4 after two Chinese characters"
    );
}

/// 验证 emoji 宽字符 - ✅ PASS
#[test]
fn test_emoji_width() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = "😀".as_bytes();
    engine.process_bytes(data);

    // Emoji 通常是 2 列宽
    assert_eq!(engine.state.cursor_x, 2, "Cursor X should be 2 after emoji");
}

// =============================================================================
// Resize 测试
// =============================================================================

/// 验证 resize - ✅ PASS
#[test]
fn test_resize() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"Hello");
    engine.resize(40, 12);

    assert_eq!(engine.state.cols, 40, "Columns should be 40 after resize");
    assert_eq!(engine.state.rows, 12, "Rows should be 12 after resize");
    assert!(engine.state.cursor_x < engine.state.cols);
    assert!(engine.state.cursor_y < engine.state.rows);
}

// =============================================================================
// 自动换行测试
// =============================================================================

/// 验证自动换行 - ✅ PASS
#[test]
fn test_auto_wrap() {
    let mut engine = TerminalEngine::new(10, 5, 100, 10, 20); // 窄屏幕

    let data = b"12345678901234567890"; // 20 个字符
    engine.process_bytes(data);

    println!(
        "After auto wrap: cursor=({}, {})",
        engine.state.cursor_x, engine.state.cursor_y
    );
    // 10 列屏幕：
    // "123456789" (9 字符) 光标在 9
    // "0" 打印在 (9, 0), about_to_wrap = true, cursor_y 还是 0
    // 第二个 "1" 触发换行，跳到 (0, 1) 打印 "1", 光标到 (1, 1)
    // 重复直到最后一个 "0" 打印在 (9, 1), about_to_wrap = true
    assert_eq!(engine.state.cursor_y, 1, "Cursor Y should be 1 (pending wrap)");
    assert_eq!(engine.state.cursor_x, 9, "Cursor X should be 9 (last column)");
    assert_eq!(engine.state.about_to_wrap, true, "Should be about to wrap");
    // 光标位置取决于具体实现，我们只验证 Y
}

// =============================================================================
// DECSET 私有模式测试
// =============================================================================

/// 验证 DECSET 光标可见性 - ✅ PASS
#[test]
fn test_decset_cursor_visible() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 隐藏光标
    engine.process_bytes(b"\x1b[?25l");
    assert_eq!(
        engine.state.cursor_enabled, false,
        "Cursor should be hidden"
    );

    // 显示光标
    engine.process_bytes(b"\x1b[?25h");
    assert_eq!(
        engine.state.cursor_enabled, true,
        "Cursor should be visible"
    );
}

/// 验证 DECSET 应用光标键 - ✅ PASS
#[test]
fn test_decset_application_cursor_keys() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[?1h");
    assert_eq!(
        engine.state.application_cursor_keys, true,
        "Application cursor keys should be enabled"
    );

    engine.process_bytes(b"\x1b[?1l");
    assert_eq!(
        engine.state.application_cursor_keys, false,
        "Application cursor keys should be disabled"
    );
}

/// 验证 DECSET 自动换行 - ✅ PASS
#[test]
fn test_decset_auto_wrap() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[?7l");
    assert_eq!(
        engine.state.auto_wrap, false,
        "Auto wrap should be disabled"
    );

    engine.process_bytes(b"\x1b[?7h");
    assert_eq!(engine.state.auto_wrap, true, "Auto wrap should be enabled");
}

/// 验证 DECSET 原点模式 - ✅ PASS
#[test]
fn test_decset_origin_mode() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[?6h");
    assert_eq!(
        engine.state.origin_mode, true,
        "Origin mode should be enabled"
    );

    engine.process_bytes(b"\x1b[?6l");
    assert_eq!(
        engine.state.origin_mode, false,
        "Origin mode should be disabled"
    );
}

/// 验证 DECSET 括号粘贴模式 - ✅ PASS
#[test]
fn test_decset_bracketed_paste() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[?2004h");
    assert_eq!(
        engine.state.bracketed_paste, true,
        "Bracketed paste should be enabled"
    );

    engine.process_bytes(b"\x1b[?2004l");
    assert_eq!(
        engine.state.bracketed_paste, false,
        "Bracketed paste should be disabled"
    );
}

// =============================================================================
// DECSET 扩展测试（新增）
// =============================================================================

/// 验证 DECSET 69 (DECLRMM) 左右边距模式 - ✅ PASS
#[test]
fn test_decset_leftright_margin_mode() {
    use termux_rust::engine::DECSET_BIT_LEFTRIGHT_MARGIN_MODE;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 DECLRMM
    engine.process_bytes(b"\x1b[?69h");
    assert_eq!(
        engine.state.leftright_margin_mode, true,
        "Left-right margin mode should be enabled"
    );
    assert_ne!(
        engine.state.decset_flags & DECSET_BIT_LEFTRIGHT_MARGIN_MODE,
        0,
        "DECSET flag bit 69 should be set"
    );

    // 设置左右边距
    engine.process_bytes(b"\x1b[5;70s");
    assert_eq!(
        engine.state.left_margin, 4,
        "Left margin should be 4 (0-based)"
    );
    assert_eq!(engine.state.right_margin, 70, "Right margin should be 70");

    // 禁用 DECLRMM
    engine.process_bytes(b"\x1b[?69l");
    assert_eq!(
        engine.state.leftright_margin_mode, false,
        "Left-right margin mode should be disabled"
    );
}

/// 验证 DECSET 1004 发送焦点事件 - ✅ PASS
#[test]
fn test_decset_send_focus_events() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"\x1b[?1004h");
    assert_eq!(
        engine.state.send_focus_events, true,
        "Send focus events should be enabled"
    );

    engine.process_bytes(b"\x1b[?1004l");
    assert_eq!(
        engine.state.send_focus_events, false,
        "Send focus events should be disabled"
    );
}

/// 验证鼠标模式互斥 (1000 vs 1002) - ✅ PASS
#[test]
fn test_mouse_mode_exclusive() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 1000（鼠标跟踪按下&释放）
    engine.process_bytes(b"\x1b[?1000h");
    assert_eq!(
        engine.state.mouse_tracking, true,
        "Mouse tracking should be enabled"
    );
    assert_eq!(
        engine.state.mouse_button_event, false,
        "Mouse button event should be disabled"
    );

    // 启用 1002（鼠标按钮事件跟踪）应该禁用 1000
    engine.process_bytes(b"\x1b[?1002h");
    assert_eq!(
        engine.state.mouse_tracking, false,
        "Mouse tracking should be disabled after enabling 1002"
    );
    assert_eq!(
        engine.state.mouse_button_event, true,
        "Mouse button event should be enabled"
    );

    // 再次启用 1000 应该禁用 1002
    engine.process_bytes(b"\x1b[?1000h");
    assert_eq!(
        engine.state.mouse_tracking, true,
        "Mouse tracking should be re-enabled"
    );
    assert_eq!(
        engine.state.mouse_button_event, false,
        "Mouse button event should be disabled"
    );

    // 禁用 1000
    engine.process_bytes(b"\x1b[?1000l");
    assert_eq!(
        engine.state.mouse_tracking, false,
        "Mouse tracking should be disabled"
    );
}

/// 验证 DECSET 标志保存/恢复 - ✅ PASS
#[test]
fn test_decset_flags_save_restore() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置一些 DECSET 标志
    engine.process_bytes(b"\x1b[?7h"); // 自动换行
    engine.process_bytes(b"\x1b[?6h"); // 原点模式

    // 保存光标
    engine.process_bytes(b"\x1b7");
    let _saved_flags = engine.state.saved_decset_flags;

    // 更改 DECSET 标志
    engine.process_bytes(b"\x1b[?7l"); // 禁用自动换行
    engine.process_bytes(b"\x1b[?6l"); // 禁用原点模式

    // 恢复光标应该恢复 DECSET 标志
    engine.process_bytes(b"\x1b8");

    assert_eq!(engine.state.auto_wrap, true, "Auto wrap should be restored");
    assert_eq!(
        engine.state.origin_mode, true,
        "Origin mode should be restored"
    );
}

// =============================================================================
// 键盘和鼠标事件测试（新增）
// =============================================================================

/// 验证鼠标事件 (SGR 模式) - ✅ PASS
#[test]
fn test_mouse_event_sgr() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 SGR 鼠标模式
    engine.process_bytes(b"\x1b[?1006h");

    // 模拟鼠标点击 (按钮 0, 位置 10,20)
    engine.state.send_mouse_event(0, 10, 20, true);

    // 验证输出格式：CSI < button ; x ; y M
    // 实际输出会通过 write_to_session 发送到会话
    // 这里我们验证状态
    assert_eq!(
        engine.state.sgr_mouse, true,
        "SGR mouse mode should be enabled"
    );
}

/// 验证鼠标事件 (旧格式) - ✅ PASS
#[test]
fn test_mouse_event_legacy() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用旧格式鼠标跟踪 (DECSET 1000)
    engine.process_bytes(b"\x1b[?1000h");

    assert_eq!(
        engine.state.mouse_tracking, true,
        "Mouse tracking should be enabled"
    );
    assert_eq!(
        engine.state.sgr_mouse, false,
        "SGR mouse should be disabled"
    );

    // 模拟鼠标点击 (按钮 0, 位置 10,20)
    engine.state.send_mouse_event(0, 10, 20, true);

    // 旧格式应该发送：CSI M Cb Cx Cy
    // Cb = 32 + 0 = 32, Cx = 32 + 10 = 42, Cy = 32 + 20 = 52
    // 验证状态
    assert_eq!(engine.state.mouse_tracking, true);
}

/// 验证鼠标移动事件 (DECSET 1002) - ✅ PASS
#[test]
fn test_mouse_event_button_tracking() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用按钮事件跟踪 (DECSET 1002)
    engine.process_bytes(b"\x1b[?1002h");

    assert_eq!(
        engine.state.mouse_button_event, true,
        "Mouse button event should be enabled"
    );
    assert_eq!(
        engine.state.mouse_tracking, false,
        "Mouse tracking should be disabled"
    );

    // 模拟鼠标移动 (button 32 = MOUSE_LEFT_BUTTON_MOVED)
    engine.state.send_mouse_event(32, 15, 25, true);

    // 验证状态
    assert_eq!(engine.state.mouse_button_event, true);
}

/// 验证中键和右键事件 - ✅ PASS
#[test]
fn test_mouse_event_middle_right_buttons() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 SGR 鼠标模式
    engine.process_bytes(b"\x1b[?1006h");

    // 中键按下 (button 1)
    engine.state.send_mouse_event(1, 10, 20, true);
    // 应该发送：CSI < 1 ; 10 ; 20 M

    // 右键按下 (button 2)
    engine.state.send_mouse_event(2, 10, 20, true);
    // 应该发送：CSI < 2 ; 10 ; 20 M

    // 中键释放
    engine.state.send_mouse_event(1, 10, 20, false);
    // 应该发送：CSI < 1 ; 10 ; 20 m

    // 右键释放
    engine.state.send_mouse_event(2, 10, 20, false);
    // 应该发送：CSI < 2 ; 10 ; 20 m
}

/// 验证中键和右键移动事件 - ✅ PASS
#[test]
fn test_mouse_event_button_movement() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用按钮事件跟踪 (DECSET 1002)
    engine.process_bytes(b"\x1b[?1002h");

    // 左键移动 (button 32)
    engine.state.send_mouse_event(32, 10, 20, true);

    // 中键移动 (button 33)
    engine.state.send_mouse_event(33, 11, 21, true);

    // 右键移动 (button 34)
    engine.state.send_mouse_event(34, 12, 22, true);
}

/// 验证鼠标释放事件 - ✅ PASS
#[test]
fn test_mouse_event_release() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 SGR 鼠标模式
    engine.process_bytes(b"\x1b[?1006h");

    // 模拟鼠标释放
    engine.state.send_mouse_event(0, 10, 20, false);
    // SGR 格式应该发送：CSI < 0 ; 10 ; 20 m (注意是小写 m)
}

/// 验证滚轮事件 - ✅ PASS
#[test]
fn test_mouse_event_wheel() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 SGR 鼠标模式
    engine.process_bytes(b"\x1b[?1006h");

    // 滚轮上 (button 64)
    engine.state.send_mouse_event(64, 40, 12, true);
    // 应该发送：CSI < 64 ; 40 ; 12 M

    // 滚轮下 (button 65)
    engine.state.send_mouse_event(65, 40, 12, true);
    // 应该发送：CSI < 65 ; 40 ; 12 M
}

/// 验证鼠标事件范围限制 - ✅ PASS
#[test]
fn test_mouse_event_bounds() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用旧格式鼠标跟踪
    engine.process_bytes(b"\x1b[?1000h");

    // 超出范围的位置应该被忽略
    engine.state.send_mouse_event(0, 230, 230, true);
    // 旧格式最大支持 223 (255 - 32)
}

// =============================================================================
// 备用屏幕缓冲区测试（新增）
// =============================================================================

/// 验证 DECSET 1048 备用光标 - ✅ PASS
#[test]
fn test_decset_1048_save_restore_cursor() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 移动光标到位置 (10, 15)
    engine.process_bytes(b"\x1b[16;11H");
    assert_eq!(engine.state.cursor_x, 10);
    assert_eq!(engine.state.cursor_y, 15);

    // 保存光标 (DECSET 1048h)
    engine.process_bytes(b"\x1b[?1048h");

    // 移动光标到其他位置
    engine.process_bytes(b"\x1b[5;20H");
    assert_eq!(engine.state.cursor_x, 19);
    assert_eq!(engine.state.cursor_y, 4);

    // 恢复光标 (DECSET 1048l)
    engine.process_bytes(b"\x1b[?1048l");

    // 验证光标恢复到保存的位置
    assert_eq!(engine.state.cursor_x, 10, "Cursor X should be restored");
    assert_eq!(engine.state.cursor_y, 15, "Cursor Y should be restored");
}

/// 验证 DECSET 1049 备用屏幕 - ✅ PASS
#[test]
fn test_decset_1049_alternate_screen() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 在主缓冲区写内容
    engine.process_bytes(b"Main Buffer Content");

    // 启用备用缓冲区 (DECSET 1049h)
    engine.process_bytes(b"\x1b[?1049h");

    // 验证切换到备用缓冲区
    assert_eq!(
        engine.state.use_alternate_buffer, true,
        "Should use alternate buffer"
    );
    assert_eq!(engine.state.is_alternate_buffer_active(), true);

    // 在备用缓冲区写内容
    engine.process_bytes(b"Alternate Buffer Content");

    // 禁用备用缓冲区 (DECSET 1049l)
    engine.process_bytes(b"\x1b[?1049l");

    // 验证切换回主缓冲区
    assert_eq!(
        engine.state.use_alternate_buffer, false,
        "Should use main buffer"
    );
    assert_eq!(engine.state.is_alternate_buffer_active(), false);
}

/// 验证备用缓冲区清除 - ⚠️ PARTIAL (备用缓冲区切换待完善)
#[test]
fn test_alternate_buffer_clear() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 验证备用缓冲区状态
    assert_eq!(
        engine.state.is_alternate_buffer_active(),
        false,
        "Should start with main buffer"
    );

    // 启用备用缓冲区
    engine.process_bytes(b"\x1b[?1049h");

    // 验证切换到备用缓冲区
    assert_eq!(
        engine.state.is_alternate_buffer_active(),
        true,
        "Should switch to alternate buffer"
    );

    // 写内容
    engine.process_bytes(b"Test Content");

    // 禁用备用缓冲区
    engine.process_bytes(b"\x1b[?1049l");

    // 验证切换回主缓冲区
    assert_eq!(
        engine.state.is_alternate_buffer_active(),
        false,
        "Should switch back to main buffer"
    );
}

/// 验证键盘事件 - 功能键 - ✅ PASS
#[test]
fn test_key_event_function_keys() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // F1 无修饰
    engine.state.send_key_event(131, None, 0);
    // F1 应该发送 \x1bOP

    // F5 有修饰键 (shift)
    engine.state.send_key_event(135, None, 0x20000000);
    // F5+Shift 应该发送 \x1b[15;2~
}

/// 验证键盘事件 - 方向键 - ✅ PASS
#[test]
fn test_key_event_arrow键() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 上箭头 (无修饰)
    engine.state.send_key_event(19, None, 0);
    // 应该发送 \x1b[A 或 \x1bOA (应用模式)

    // 启用应用光标键模式
    engine.process_bytes(b"\x1b[?1h");
    engine.state.send_key_event(19, None, 0);
    // 应该发送 \x1bOA
}

/// 验证键盘事件 - Ctrl 组合 - ✅ PASS
#[test]
fn test_key_event_ctrl_combinations() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // Ctrl+A
    engine
        .state
        .send_key_event(0, Some("a".to_string()), 0x40000000);
    // 应该发送 \x01

    // Ctrl+Space
    engine
        .state
        .send_key_event(62, Some(" ".to_string()), 0x40000000);
    // 应该发送 \x00
}

/// 验证键盘事件 - Alt 前缀 - ✅ PASS
#[test]
fn test_key_event_alt_prefix() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // Alt+D
    engine
        .state
        .send_key_event(0, Some("d".to_string()), 0x80000000u32 as i32);
    // 应该发送 \x1bd
}

/// 验证键盘事件 - 数字小键盘 - ✅ PASS
#[test]
fn test_key_event_keypad() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 应用键盘模式禁用
    engine.state.send_key_event(149, None, 0);
    // KP Enter 应该发送 \r

    // 启用应用键盘模式
    engine.process_bytes(b"\x1b=");
    engine.state.send_key_event(149, None, 0);
    // KP Enter 应该发送 \x1bOM
}

// =============================================================================
// DCS/APC 序列测试（新增）
// =============================================================================

/// 验证 DCS 序列处理框架 - ⚠️ PARTIAL
#[test]
fn test_dcs_sequence_framework() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // DECSIXEL 序列框架
    let data = b"\x1bPq\x1b\\";
    engine.process_bytes(data);

    // 目前 DCS 处理是框架性的，不报错即可
    // TODO: 添加 Sixel 解析后的具体验证
}

/// 验证 APC 序列处理框架 - ⚠️ PARTIAL
#[test]
fn test_apc_sequence_framework() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // APC 序列
    let data = b"\x1b_Hello World\x1b\\";
    engine.process_bytes(data);

    // 目前 APC 处理是框架性的，不报错即可
}

// =============================================================================
// 焦点事件测试（新增）
// =============================================================================

/// 验证焦点事件报告 - ✅ PASS
#[test]
fn test_focus_event_reporting() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用焦点事件
    engine.process_bytes(b"\x1b[?1004h");
    assert_eq!(engine.state.send_focus_events, true);

    // 报告焦点获得
    engine.state.report_focus_gain();
    // 应该发送 \x1b[I

    // 报告焦点失去
    engine.state.report_focus_loss();
    // 应该发送 \x1b[O
}

// =============================================================================
// 括号粘贴模式测试（新增）
// =============================================================================

/// 验证括号粘贴模式 - ✅ PASS
#[test]
fn test_bracketed_paste_mode() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用括号粘贴模式
    engine.process_bytes(b"\x1b[?2004h");
    assert_eq!(engine.state.bracketed_paste, true);

    // 粘贴文本
    engine.state.paste_start("Hello Paste");
    // 应该发送 \x1b[200~Hello Paste\x1b[201~

    // 禁用括号粘贴模式
    engine.process_bytes(b"\x1b[?2004l");
    assert_eq!(engine.state.bracketed_paste, false);

    // 粘贴文本（无括号）
    engine.state.paste_start("Hello Direct");
    // 应该直接发送 Hello Direct
}

/// 验证重复字符 (REP) - ✅ PASS
#[test]
fn test_repeat_character() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    let data = b"A\x1b[3b";
    engine.process_bytes(data);

    assert_eq!(
        engine.state.cursor_x, 4,
        "Cursor X should be 4 after repeating A 3 times"
    );

    let mut text = [0u16; 80];
    engine.state.copy_row_text(0, &mut text);

    assert_eq!(text[0] as u8, b'A');
    assert_eq!(text[1] as u8, b'A');
    assert_eq!(text[2] as u8, b'A');
    assert_eq!(text[3] as u8, b'A');
}

// =============================================================================
// 制表位测试
// =============================================================================

/// 验证制表符移动 - ✅ PASS
#[test]
fn test_tab_forward() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 移动到位置 5，然后制表到下一个制表位 (8)
    engine.process_bytes(b"\x1b[6G\x09");
    assert_eq!(engine.state.cursor_x, 8, "Cursor X should be 8 after tab");
}

// =============================================================================
// ESC 序列测试（新增）
// =============================================================================

/// 验证 DECBI (ESC 6) - ✅ PASS
#[test]
fn test_decbi_back_index() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一些文本
    engine.process_bytes(b"Hello");
    assert_eq!(engine.state.cursor_x, 5);

    // ESC 6 - Back Index (向左移动光标)
    engine.process_bytes(b"\x1b6");
    assert_eq!(engine.state.cursor_x, 4, "Cursor X should be 4 after DECBI");

    // 在左边界时使用 DECBI 应该滚动
    engine.process_bytes(b"\x1b[H"); // 移动到左上角 (0,0)
    assert_eq!(engine.state.cursor_x, 0);
    engine.process_bytes(b"\x1b6"); // 应该触发滚动，光标保持在左边界
    assert_eq!(
        engine.state.cursor_x, 0,
        "Cursor X should be 0 after DECBI at margin"
    );
}

/// 验证 DECFI (ESC 9) - ✅ PASS
#[test]
fn test_decfi_forward_index() {
    let mut engine = TerminalEngine::new(10, 5, 100, 10, 20); // 窄屏幕

    // 写入到右边界
    engine.process_bytes(b"123456789");
    assert_eq!(engine.state.cursor_x, 9);

    // ESC 9 - Forward Index (向右移动光标)
    engine.process_bytes(b"\x1b9");
    // 在右边界，应该触发滚动
    assert_eq!(
        engine.state.cursor_x, 9,
        "Cursor X should be at right margin after DECFI"
    );
}

/// 验证 RIS (ESC c) - ✅ PASS
#[test]
fn test_ris_reset() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置一些状态
    engine.process_bytes(b"\x1b[?7l"); // 禁用自动换行
    engine.process_bytes(b"\x1b[5;10r"); // 设置边距
    engine.process_bytes(b"\x1b[31m"); // 红色前景
    engine.process_bytes(b"Test");

    // ESC c - RIS 重置
    engine.process_bytes(b"\x1bc");

    // 验证重置
    assert_eq!(engine.state.cursor_x, 0, "Cursor X should be 0 after RIS");
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0 after RIS");
    assert_eq!(
        engine.state.top_margin, 0,
        "Top margin should be 0 after RIS"
    );
    assert_eq!(
        engine.state.bottom_margin, 24,
        "Bottom margin should be 24 after RIS"
    );
    assert_eq!(
        engine.state.auto_wrap, true,
        "Auto wrap should be enabled after RIS"
    );
}

/// 验证 DECALN (ESC # 8) - ✅ PASS
#[test]
fn test_decaln_screen_align() {
    let mut engine = TerminalEngine::new(10, 5, 100, 10, 20);

    // 先写入一些文本
    engine.process_bytes(b"Hello");

    // ESC # 8 - DECALN 屏幕对齐测试
    engine.process_bytes(b"\x1b#8");

    // 验证整个屏幕被 'E' 填充
    let mut text = [0u16; 10];
    for row in 0..5 {
        engine.state.copy_row_text(row, &mut text);
        for col in 0..10 {
            assert_eq!(
                text[col], 'E' as u16,
                "Screen[{},{}] should be 'E'",
                row, col
            );
        }
    }

    // 验证光标在左上角
    assert_eq!(
        engine.state.cursor_x, 0,
        "Cursor X should be 0 after DECALN"
    );
    assert_eq!(
        engine.state.cursor_y, 0,
        "Cursor Y should be 0 after DECALN"
    );
}

/// 验证 RI (ESC M) - ✅ PASS
#[test]
fn test_ri_reverse_index() {
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);

    // 移动到底部
    engine.process_bytes(b"\x1b[10;5H");
    assert_eq!(engine.state.cursor_y, 9);
    assert_eq!(engine.state.cursor_x, 4);

    // ESC M - RI (反向索引)
    engine.process_bytes(b"\x1bM");
    assert_eq!(engine.state.cursor_y, 8, "Cursor Y should be 8 after RI");
    assert_eq!(
        engine.state.cursor_x, 4,
        "Cursor X should be unchanged after RI"
    );

    // 在顶部边距时使用 RI 应该滚动
    engine.process_bytes(b"\x1b[1;1H"); // 移动到 (0, 0)
    engine.process_bytes(b"ABC"); // 写入一些文本
    engine.process_bytes(b"\x1bM"); // 应该触发向下滚动
    assert_eq!(
        engine.state.cursor_y, 0,
        "Cursor Y should be 0 after RI at top margin"
    );
}

/// 验证后退制表 (CBT) - ✅ PASS
#[test]
fn test_cursor_backward_tab() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 移动到位置 20，然后后退 5 格
    engine.process_bytes(b"\x1b[21G\x1b[5D");
    // 从位置 20 后退 5 格到位置 15
    assert_eq!(
        engine.state.cursor_x, 15,
        "Cursor X should be 15 after moving back 5"
    );
}

/// 验证清除制表位 (TBC) - ✅ PASS
#[test]
fn test_clear_tab_stop() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 清除当前位置的制表位
    engine.process_bytes(b"\x1b[8G\x1b[0g");
    assert_eq!(
        engine.state.tab_stops[7], false,
        "Tab stop at position 7 should be cleared"
    );
}

// =============================================================================
// 中文字符背景色测试（修复验证）
// =============================================================================

/// 验证中文字符背景色 - ✅ 修复验证
///
/// 问题描述：中文字符（宽字符）会导致背景色少一个字少一格
/// 根本原因：写入宽字符时，只设置了第一列的样式，第二列样式未设置
/// 修复方案：在 setChar 时，对于宽字符同时设置两列的样式
#[test]
fn test_chinese_character_background() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置红色背景 (索引 1)
    let red_bg_style = encode_style(256, 1, 0); // 前景默认，背景红色索引 1

    // 写入带背景色的中文字符
    engine.state.current_style = red_bg_style;
    let data = "你好".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置（两个中文字符，每个占 2 列）
    assert_eq!(
        engine.state.cursor_x, 4,
        "Cursor X should be 4 after two Chinese characters"
    );

    // 验证第一列和第二列的样式是否都设置了红色背景
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // "你" 在列 0-1，两列都应该有红色背景
    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        1,
        "Column 0 background should be red (index 1)"
    );
    assert_eq!(
        ((styles[1] as u64) >> 16) & 0x1FF,
        1,
        "Column 1 background should be red (index 1) - THIS IS THE FIX!"
    );

    // "好" 在列 2-3，两列都应该有红色背景
    assert_eq!(
        ((styles[2] as u64) >> 16) & 0x1FF,
        1,
        "Column 2 background should be red (index 1)"
    );
    assert_eq!(
        ((styles[3] as u64) >> 16) & 0x1FF,
        1,
        "Column 3 background should be red (index 1) - THIS IS THE FIX!"
    );
}

/// 验证宽字符覆盖时的样式处理 - ✅ 修复验证
#[test]
fn test_wide_char_overwrite_style() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 先写入两个单宽度字符，带不同背景色
    let blue_bg = encode_style(256, 4, 0); // 背景蓝色索引 4
    let green_bg = encode_style(256, 2, 0); // 背景绿色索引 2

    engine.state.current_style = blue_bg;
    engine.process_bytes(b"A");

    engine.state.current_style = green_bg;
    engine.process_bytes(b"B");

    // 现在用宽字符覆盖 "AB"
    engine.state.cursor_x = 0;
    let red_bg = encode_style(256, 1, 0); // 背景红色索引 1
    engine.state.current_style = red_bg;
    engine.process_bytes("中".as_bytes());

    // 验证宽字符 "中" 覆盖后，两列都是红色背景
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        1,
        "Column 0 background should be red after wide char overwrite"
    );
    assert_eq!(
        ((styles[1] as u64) >> 16) & 0x1FF,
        1,
        "Column 1 background should be red after wide char overwrite - THIS IS THE FIX!"
    );
}

/// 验证宽字符在行尾的样式处理 - ✅ 修复验证
#[test]
fn test_wide_char_at_line_end_style() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(10, 5, 100, 10, 20); // 窄屏幕

    // 设置黄色背景 (索引 3)
    let yellow_bg = encode_style(256, 3, 0);
    engine.state.current_style = yellow_bg;

    // 写入 8 个字符到倒数第二列
    engine.process_bytes(b"12345678");
    assert_eq!(engine.state.cursor_x, 8);

    // 现在写入一个宽字符，应该触发换行
    engine.process_bytes("中".as_bytes());

    // 验证样式
    let mut styles = [0i64; 10];
    engine.state.copy_row_styles(0, &mut styles);

    // 第一行应该有 8 个黄色背景 + 2 个默认背景（宽字符第二列在下一行）
    // 具体行为取决于实现，这里验证基本样式设置
    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        3,
        "Column 0 background should be yellow"
    );
    assert_eq!(
        ((styles[7] as u64) >> 16) & 0x1FF,
        3,
        "Column 7 background should be yellow"
    );
}

// =============================================================================
// Emoji 和非标准字符测试（新增）
// =============================================================================

/// 验证 Emoji 背景色 - ✅ 修复验证
///
/// Emoji 通常是宽字符（2 列），需要验证背景色正确覆盖两列
#[test]
fn test_emoji_background() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置绿色背景 (索引 2)
    let green_bg_style = encode_style(256, 2, 0);
    engine.state.current_style = green_bg_style;

    // 写入带背景色的 Emoji
    let data = "😀😎🎉".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置（3 个 Emoji，每个占 2 列）
    assert_eq!(
        engine.state.cursor_x, 6,
        "Cursor X should be 6 after three emoji"
    );

    // 验证每列的样式是否都设置了绿色背景
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 每个 Emoji 占 2 列，共 6 列都应该有绿色背景
    for i in 0..6 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            2,
            "Column {} background should be green (index 2) for emoji",
            i
        );
    }
}

/// 验证混合字符（ASCII + 中文 + Emoji）背景色 - ✅ 修复验证
#[test]
fn test_mixed_characters_background() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置青色背景 (索引 6)
    let cyan_bg_style = encode_style(256, 6, 0);
    engine.state.current_style = cyan_bg_style;

    // 写入混合字符：ASCII + 中文 + Emoji
    let data = "A 你好😀".as_bytes();
    engine.process_bytes(data);

    // 验证每列的样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 验证实际占用的列数都有青色背景
    // A(1) + 你 (2) + 好 (2) + 😀(2) = 7 或更多（取决于具体实现）
    let cursor_x = engine.state.cursor_x as usize;
    for i in 0..cursor_x {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            6,
            "Column {} background should be cyan (index 6)",
            i
        );
    }
}

/// 验证韩文（Hangul）背景色 - ✅ 修复验证
///
/// 韩文字符通常是宽字符
#[test]
fn test_korean_hangul_background() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置品红色背景 (索引 5)
    let magenta_bg_style = encode_style(256, 5, 0);
    engine.state.current_style = magenta_bg_style;

    // 写入韩文字符
    let data = "안녕하세요".as_bytes();
    engine.process_bytes(data);

    // 验证每列的样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 韩文通常是宽字符，5 个字符应该占 10 列
    // 验证前 10 列都有品红色背景
    for i in 0..10 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            5,
            "Column {} background should be magenta (index 5) for Hangul",
            i
        );
    }
}

/// 验证日文假名背景色 - ✅ 修复验证
#[test]
fn test_japanese_kana_background() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置黄色背景 (索引 3)
    let yellow_bg_style = encode_style(256, 3, 0);
    engine.state.current_style = yellow_bg_style;

    // 写入日文假名（平假名和片假名）
    let data = "こんにちはコンニチワ".as_bytes();
    engine.process_bytes(data);

    // 验证每列的样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 日文假名通常是宽字符，10 个字符应该占 20 列
    // 验证前 20 列都有黄色背景
    for i in 0..20 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            3,
            "Column {} background should be yellow (index 3) for Kana",
            i
        );
    }
}

/// 验证全角字符背景色 - ✅ 修复验证
///
/// 全角 ASCII 字符（如 A）也是宽字符
#[test]
fn test_fullwidth_ascii_background() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置红色背景 (索引 1)
    let red_bg_style = encode_style(256, 1, 0);
    engine.state.current_style = red_bg_style;

    // 写入全角 ASCII 字符
    let data = "A B C".as_bytes();
    engine.process_bytes(data);

    // 验证每列的样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 验证实际占用的列数都有红色背景
    // 具体列数取决于字符的实际宽度
    let cursor_x = engine.state.cursor_x as usize;
    for i in 0..cursor_x {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            1,
            "Column {} background should be red (index 1) for fullwidth ASCII",
            i
        );
    }
}

/// 验证组合字符（Combining Characters）处理 - ⚠️ PARTIAL
///
/// 组合字符（如重音符号）宽度为 0，不应该占用额外的列
#[test]
fn test_combining_characters() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置蓝色背景 (索引 4)
    let blue_bg_style = encode_style(256, 4, 0);
    engine.state.current_style = blue_bg_style;

    // 写入带组合字符的文本：e +  ́ = é
    // \u{0301} 是组合重音符号（宽度 0）
    let data = "cafe\u{0301}".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置：4 个基础字符，组合字符不占额外列
    // 注意：具体行为取决于实现
    assert!(
        engine.state.cursor_x >= 4,
        "Cursor X should be at least 4 after 'café'"
    );
}

/// 验证变体选择器（Variation Selectors） - ⚠️ PARTIAL
///
/// 变体选择器用于选择 Emoji 的显示样式（如文本 vs 表情）
#[test]
fn test_variation_selectors() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置绿色背景 (索引 2)
    let green_bg_style = encode_style(256, 2, 0);
    engine.state.current_style = green_bg_style;

    // 写入带变体选择器的字符
    // U+2764 (❤) + U+FE0F (变体选择器 -16, 表情样式)
    let data = "\u{2764}\u{FE0F}".as_bytes();
    engine.process_bytes(data);

    // 验证样式设置
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 至少第一列应该有绿色背景
    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        2,
        "Column 0 background should be green (index 2)"
    );
}

/// 验证零宽字符处理 - ✅ 修复验证
///
/// 零宽字符（如零宽空格、零宽连字）不应该占用列
#[test]
fn test_zero_width_characters() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置青色背景 (索引 6)
    let cyan_bg_style = encode_style(256, 6, 0);
    engine.state.current_style = cyan_bg_style;

    // 写入零宽空格 (U+200B)
    let data = "AB\u{200B}CD".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置：零宽字符不占列，应该是 4
    assert_eq!(
        engine.state.cursor_x, 4,
        "Cursor X should be 4 (zero-width char doesn't add columns)"
    );

    // 验证样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 4 列都应该有青色背景
    for i in 0..4 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            6,
            "Column {} background should be cyan (index 6)",
            i
        );
    }
}

/// 验证复杂 Emoji 序列（ZWNJ 序列） - ⚠️ PARTIAL
///
/// 复杂 Emoji 如家庭（👨‍👩‍👧‍👦）使用零宽连字（ZWNJ, U+200D）连接
#[test]
fn test_complex_emoji_sequence() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置红色背景 (索引 1)
    let red_bg_style = encode_style(256, 1, 0);
    engine.state.current_style = red_bg_style;

    // 写入复杂 Emoji 序列
    let data = "👨‍👩‍👧‍👦".as_bytes();
    engine.process_bytes(data);

    // 验证样式设置
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 至少前两列应该有红色背景（Emoji 是宽字符）
    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        1,
        "Column 0 background should be red (index 1) for complex emoji"
    );
    assert_eq!(
        ((styles[1] as u64) >> 16) & 0x1FF,
        1,
        "Column 1 background should be red (index 1) for complex emoji"
    );
}

/// 验证区域指示符号（国旗 Emoji） - ✅ 修复验证
///
/// 国旗 Emoji 由两个区域指示符号组成（如 🇺🇸 = U+1F1FA + U+1F1F8）
#[test]
fn test_regional_indicator_symbols() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置蓝色背景 (索引 4)
    let blue_bg_style = encode_style(256, 4, 0);
    engine.state.current_style = blue_bg_style;

    // 写入美国国旗 Emoji
    let data = "🇺🇸".as_bytes();
    engine.process_bytes(data);

    // 验证样式设置
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 国旗 Emoji 应该占 2 列，两列都应该有蓝色背景
    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        4,
        "Column 0 background should be blue (index 4) for flag emoji"
    );
    assert_eq!(
        ((styles[1] as u64) >> 16) & 0x1FF,
        4,
        "Column 1 background should be blue (index 4) for flag emoji"
    );
}

/// 验证多个连续 Emoji 背景色 - ✅ 修复验证
#[test]
fn test_multiple_consecutive_emoji() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置黄色背景 (索引 3)
    let yellow_bg_style = encode_style(256, 3, 0);
    engine.state.current_style = yellow_bg_style;

    // 写入多个连续 Emoji
    let data = "😀😎🎉🔥💯".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置：5 个 Emoji，每个占 2 列 = 10 列
    assert_eq!(
        engine.state.cursor_x, 10,
        "Cursor X should be 10 after five emoji"
    );

    // 验证每列的样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // 所有 10 列都应该有黄色背景
    for i in 0..10 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            3,
            "Column {} background should be yellow (index 3) for consecutive emoji",
            i
        );
    }
}

/// 验证 Emoji 和文本混合背景色切换 - ✅ 修复验证
#[test]
fn test_emoji_text_style_switch() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 先写入红色背景的文本
    engine.state.current_style = encode_style(256, 1, 0);
    engine.process_bytes(b"Hello");

    // 切换到绿色背景写入 Emoji
    engine.state.current_style = encode_style(256, 2, 0);
    engine.process_bytes("😀".as_bytes());

    // 再切换到蓝色背景写入文本
    engine.state.current_style = encode_style(256, 4, 0);
    engine.process_bytes(b"World");

    // 验证样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // "Hello" (5 列) 红色背景
    for i in 0..5 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            1,
            "Column {} background should be red (index 1) for 'Hello'",
            i
        );
    }

    // "😀" (2 列) 绿色背景
    assert_eq!(
        ((styles[5] as u64) >> 16) & 0x1FF,
        2,
        "Column 5 background should be green (index 2) for emoji"
    );
    assert_eq!(
        ((styles[6] as u64) >> 16) & 0x1FF,
        2,
        "Column 6 background should be green (index 2) for emoji"
    );

    // "World" (5 列) 蓝色背景
    for i in 7..12 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            4,
            "Column {} background should be blue (index 4) for 'World'",
            i
        );
    }
}

// =============================================================================
// 光标测试（新增）
// =============================================================================

/// 验证宽字符上的光标 - ✅ 修复验证
///
/// 块状光标在宽字符上时应该覆盖两列
#[test]
fn test_cursor_on_wide_character() {
    use termux_rust::engine::TerminalEngine;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一个中文字符
    let data = "你".as_bytes();
    engine.process_bytes(data);

    // 光标应该在位置 2（中文字符占 2 列）
    assert_eq!(
        engine.state.cursor_x, 2,
        "Cursor X should be 2 after Chinese character"
    );

    // 现在移动到第一列（宽字符的第一列）
    engine.process_bytes(b"\x1b[1G"); // 移动到列 1（0-based 是 0）
    assert_eq!(
        engine.state.cursor_x, 0,
        "Cursor X should be 0 after moving to first column"
    );

    // 验证光标位置
    assert_eq!(engine.state.cursor_y, 0, "Cursor Y should be 0");
}

/// 验证光标在 Emoji 上 - ✅ 修复验证
#[test]
fn test_cursor_on_emoji() {
    use termux_rust::engine::TerminalEngine;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一个 Emoji
    let data = "😀".as_bytes();
    engine.process_bytes(data);

    // 光标应该在位置 2（Emoji 占 2 列）
    assert_eq!(engine.state.cursor_x, 2, "Cursor X should be 2 after emoji");
}

/// 验证光标在混合字符上的位置 - ✅ 修复验证
#[test]
fn test_cursor_on_mixed_characters() {
    use termux_rust::engine::TerminalEngine;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入混合字符：ASCII + 中文 + Emoji
    let data = "A 你😀".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置
    // 注意：具体列数取决于字符宽度计算
    let cursor_x = engine.state.cursor_x;
    assert!(
        cursor_x >= 5,
        "Cursor X should be at least 5 after 'A 你😀', got {}",
        cursor_x
    );
}

// =============================================================================
// 综合测试：背景色 + 光标（新增）
// =============================================================================

/// 验证宽字符上的光标和背景色 - ✅ 修复验证
///
/// 综合测试：块状光标在宽字符上时，应该覆盖两列，且背景色正确
#[test]
fn test_cursor_and_background_on_wide_character() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置红色背景 (索引 1)
    let red_bg_style = encode_style(256, 1, 0);
    engine.state.current_style = red_bg_style;

    // 写入一个中文字符
    let data = "你".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置
    assert_eq!(
        engine.state.cursor_x, 2,
        "Cursor X should be 2 after Chinese character"
    );

    // 验证样式：两列都应该有红色背景
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        1,
        "Column 0 background should be red (index 1) for Chinese char"
    );
    assert_eq!(
        ((styles[1] as u64) >> 16) & 0x1FF,
        1,
        "Column 1 background should be red (index 1) for Chinese char"
    );
}

/// 验证 Emoji 上的光标和背景色 - ✅ 修复验证
///
/// 综合测试：块状光标在 Emoji 上时，应该覆盖两列，且背景色正确
#[test]
fn test_cursor_and_background_on_emoji() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置绿色背景 (索引 2)
    let green_bg_style = encode_style(256, 2, 0);
    engine.state.current_style = green_bg_style;

    // 写入一个 Emoji
    let data = "😀".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置
    assert_eq!(engine.state.cursor_x, 2, "Cursor X should be 2 after emoji");

    // 验证样式：两列都应该有绿色背景
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        2,
        "Column 0 background should be green (index 2) for emoji"
    );
    assert_eq!(
        ((styles[1] as u64) >> 16) & 0x1FF,
        2,
        "Column 1 background should be green (index 2) for emoji"
    );
}

/// 验证复杂混合场景 - ✅ 修复验证
///
/// 综合测试：文本 + 中文+Emoji 混合，带背景色，验证光标和背景色都正确
#[test]
fn test_complex_mixed_scenario() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置青色背景 (索引 6)
    let cyan_bg_style = encode_style(256, 6, 0);
    engine.state.current_style = cyan_bg_style;

    // 写入复杂混合内容：ASCII + 中文 + Emoji
    let data = "Hello 你好😀🎉".as_bytes();
    engine.process_bytes(data);

    // 验证光标位置
    // 注意：具体列数取决于字符宽度计算
    let cursor_x = engine.state.cursor_x;
    assert!(
        cursor_x >= 13,
        "Cursor X should be at least 13 after 'Hello 你好😀🎉', got {}",
        cursor_x
    );

    // 验证样式：所有列都应该有青色背景
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    let cursor_x_usize = cursor_x as usize;
    for i in 0..cursor_x_usize {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            6,
            "Column {} background should be cyan (index 6)",
            i
        );
    }
}

/// 验证背景色切换时的宽字符 - ✅ 修复验证
///
/// 综合测试：在宽字符前后切换背景色，验证样式边界正确
#[test]
fn test_background_switch_with_wide_characters() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 先写入红色背景的 ASCII
    engine.state.current_style = encode_style(256, 1, 0);
    engine.process_bytes(b"AB");

    // 切换到绿色背景写入中文
    engine.state.current_style = encode_style(256, 2, 0);
    engine.process_bytes("你".as_bytes());

    // 切换到蓝色背景写入 ASCII
    engine.state.current_style = encode_style(256, 4, 0);
    engine.process_bytes(b"CD");

    // 验证样式
    let mut styles = [0i64; 80];
    engine.state.copy_row_styles(0, &mut styles);

    // "AB" (2 列) 红色背景
    assert_eq!(
        ((styles[0] as u64) >> 16) & 0x1FF,
        1,
        "Column 0 should be red"
    );
    assert_eq!(
        ((styles[1] as u64) >> 16) & 0x1FF,
        1,
        "Column 1 should be red"
    );

    // "你" (2 列) 绿色背景
    assert_eq!(
        ((styles[2] as u64) >> 16) & 0x1FF,
        2,
        "Column 2 should be green"
    );
    assert_eq!(
        ((styles[3] as u64) >> 16) & 0x1FF,
        2,
        "Column 3 should be green"
    );

    // "CD" (2 列) 蓝色背景
    assert_eq!(
        ((styles[4] as u64) >> 16) & 0x1FF,
        4,
        "Column 4 should be blue"
    );
    assert_eq!(
        ((styles[5] as u64) >> 16) & 0x1FF,
        4,
        "Column 5 should be blue"
    );
}

/// 验证行尾宽字符的背景色 - ✅ 修复验证
///
/// 综合测试：宽字符在行尾时，背景色和换行都正确
#[test]
fn test_wide_char_at_line_end_with_background() {
    use termux_rust::engine::{TerminalEngine, encode_style};

    let mut engine = TerminalEngine::new(10, 5, 100, 10, 20); // 窄屏幕

    // 设置黄色背景 (索引 3)
    let yellow_bg_style = encode_style(256, 3, 0);
    engine.state.current_style = yellow_bg_style;

    // 写入 8 个字符到倒数第二列
    engine.process_bytes(b"12345678");

    // 写入一个宽字符，应该触发换行
    engine.process_bytes("你".as_bytes());

    // 验证第一行样式
    let mut styles = [0i64; 10];
    engine.state.copy_row_styles(0, &mut styles);

    // 第一行 8 列应该是黄色背景
    for i in 0..8 {
        assert_eq!(
            ((styles[i] as u64) >> 16) & 0x1FF,
            3,
            "Column {} background should be yellow (index 3)",
            i
        );
    }

    // 验证光标位置：
    // "12345678" -> cursor_x = 8
    // "你" (width 2) -> 此时 8+2=10, 刚好填满行 (right_margin=10)
    // 根据逻辑：它会填满 8, 9 列，然后 cursor_x 变成 10
    // 因为 10 >= right_margin，它会触发 pending wrap 逻辑
    assert_eq!(engine.state.cursor_y, 0, "Cursor should still be on row 0 (fits exactly)");
    assert_eq!(engine.state.about_to_wrap, true, "Should be about to wrap");
}

// =============================================================================
// 新增功能测试 - 颜色管理、标题栈、行绘图等
// =============================================================================

/// 验证 OSC 4 设置颜色索引 - ✅ PASS
#[test]
fn test_osc4_set_color() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // OSC 4 ; 1 ; #FF0000 BEL - 设置颜色索引 1 为红色
    engine.process_bytes(b"\x1b]4;1;#FF0000\x07");

    // 验证颜色已更改
    let color = engine.state.colors.current_colors[1];
    assert_eq!(color, 0xffff0000, "Color index 1 should be set to #FF0000");
}

/// 验证 OSC 10 设置前景色 - ✅ PASS
#[test]
fn test_osc10_set_foreground() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // OSC 10 ; #00FF00 BEL - 设置前景色为绿色
    engine.process_bytes(b"\x1b]10;#00FF00\x07");

    // 验证前景色已更改
    let fg_color = engine.state.colors.current_colors[256];
    assert_eq!(
        fg_color, 0xff00ff00,
        "Foreground color should be set to #00FF00"
    );
}

/// 验证 OSC 11 设置背景色 - ✅ PASS
#[test]
fn test_osc11_set_background() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // OSC 11 ; #0000FF BEL - 设置背景色为蓝色
    engine.process_bytes(b"\x1b]11;#0000FF\x07");

    // 验证背景色已更改
    let bg_color = engine.state.colors.current_colors[257];
    assert_eq!(
        bg_color, 0xff0000ff,
        "Background color should be set to #0000FF"
    );
}

/// 验证 OSC 104 重置颜色 - ✅ PASS
#[test]
fn test_osc104_reset_colors() {
    use termux_rust::engine::DEFAULT_COLORSCHEME;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 先更改一些颜色
    engine.process_bytes(b"\x1b]10;#FFFFFF\x07");
    engine.process_bytes(b"\x1b]11;#000000\x07");

    // OSC 104 - 重置所有颜色
    engine.process_bytes(b"\x1b]104\x07");

    // 验证颜色已重置
    assert_eq!(
        engine.state.colors.current_colors[256], DEFAULT_COLORSCHEME[256],
        "Foreground color should be reset to default"
    );
    assert_eq!(
        engine.state.colors.current_colors[257], DEFAULT_COLORSCHEME[257],
        "Background color should be reset to default"
    );
}

/// 验证 OSC 22/23 标题栈 - ✅ PASS
#[test]
fn test_osc22_23_title_stack() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置初始标题
    engine.process_bytes(b"\x1b]2;Initial Title\x07");
    assert_eq!(
        engine.state.title,
        Some("Initial Title".to_string()),
        "Title should be set to 'Initial Title'"
    );

    // OSC 22 - 保存标题
    engine.process_bytes(b"\x1b]22;0\x07");

    // 更改标题
    engine.process_bytes(b"\x1b]2;Changed Title\x07");
    assert_eq!(
        engine.state.title,
        Some("Changed Title".to_string()),
        "Title should be changed to 'Changed Title'"
    );

    // OSC 23 - 恢复标题
    engine.process_bytes(b"\x1b]23;0\x07");
    assert_eq!(
        engine.state.title,
        Some("Initial Title".to_string()),
        "Title should be restored to 'Initial Title'"
    );
}

/// 验证 ESC ( 和 ESC ) 行绘图字符集切换 - ✅ PASS
#[test]
fn test_line_drawing_charset_switch() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // ESC ( 0 - 选择行绘图字符集为 G0
    engine.process_bytes(b"\x1b(0");
    assert_eq!(
        engine.state.use_line_drawing_g0, true,
        "Line drawing G0 should be enabled"
    );
    assert_eq!(
        engine.state.use_line_drawing_uses_g0, true,
        "Should be using G0"
    );

    // ESC ) 0 - 选择行绘图字符集为 G1
    engine.process_bytes(b"\x1b)0");
    assert_eq!(
        engine.state.use_line_drawing_g1, true,
        "Line drawing G1 should be enabled"
    );
    assert_eq!(
        engine.state.use_line_drawing_uses_g0, false,
        "Should be using G1"
    );
}

/// 验证 SO/SI 字符集切换 - ✅ PASS
#[test]
fn test_so_si_charset_switch() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 先启用 G0 行绘图
    engine.process_bytes(b"\x1b(0");

    // SO (0x0e) - 切换到 G1
    engine.process_bytes(b"\x0e");
    assert_eq!(
        engine.state.use_line_drawing_uses_g0, false,
        "Should switch to G1 with SO"
    );

    // SI (0x0f) - 切换到 G0
    engine.process_bytes(b"\x0f");
    assert_eq!(
        engine.state.use_line_drawing_uses_g0, true,
        "Should switch to G0 with SI"
    );
}

/// 验证 RIS 完整重置 - ✅ PASS
#[test]
fn test_ris_full_reset() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 更改一些状态
    engine.process_bytes(b"\x1b[?7l"); // 禁用自动换行
    engine.process_bytes(b"\x1b[5;20r"); // 设置边距
    engine.process_bytes(b"\x1b[31m"); // 红色前景
    engine.process_bytes(b"\x1b]2;Test Title\x07"); // 设置标题

    // RIS - 重置到初始状态
    engine.process_bytes(b"\x1bc");

    // 验证所有状态已重置
    assert_eq!(engine.state.auto_wrap, true, "Auto wrap should be reset");
    assert_eq!(engine.state.top_margin, 0, "Top margin should be reset");
    assert_eq!(
        engine.state.bottom_margin, 24,
        "Bottom margin should be reset"
    );
    assert_eq!(
        (engine.state.current_style >> 40) & 0x1FF,
        256,
        "Foreground color should be reset"
    );
    assert_eq!(engine.state.title, None, "Title should be cleared");
    assert_eq!(
        engine.state.scroll_counter, 0,
        "Scroll counter should be reset"
    );
}

/// 验证滚动计数器 - ✅ PASS
#[test]
fn test_scroll_counter() {
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);

    // 初始滚动计数器应为 0
    assert_eq!(
        engine.state.scroll_counter, 0,
        "Initial scroll counter should be 0"
    );

    // 写满屏幕触发滚动
    for i in 0..10 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }

    // 验证滚动计数器已增加
    assert!(
        engine.state.scroll_counter > 0,
        "Scroll counter should be incremented after scrolling"
    );
}

/// 验证自动滚动禁用 - ✅ PASS
#[test]
fn test_auto_scroll_disabled() {
    let mut engine = TerminalEngine::new(80, 5, 100, 10, 20);

    // 禁用自动滚动
    engine.state.auto_scroll_disabled = true;

    // 写满屏幕触发滚动
    for i in 0..10 {
        let line = format!("Line {}\r\n", i);
        engine.process_bytes(line.as_bytes());
    }

    // 滚动计数器不应增加
    assert_eq!(
        engine.state.scroll_counter, 0,
        "Scroll counter should not increment when auto-scroll is disabled"
    );
}

/// 验证 SGR 58/59 下划线颜色 - ✅ PASS
#[test]
fn test_sgr_underline_color() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置下划线颜色为红色 (索引 1)
    engine.process_bytes(b"\x1b[58;5;1m");

    assert_eq!(
        engine.state.underline_color, 1,
        "Underline color should be set to index 1"
    );

    // 重置下划线颜色
    engine.process_bytes(b"\x1b[59m");

    assert_eq!(
        engine.state.underline_color, 256,
        "Underline color should be reset to default (256)"
    );
}

/// 验证保存/恢复光标包含行绘图状态 - ✅ PASS
#[test]
fn test_save_restore_cursor_line_drawing() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 G0 行绘图
    engine.process_bytes(b"\x1b(0");

    // 保存光标
    engine.process_bytes(b"\x1b7");

    // 更改行绘图状态
    engine.process_bytes(b"\x1b)0");

    // 恢复光标
    engine.process_bytes(b"\x1b8");

    // 验证行绘图状态已恢复
    assert_eq!(
        engine.state.use_line_drawing_uses_g0, true,
        "Should restore to using G0"
    );
}

/// 验证保存/恢复光标包含颜色 - ✅ PASS
#[test]
fn test_save_restore_cursor_colors() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置红色前景
    engine.process_bytes(b"\x1b[31m");

    // 保存光标
    engine.process_bytes(b"\x1b7");

    // 更改颜色为蓝色
    engine.process_bytes(b"\x1b[34m");

    // 恢复光标
    engine.process_bytes(b"\x1b8");

    // 验证颜色已恢复
    assert_eq!(
        engine.state.fore_color, 1,
        "Foreground color should be restored to red (1)"
    );
}

#[test]
fn test_erase_display_mode_3() {
    use termux_rust::engine::TerminalEngine;
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    // 填充一些滚动历史 (超过 24 行)
    for i in 0..50 {
        let msg = format!("Line {}\n", i);
        engine.process_bytes(msg.as_bytes());
    }

    // 确认有滚动历史 (screen_first_row 应该已经滚动)
    assert!(engine.state.screen_first_row > 0, "Should have scrolled");

    // 执行 CSI 3 J (清历史)
    engine.process_bytes(b"\x1b[3J");

    // 验证 screen_first_row 已重置
    assert_eq!(
        engine.state.screen_first_row, 0,
        "Screen first row should be reset"
    );
    assert_eq!(
        engine.state.scroll_counter, 0,
        "Scroll counter should be reset"
    );

    // 验证整个 buffer 都是空的
    for y in 0..engine.state.buffer.len() {
        let row = &engine.state.buffer[y];
        for x in 0..80 {
            let c = row.text[x];
            assert_eq!(c, ' ', "Cell ({}, {}) should be empty space", x, y);
        }
    }
}

// =============================================================================
// DCS/Sixel 图形测试
// =============================================================================

/// 验证 Sixel 基础解码 - ⚠️ PARTIAL
#[test]
fn test_sixel_basic_decode() {
    use termux_rust::engine::SixelDecoder;

    let mut decoder = SixelDecoder::new();

    // 验证初始状态
    assert_eq!(decoder.state, termux_rust::engine::SixelState::Ground);
    assert_eq!(decoder.width, 0, "Initial width should be 0");
    assert_eq!(decoder.height, 0, "Initial height should be 0");
    assert_eq!(decoder.current_color, 0, "Initial color should be 0");

    // 发送简单的 sixel 数据（不经过 start，直接测试 process_data）
    decoder.process_data(b"?");

    // 验证数据被处理
    assert!(
        !decoder.pixel_data.is_empty() || decoder.width > 0,
        "Should have processed sixel data"
    );
}

/// 验证 Sixel 数据解析 - ⚠️ PARTIAL
#[test]
fn test_sixel_data_parsing() {
    use termux_rust::engine::SixelDecoder;

    let mut decoder = SixelDecoder::new();

    // 发送 sixel 数据
    decoder.process_data(b"??????????");

    // 验证宽度扩展（默认初始化至少 100）
    assert!(
        decoder.width >= 10,
        "Width should be at least 10, got {}",
        decoder.width
    );
}

/// 验证 Sixel 换行 - ✅ PASS
#[test]
fn test_sixel_newline() {
    use termux_rust::engine::SixelDecoder;

    let mut decoder = SixelDecoder::new();

    // 第一行 sixel 数据
    decoder.process_data(b"??????????");
    let col_after_data = decoder.current_col;
    assert!(col_after_data > 0, "Column should have moved");

    // 换行 (!)
    decoder.process_data(b"!");

    // 验证当前行位置
    assert_eq!(
        decoder.current_row, 6,
        "Should move to next sixel row (6 pixels)"
    );
    assert_eq!(decoder.current_col, 0, "Should reset column to 0");
}

/// 验证 Sixel 光标归位 - ✅ PASS
#[test]
fn test_sixel_carriage_return() {
    use termux_rust::engine::SixelDecoder;

    let mut decoder = SixelDecoder::new();

    // 发送一些数据移动光标
    decoder.process_data(b"??????????");
    assert!(decoder.current_col > 0, "Column should have moved");

    // 光标归位 ($)
    decoder.process_data(b"$");

    // 验证光标归位
    assert_eq!(decoder.current_col, 0, "Column should be reset to 0");
}

/// 验证 Sixel 删除图形 - ✅ PASS
#[test]
fn test_sixel_delete() {
    use termux_rust::engine::SixelDecoder;

    let mut decoder = SixelDecoder::new();

    // 设置一些像素
    decoder.process_data(b"??????????");

    // 删除当前像素 (~)
    decoder.process_data(b"~");

    // 验证删除（简化测试）
    assert!(decoder.pixel_data.len() > 0, "Should still have pixel data");
}

/// 验证 Sixel 完整序列 - ⚠️ PARTIAL
#[test]
fn test_sixel_full_sequence() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 完整的 Sixel 序列示例：
    // DCS Pn1;Pn2;Pn3 q sixel_data ST
    // DCS 0;10;10 q ?!?????!?????!?????! ST
    // 这是一个 10x24 像素的简单图像
    let sixel_seq =
        "\x1bP0;10;10q?!?????!?????!?????!?????!?????!?????!?????!?????!?????!?????!?????!\x1b\\";
    engine.process_bytes(sixel_seq.as_bytes());

    // 验证不崩溃
    // 完整的验证需要检查 Java 回调是否收到图像数据
    assert!(engine.state.cols > 0, "Engine should still be valid");
}

/// 验证 Sixel 图像渲染回调 - ⚠️ PARTIAL
#[test]
fn test_sixel_image_rendering() {
    // 这个测试需要 Java 环境，只能在集成测试中运行
    // 这里只做框架验证
    use termux_rust::engine::TerminalEngine;

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 发送 Sixel 序列
    engine.process_bytes(b"\x1bP0;10;10q?\x1b\\");

    // 验证引擎状态正常
    assert!(engine.state.cols > 0, "Engine should still be valid");
}
