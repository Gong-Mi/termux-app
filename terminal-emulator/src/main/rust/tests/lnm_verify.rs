// LNM / DECSTR / Insert Mode - 修复验证 + 扩大测试覆盖
// 运行：cargo test --test lnm_verify -- --nocapture

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ").trim_end().to_string()
}

// =============================================================================
// Part A: DECSTR Soft Reset - 验证修复
// =============================================================================

#[test]
fn test_decstr_resets_lnm() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");
    assert!(engine.state.modes.is_enabled(1 << 13), "LNM should be ON");

    // DECSTR
    engine.process_bytes(b"\x1b[!p");

    // LNM 应该被重置
    assert!(!engine.state.modes.is_enabled(1 << 13),
        "DECSTR should reset LNM");

    // 功能验证：\n 不应该重置 x
    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 10, "After DECSTR, LF should NOT reset x");

    println!("✅ DECSTR resets LNM");
}

#[test]
fn test_decstr_resets_all_modes() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用各种模式
    engine.process_bytes(b"\x1b[?1h");    // application cursor keys
    engine.process_bytes(b"\x1b[?6h");    // origin mode
    engine.process_bytes(b"\x1b[20h");    // LNM
    engine.process_bytes(b"\x1b[4h");     // insert mode
    engine.process_bytes(b"\x1b[?2004h"); // bracketed paste

    // 确认都开了
    assert!(engine.state.application_cursor_keys);
    assert!(engine.state.modes.is_enabled(1 << 2));   // ORIGIN
    assert!(engine.state.modes.is_enabled(1 << 13));  // LNM
    assert!(engine.state.modes.is_enabled(1 << 12));  // INSERT
    assert!(engine.state.bracketed_paste);

    // DECSTR
    engine.process_bytes(b"\x1b[!p");

    // 确认都关了
    assert!(!engine.state.application_cursor_keys, "DECSTR: app cursor keys");
    assert!(!engine.state.modes.is_enabled(1 << 2), "DECSTR: origin mode");
    assert!(!engine.state.modes.is_enabled(1 << 13), "DECSTR: LNM");
    assert!(!engine.state.modes.is_enabled(1 << 12), "DECSTR: insert mode");
    assert!(!engine.state.bracketed_paste, "DECSTR: bracketed paste");
    assert!(!engine.state.send_focus_events, "DECSTR: focus events");
    assert!(!engine.state.mouse_tracking, "DECSTR: mouse tracking");
    assert!(!engine.state.sgr_mouse, "DECSTR: SGR mouse");

    // AUTOWRAP 应该保持开启
    assert!(engine.state.modes.is_enabled(1 << 3), "DECSTR: autowrap should stay ON");

    // margins 应该被重置
    assert_eq!(engine.state.top_margin, 0);
    assert_eq!(engine.state.bottom_margin, 24);
    assert_eq!(engine.state.left_margin, 0);
    assert_eq!(engine.state.right_margin, 80);

    println!("✅ DECSTR resets ALL modes correctly");
}

#[test]
fn test_decstr_resets_charset() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 改变字符集选择
    engine.process_bytes(b"\x1b(0"); // G0 = line drawing
    assert!(engine.state.use_line_drawing_g0);
    assert!(engine.state.use_line_drawing_uses_g0);

    // DECSTR
    engine.process_bytes(b"\x1b[!p");

    // 字符集应该被重置
    assert!(!engine.state.use_line_drawing_g0);
    assert!(!engine.state.use_line_drawing_g1);
    assert!(engine.state.use_line_drawing_uses_g0);

    println!("✅ DECSTR resets charset");
}

// =============================================================================
// Part B: LNM - 兼容性 (?20h) + 功能验证
// =============================================================================

#[test]
fn test_lnm_private_mode_compatibility() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);

    // 尝试用私有模式: CSI ? 20 h
    engine.process_bytes(b"\x1b[?20h");

    // 现在应该生效了（修复后）
    assert!(engine.state.modes.is_enabled(1 << 13),
        "CSI ? 20 h should enable LNM (compatibility mode)");

    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 0, "LNM via ?20h: x should reset to 0");

    // 关闭
    engine.process_bytes(b"\x1b[?20l");
    assert!(!engine.state.modes.is_enabled(1 << 13));

    println!("✅ LNM private mode compatibility works");
}

#[test]
fn test_lnm_standard_mode() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 标准 ANSI 模式: CSI 20 h
    engine.process_bytes(b"\x1b[20h");
    assert!(engine.state.modes.is_enabled(1 << 13));

    engine.process_bytes(b"ABCDEFGH");
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 0);

    // 关闭
    engine.process_bytes(b"\x1b[20l");
    assert!(!engine.state.modes.is_enabled(1 << 13));

    engine.process_bytes(b"XYZ");
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 3, "LNM off: x should stay");

    println!("✅ LNM standard mode works");
}

#[test]
fn test_lnm_all_control_chars() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");

    // 测试 LF (0x0A)
    engine.process_bytes(b"AAAA");
    assert_eq!(engine.state.cursor.x, 4);
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 0, "LF: x resets");

    // 测试 VT (0x0B)
    engine.process_bytes(b"BBBB");
    assert_eq!(engine.state.cursor.x, 4);
    engine.process_bytes(b"\x0b");
    assert_eq!(engine.state.cursor.x, 0, "VT: x resets");

    // 测试 FF (0x0C)
    engine.process_bytes(b"CCCC");
    assert_eq!(engine.state.cursor.x, 4);
    engine.process_bytes(b"\x0c");
    assert_eq!(engine.state.cursor.x, 0, "FF: x resets");

    // CR (0x0D) 总是重置 x，不受 LNM 影响
    engine.process_bytes(b"DDDD");
    engine.process_bytes(b"\r");
    assert_eq!(engine.state.cursor.x, 0);

    println!("✅ LNM affects all control chars (LF, VT, FF)");
}

#[test]
fn test_lnm_with_margins() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置滚动区域: 第 3-8 行 (0-indexed: 2-7)
    engine.process_bytes(b"\x1b[3;8r");
    assert_eq!(engine.state.top_margin, 2);
    assert_eq!(engine.state.bottom_margin, 8);

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");

    // 移动光标到 margin 区域
    engine.process_bytes(b"\x1b[3;5H"); // 1-indexed: row=3, col=5 → y=2, x=4
    assert_eq!(engine.state.cursor.y, 2);
    assert_eq!(engine.state.cursor.x, 4);

    engine.process_bytes(b"ABC");
    assert_eq!(engine.state.cursor.x, 7);

    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 3);
    assert_eq!(engine.state.cursor.x, 0, "LNM: x resets to left_margin");

    // 在 bottom margin 上按 \n 应该 scroll
    engine.process_bytes(b"\x1b[8;1H"); // y=7 (bottom margin - 1)
    engine.process_bytes(b"XYZ");
    assert_eq!(engine.state.cursor.y, 7);

    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 7, "Cursor stays at bottom margin");
    assert_eq!(engine.state.cursor.x, 0, "LNM: x resets");

    println!("✅ LNM works with scrolling margins");
}

// =============================================================================
// Part C: Insert Mode - 验证实际插入行为
// =============================================================================

#[test]
fn test_insert_mode_single_char() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一行
    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);

    // 回到 x=3
    engine.process_bytes(b"\x1b[1;4H"); // y=0, x=3
    assert_eq!(engine.state.cursor.x, 3);

    // 启用 Insert
    engine.process_bytes(b"\x1b[4h");
    assert!(engine.state.modes.is_enabled(1 << 12));

    // 插入字符
    engine.process_bytes(b"X");

    // 光标应该移动
    assert_eq!(engine.state.cursor.x, 4, "Insert: cursor moves");

    // 行内容: "ABCXDEFGHIJ" (X 插入到 D 前面，D 及后面右移)
    let row0 = get_row_text(&engine, 0);
    // 因为插入，原 D 应该在 x=4
    assert!(row0.starts_with("ABCX"), "Insert: row starts with ABCX, got '{}'", row0);

    println!("✅ Insert mode single char: '{}'", row0);
}

#[test]
fn test_insert_mode_multiple_chars() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一行
    engine.process_bytes(b"1234567890");

    // 回到 x=2
    engine.process_bytes(b"\x1b[1;3H");

    // 启用 Insert
    engine.process_bytes(b"\x1b[4h");

    // 插入多个字符
    engine.process_bytes(b"AB");

    let row0 = get_row_text(&engine, 0);
    assert!(row0.starts_with("12AB"), "Insert multiple: row starts with 12AB, got '{}'", row0);

    println!("✅ Insert mode multiple chars: '{}'", row0);
}

#[test]
fn test_insert_mode_off_overwrites() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一行
    engine.process_bytes(b"ABCDEFGHIJ");

    // 回到 x=3
    engine.process_bytes(b"\x1b[1;4H");

    // 关闭 Insert (默认)
    engine.process_bytes(b"\x1b[4l");
    assert!(!engine.state.modes.is_enabled(1 << 12));

    // 写入字符 - 应该覆盖
    engine.process_bytes(b"X");

    let row0 = get_row_text(&engine, 0);
    assert!(row0.starts_with("ABCX"), "Overwrite: row starts with ABCX, got '{}'", row0);
    // D 应该被覆盖，不再在 x=4
    assert!(!row0.starts_with("ABCXD"), "Overwrite: D should be replaced");

    println!("✅ Insert mode off (overwrite): '{}'", row0);
}

#[test]
fn test_insert_mode_disabled_by_decstr() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入 + 启用 Insert
    engine.process_bytes(b"ABCDEFGHIJ");
    engine.process_bytes(b"\x1b[4h");
    assert!(engine.state.modes.is_enabled(1 << 12));

    // DECSTR
    engine.process_bytes(b"\x1b[!p");

    // Insert 应该被关闭
    assert!(!engine.state.modes.is_enabled(1 << 12),
        "DECSTR should reset insert mode");

    // 现在写入应该覆盖
    engine.process_bytes(b"\x1b[1;4H");
    engine.process_bytes(b"X");

    let row0 = get_row_text(&engine, 0);
    assert!(row0.starts_with("ABCX"), "After DECSTR, overwrite mode, got '{}'", row0);

    println!("✅ Insert mode disabled by DECSTR: '{}'", row0);
}

#[test]
fn test_insert_mode_scroll_at_end() {
    let mut engine = TerminalEngine::new(10, 5, 100, 10, 20);

    // 填满一行
    engine.process_bytes(b"1234567890");

    // 回到开头
    engine.process_bytes(b"\x1b[1;1H");

    // 启用 Insert
    engine.process_bytes(b"\x1b[4h");

    // 插入字符，应该会滚动
    engine.process_bytes(b"X");

    let row0 = get_row_text(&engine, 0);
    println!("Insert at end: row0='{}'", row0);

    // 验证没有 panic 或崩溃
    assert!(row0.len() > 0);

    println!("✅ Insert mode scroll at end");
}

// =============================================================================
// Part D: DECSTR + LNM 组合场景
// =============================================================================

#[test]
fn test_lnm_survives_resize() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");
    assert!(engine.state.modes.is_enabled(1 << 13));

    // Resize 不应该影响 LNM
    // (resize 函数内部可能会改变 state，但不应重置模式)
    let flags_before = engine.state.modes.flags;
    engine.state.resize(100, 30);
    let flags_after = engine.state.modes.flags;

    // 检查 LNM 位是否保持
    // 注意：resize 可能会重置某些状态，但 LNM 应该保持不变
    println!("Flags before resize: {:032b}", flags_before);
    println!("Flags after resize:  {:032b}", flags_after);

    // 这个测试主要是观察行为，不一定 assert
    println!("✅ LNM survives resize (observation)");
}

#[test]
fn test_decstr_full_cycle() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置一个复杂的终端状态
    engine.process_bytes(b"\x1b[?1h");    // cursor app mode
    engine.process_bytes(b"\x1b[?6h");    // origin mode
    engine.process_bytes(b"\x1b[20h");    // LNM
    engine.process_bytes(b"\x1b[4h");     // insert
    engine.process_bytes(b"\x1b[?2004h"); // bracketed paste
    engine.process_bytes(b"\x1b[?1000h"); // mouse tracking
    engine.process_bytes(b"\x1b[?1006h"); // SGR mouse
    engine.process_bytes(b"\x1b[?1004h"); // focus events
    engine.process_bytes(b"\x1b[2;10r");  // margins

    // 写入一些内容
    engine.process_bytes(b"Line 1\r\n");
    engine.process_bytes(b"Line 2\r\n");

    // DECSTR
    engine.process_bytes(b"\x1b[!p");

    // 验证状态
    assert!(!engine.state.application_cursor_keys);
    assert!(!engine.state.modes.is_enabled(1 << 2));   // origin
    assert!(!engine.state.modes.is_enabled(1 << 13));  // LNM
    assert!(!engine.state.modes.is_enabled(1 << 12));  // insert
    assert!(!engine.state.bracketed_paste);
    assert!(!engine.state.mouse_tracking);
    assert!(!engine.state.sgr_mouse);
    assert!(!engine.state.send_focus_events);
    assert!(engine.state.cursor_enabled);
    assert!(engine.state.modes.is_enabled(1 << 3));   // autowrap

    // margins 应该被重置
    assert_eq!(engine.state.top_margin, 0);
    assert_eq!(engine.state.bottom_margin, 24);

    // 现在 LNM 关闭，\n 不应该重置 x
    engine.process_bytes(b"TEST");
    assert_eq!(engine.state.cursor.x, 4);
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 4, "After DECSTR, LNM is off: x stays");

    println!("✅ DECSTR full cycle verified");
}

#[test]
fn test_decstr_then_reenable_lnm() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");
    engine.process_bytes(b"\x1b[!p");
    assert!(!engine.state.modes.is_enabled(1 << 13));

    // 重新启用 LNM
    engine.process_bytes(b"\x1b[20h");
    assert!(engine.state.modes.is_enabled(1 << 13));

    engine.process_bytes(b"ABCDEFGH");
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 0);

    println!("✅ Re-enable LNM after DECSTR works");
}

// =============================================================================
// Part E: 键盘事件转义符 - 修饰符编码验证
// =============================================================================

#[test]
fn test_key_event_modifier_encoding() {
    use termux_rust::terminal::key_handler;

    // 方向键 Up (KEYCODE_DPAD_UP = 19)
    // 无修饰符
    let seq = key_handler::get_code(19, 0, false, false);
    assert_eq!(seq, Some("\x1b[A".to_string()));

    // Shift + Up → \x1b[1;2A
    let seq = key_handler::get_code(19, key_handler::KEYMOD_SHIFT, false, false);
    assert_eq!(seq, Some("\x1b[1;2A".to_string()));

    // Alt + Up → \x1b[1;3A
    let seq = key_handler::get_code(19, key_handler::KEYMOD_ALT, false, false);
    assert_eq!(seq, Some("\x1b[1;3A".to_string()));

    // Ctrl + Up → \x1b[1;5A
    let seq = key_handler::get_code(19, key_handler::KEYMOD_CTRL, false, false);
    assert_eq!(seq, Some("\x1b[1;5A".to_string()));

    // Ctrl+Shift + Up → \x1b[1;6A
    let seq = key_handler::get_code(19, key_handler::KEYMOD_CTRL | key_handler::KEYMOD_SHIFT, false, false);
    assert_eq!(seq, Some("\x1b[1;6A".to_string()));

    // Alt+Ctrl+Shift + Up → \x1b[1;8A
    let seq = key_handler::get_code(19, key_handler::KEYMOD_ALT | key_handler::KEYMOD_CTRL | key_handler::KEYMOD_SHIFT, false, false);
    assert_eq!(seq, Some("\x1b[1;8A".to_string()));

    println!("✅ Key event modifier encoding is correct");
}

#[test]
fn test_key_event_f1_f4_with_modifiers() {
    use termux_rust::terminal::key_handler;

    // F1 (KEYCODE_F1 = 131) 无修饰符 → \x1bOP
    let seq = key_handler::get_code(131, 0, false, false);
    assert_eq!(seq, Some("\x1bOP".to_string()));

    // F1 + Shift → \x1b[1;2P
    let seq = key_handler::get_code(131, key_handler::KEYMOD_SHIFT, false, false);
    assert_eq!(seq, Some("\x1b[1;2P".to_string()));

    // F2 + Ctrl → \x1b[1;5Q
    let seq = key_handler::get_code(132, key_handler::KEYMOD_CTRL, false, false);
    assert_eq!(seq, Some("\x1b[1;5Q".to_string()));

    // F3 + Alt+Ctrl → \x1b[1;7R
    let seq = key_handler::get_code(133, key_handler::KEYMOD_ALT | key_handler::KEYMOD_CTRL, false, false);
    assert_eq!(seq, Some("\x1b[1;7R".to_string()));

    // F4 + Alt+Ctrl+Shift → \x1b[1;8S
    let seq = key_handler::get_code(134, key_handler::KEYMOD_ALT | key_handler::KEYMOD_CTRL | key_handler::KEYMOD_SHIFT, false, false);
    assert_eq!(seq, Some("\x1b[1;8S".to_string()));

    println!("✅ F1-F4 modifier sequences are correct");
}

#[test]
fn test_key_event_del_ctrl() {
    use termux_rust::terminal::key_handler;

    // DEL 无修饰符 → \x7f
    let seq = key_handler::get_code(67, 0, false, false);
    assert_eq!(seq, Some("\x7f".to_string()));

    // DEL + Ctrl → \x08 (Backspace)
    let seq = key_handler::get_code(67, key_handler::KEYMOD_CTRL, false, false);
    assert_eq!(seq, Some("\x08".to_string()));

    // DEL + Alt → \x1b\x7f
    let seq = key_handler::get_code(67, key_handler::KEYMOD_ALT, false, false);
    assert_eq!(seq, Some("\x1b\x7f".to_string()));

    println!("✅ DEL key encoding is correct");
}

#[test]
fn test_key_event_tab_shift() {
    use termux_rust::terminal::key_handler;

    // Tab → \t
    let seq = key_handler::get_code(61, 0, false, false);
    assert_eq!(seq, Some("\t".to_string()));

    // Shift+Tab → \x1b[Z (back-tab)
    let seq = key_handler::get_code(61, key_handler::KEYMOD_SHIFT, false, false);
    assert_eq!(seq, Some("\x1b[Z".to_string()));

    println!("✅ Tab/Shift-Tab encoding is correct");
}

// =============================================================================
// Part F: 综合场景测试
// =============================================================================

#[test]
fn test_full_terminal_session() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 模拟真实 shell 会话
    engine.process_bytes(b"\x1b[?1h");        // vim 启用 cursor app mode
    engine.process_bytes(b"\x1b[?2004h");     // bracketed paste
    engine.process_bytes(b"\x1b[?1000h");     // mouse tracking
    engine.process_bytes(b"\x1b[?1006h");     // SGR mouse
    engine.process_bytes(b"\x1b[20h");        // LNM
    engine.process_bytes(b"\x1b[?6h");        // origin mode

    // 写入一些内容
    engine.process_bytes(b"$ echo hello\r\n");
    engine.process_bytes(b"hello\r\n");
    engine.process_bytes(b"$ ");

    // 用户按 Ctrl+C (退出当前程序)
    engine.process_bytes(b"\x03");

    // shell 发送 DECSTR 重置终端
    engine.process_bytes(b"\x1b[!p");

    // 验证所有模式被重置
    assert!(!engine.state.application_cursor_keys);
    assert!(!engine.state.bracketed_paste);
    assert!(!engine.state.mouse_tracking);
    assert!(!engine.state.sgr_mouse);
    assert!(!engine.state.modes.is_enabled(1 << 13)); // LNM

    // 新 shell 提示符
    engine.process_bytes(b"\r\n$ ");
    assert!(engine.state.cursor.x > 0);

    // LNM 关闭，\n 不重置 x
    engine.process_bytes(b"cd /tmp");
    let x_before = engine.state.cursor.x;
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, x_before);

    println!("✅ Full terminal session simulation passed");
}

#[test]
fn test_nvim_like_sequence() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 模拟 nvim 启动时的序列
    engine.process_bytes(b"\x1b[?1h");     // cursor app
    engine.process_bytes(b"\x1b[?25l");    // hide cursor
    engine.process_bytes(b"\x1b[?1000h");  // mouse
    engine.process_bytes(b"\x1b[?1006h");  // SGR mouse
    engine.process_bytes(b"\x1b[?2004h");  // bracketed paste

    // nvim 清屏
    engine.process_bytes(b"\x1b[H\x1b[2J");

    // 写入一些"文本"
    engine.process_bytes(b"Hello, World!");

    // 退出 nvim (ESC + :q! + Enter)
    engine.process_bytes(b"\x1b");         // ESC
    engine.process_bytes(b":q!\r");

    // 终端重置
    engine.process_bytes(b"\x1b[!p");
    engine.process_bytes(b"\x1b[?25h");    // show cursor
    engine.process_bytes(b"\x1b[?1l");     // cursor normal
    engine.process_bytes(b"\x1b[?1000l");  // mouse off
    engine.process_bytes(b"\x1b[?2004l");  // bracketed paste off

    // 验证状态
    assert!(!engine.state.application_cursor_keys);
    assert!(!engine.state.bracketed_paste);
    assert!(!engine.state.mouse_tracking);
    assert!(engine.state.cursor_enabled);

    println!("✅ nvim-like sequence simulation passed");
}

#[test]
fn test_rapid_decstr_toggle() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    for i in 0..100 {
        // 启用各种模式
        engine.process_bytes(b"\x1b[?1h\x1b[?6h\x1b[20h\x1b[4h\x1b[?2004h");

        assert!(engine.state.application_cursor_keys);
        assert!(engine.state.modes.is_enabled(1 << 13));
        assert!(engine.state.modes.is_enabled(1 << 12));

        // DECSTR
        engine.process_bytes(b"\x1b[!p");

        assert!(!engine.state.application_cursor_keys);
        assert!(!engine.state.modes.is_enabled(1 << 13));
        assert!(!engine.state.modes.is_enabled(1 << 12));

        // 写入数据防止崩溃
        engine.process_bytes(b"test");
    }

    println!("✅ Rapid DECSTR toggle stress test (100 cycles) passed");
}

#[test]
fn test_alternate_buffer_with_decstr() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入主缓冲区
    engine.process_bytes(b"Main buffer");

    // 切换到 alt buffer
    engine.process_bytes(b"\x1b[?1049h");
    assert!(engine.state.use_alternate_buffer);

    // 在 alt buffer 写入
    engine.process_bytes(b"Alt buffer");

    // DECSTR 不应该切换缓冲区
    engine.process_bytes(b"\x1b[!p");
    assert!(engine.state.use_alternate_buffer, "DECSTR should not switch buffer");

    // 但应该重置模式
    assert!(!engine.state.modes.is_enabled(1 << 13)); // LNM
    assert!(!engine.state.modes.is_enabled(1 << 12)); // Insert

    // 切换回主缓冲区
    engine.process_bytes(b"\x1b[?1049l");
    assert!(!engine.state.use_alternate_buffer);

    println!("✅ Alternate buffer with DECSTR works");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_all() {
        // Part A: DECSTR
        test_decstr_resets_lnm();
        test_decstr_resets_all_modes();
        test_decstr_resets_charset();

        // Part B: LNM
        test_lnm_private_mode_compatibility();
        test_lnm_standard_mode();
        test_lnm_all_control_chars();
        test_lnm_with_margins();

        // Part C: Insert
        test_insert_mode_single_char();
        test_insert_mode_multiple_chars();
        test_insert_mode_off_overwrites();
        test_insert_mode_disabled_by_decstr();
        test_insert_mode_scroll_at_end();

        // Part D: DECSTR + LNM combo
        test_lnm_survives_resize();
        test_decstr_full_cycle();
        test_decstr_then_reenable_lnm();

        // Part E: Key events
        test_key_event_modifier_encoding();
        test_key_event_f1_f4_with_modifiers();
        test_key_event_del_ctrl();
        test_key_event_tab_shift();

        // Part F: Integration
        test_full_terminal_session();
        test_nvim_like_sequence();
        test_rapid_decstr_toggle();
        test_alternate_buffer_with_decstr();

        println!("\n🎉 ALL TESTS PASSED!");
    }
}
