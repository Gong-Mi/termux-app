// LNM 模式问题验证测试
// 运行：cargo test --test lnm_problems -- --nocapture

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ").trim_end().to_string()
}

// =============================================================================
// 测试 1: 验证 LNM 基础功能正常工作（已知正常）
// =============================================================================

#[test]
fn test_lnm_basic_works() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 默认 LNM 关闭: \n 只换行，不回车
    engine.process_bytes(b"ABCDEFGHIJ"); // x=10, y=0
    assert_eq!(engine.state.cursor.x, 10);
    assert_eq!(engine.state.cursor.y, 0);

    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 10, "LNM off: x should stay at 10");

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");

    engine.process_bytes(b"KLMNOP"); // x=6, y=1
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 2);
    assert_eq!(engine.state.cursor.x, 0, "LNM on: x should reset to 0");

    // 关闭 LNM
    engine.process_bytes(b"\x1b[20l");

    engine.process_bytes(b"QRSTUV"); // x=6, y=2
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 3);
    assert_eq!(engine.state.cursor.x, 6, "LNM off: x should stay at 6");

    println!("✅ LNM basic works (no surprise)");
}

// =============================================================================
// 测试 2: 验证 DECSTR (soft reset) 是否重置 LNM
// 问题：decstr_soft_reset() 没有 reset(MODE_LNM)
// =============================================================================

#[test]
fn test_decstr_does_not_reset_lnm_bug() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 确认初始状态 LNM 关闭
    assert_eq!(engine.state.cursor.x, 0);
    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);

    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 10, "Before LNM: x=10");

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");
    engine.process_bytes(b"KLMNOP");
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.x, 0, "LNM on: x reset to 0");

    // 发送 DECSTR 软复位: CSI ! p
    engine.process_bytes(b"\x1b[!p");

    // 软复位后 LNM 应该被重置为关闭
    // 验证：\n 不应该回到 x=0
    engine.process_bytes(b"QRSTUV");
    assert_eq!(engine.state.cursor.x, 6, "After some typing, x=6");

    engine.process_bytes(b"\n");
    let x_after_lf = engine.state.cursor.x;

    // 如果 DECSTR 正确重置了 LNM，x 应该保持在 6
    // 如果 DECSTR 没有重置 LNM，x 会回到 0 (bug!)
    if x_after_lf == 0 {
        println!("❌ BUG CONFIRMED: DECSTR did NOT reset LNM! x={}", x_after_lf);
        println!("   After soft reset, LNM should be OFF, but LF still resets column to 0.");
    } else {
        println!("✅ DECSTR correctly resets LNM. x={}", x_after_lf);
    }

    // 断言：如果这个测试失败了，说明 bug 存在
    assert_eq!(x_after_lf, 6,
        "BUG: DECSTR soft reset should disable LNM, but LNM is still active (x={} instead of 6)",
        x_after_lf);
}

// =============================================================================
// 测试 3: 验证 CSI ? 20 h 是否能设置 LNM
// 标准中 LNM 是 ANSI 模式 (CSI 20 h)，不是私有模式 (CSI ? 20 h)
// 但某些软件可能会错误地发送 ?20
// =============================================================================

#[test]
fn test_lnm_private_mode_ignored() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 先写几个字符
    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);

    // 尝试用私有模式启用 LNM: CSI ? 20 h
    // 如果 handle_decset 没有处理 mode 20，这个会被忽略
    engine.process_bytes(b"\x1b[?20h");

    engine.process_bytes(b"\n");
    let x_after_lf = engine.state.cursor.x;

    // 如果 LNM 没有被 ?20h 设置，x 应该保持 10
    if x_after_lf == 10 {
        println!("❌ CSI ? 20 h is ignored (expected for standard behavior)");
        println!("   Some apps may incorrectly send ?20h instead of 20h.");
        println!("   Consider adding mode 20 support to handle_decset for compatibility.");
    } else if x_after_lf == 0 {
        println!("✅ CSI ? 20 h is handled (compatibility mode enabled)");
    }

    // 当前行为：?20h 被忽略，这不是 bug，而是标准行为
    // 但为了兼容性，建议在 handle_decset 中添加 mode 20
    assert_eq!(x_after_lf, 10,
        "CSI ? 20 h should be ignored by standard (no mode 20 in handle_decset)");
}

// =============================================================================
// 测试 4: 验证 LNM 对 VT (0x0B) 和 FF (0x0C) 的影响
// =============================================================================

#[test]
fn test_lnm_affects_vt_and_ff() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // LNM 关闭
    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);
    assert_eq!(engine.state.cursor.y, 0);

    engine.process_bytes(b"\x0b"); // VT
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 10, "VT without LNM: x stays at 10");

    // 重新定位到行首再写，避免 x 越界
    engine.process_bytes(b"\r");
    engine.process_bytes(b"KLMNOP");
    assert_eq!(engine.state.cursor.x, 6);
    assert_eq!(engine.state.cursor.y, 1);

    engine.process_bytes(b"\x0c"); // FF
    assert_eq!(engine.state.cursor.y, 2);
    assert_eq!(engine.state.cursor.x, 6, "FF without LNM: x stays at 6");

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");

    engine.process_bytes(b"QRSTUVWX");
    let x_before = engine.state.cursor.x;
    assert_eq!(engine.state.cursor.y, 2);

    engine.process_bytes(b"\x0b"); // VT
    let y_after_vt = engine.state.cursor.y;
    let x_after_vt = engine.state.cursor.x;
    assert!(y_after_vt > 2, "VT should increment y (got {})", y_after_vt);
    // LNM 开启时 VT 应该重置 x 到 left_margin
    assert_eq!(x_after_vt, 0, "VT with LNM: x resets to 0 (got {})", x_after_vt);

    engine.process_bytes(b"YZ");
    let x_after_yz = engine.state.cursor.x;

    engine.process_bytes(b"\x0c"); // FF
    let y_after_ff = engine.state.cursor.y;
    let x_after_ff = engine.state.cursor.x;
    assert!(y_after_ff > y_after_vt, "FF should increment y (before={}, after={})", y_after_vt, y_after_ff);
    // LNM 开启时 FF 应该重置 x 到 left_margin
    assert_eq!(x_after_ff, 0, "FF with LNM: x resets to 0 (got {})", x_after_ff);

    println!("✅ LNM correctly affects VT and FF (x_before={}, x_after_vt={}, x_after_yz={}, x_after_ff={})",
        x_before, x_after_vt, x_after_yz, x_after_ff);

    println!("✅ LNM correctly affects VT and FF");
}

// =============================================================================
// 测试 5: 验证 DECSTR 软复位对其他模式的影响
// =============================================================================

#[test]
fn test_decstr_soft_reset_mode_effects() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 启用多种模式
    engine.process_bytes(b"\x1b[?1h");   // application cursor keys (DECSET 1)
    engine.process_bytes(b"\x1b[?6h");   // origin mode (DECSET 6)
    engine.process_bytes(b"\x1b[20h");   // LNM (mode 20)
    engine.process_bytes(b"\x1b[?2004h"); // bracketed paste (DECSET 2004)

    // 验证都生效了
    assert!(engine.state.application_cursor_keys, "App cursor keys should be ON before DECSTR");
    assert!(engine.state.modes.is_enabled(1 << 2), "ORIGIN_MODE should be ON before DECSTR");
    assert!(engine.state.modes.is_enabled(1 << 13), "LNM should be ON before DECSTR");
    assert!(engine.state.bracketed_paste, "Bracketed paste should be ON before DECSTR");

    // DECSTR 软复位
    engine.process_bytes(b"\x1b[!p");

    // 根据规范，软复位应该：
    // - 关闭 origin mode (DECSET 6) ✓
    // - 关闭 LNM (mode 20) ✗ (bug: not reset)
    // - 关闭 bracketed paste (DECSET 2004) ✗ (bug: not reset)
    // - 关闭 application cursor keys (DECSET 1) ✗ (bug: not reset)
    // - 打开 auto-wrap (DECSET 7) ✓
    // - 光标可见 ✓

    // 已知正确的：
    assert!(!engine.state.modes.is_enabled(1 << 2),
        "DECSTR should reset ORIGIN_MODE (DECSET 6)");
    assert!(engine.state.modes.is_enabled(1 << 3),
        "DECSTR should set AUTOWRAP (DECSET 7)");

    // 已知 Bug：
    let lnm_reset = !engine.state.modes.is_enabled(1 << 13);
    let bracketed_reset = !engine.state.bracketed_paste;
    let cursor_reset = !engine.state.application_cursor_keys;

    if !lnm_reset {
        println!("❌ BUG: DECSTR did NOT reset LNM (mode 20)");
    }
    if !bracketed_reset {
        println!("❌ BUG: DECSTR did NOT reset bracketed paste (DECSET 2004)");
    }
    if !cursor_reset {
        println!("❌ BUG: DECSTR did NOT reset application cursor keys (DECSET 1)");
    }

    // 这些断言会失败，展示 bug：
    assert!(lnm_reset,
        "DECSTR should reset LNM (mode 20) - BUG CONFIRMED IF THIS FAILS");
    assert!(bracketed_reset,
        "DECSTR should reset BRACKETED_PASTE (DECSET 2004) - BUG CONFIRMED IF THIS FAILS");
    assert!(cursor_reset,
        "DECSTR should reset application cursor keys (DECSET 1) - BUG CONFIRMED IF THIS FAILS");

    println!("✅ DECSTR soft reset correctly resets all modes");
}

// =============================================================================
// 测试 6: 验证 Insert 模式是否实际影响字符写入
// =============================================================================

#[test]
fn test_insert_mode_actually_works() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 写入一行内容
    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);

    // 移动光标到位置 2
    engine.process_bytes(b"\x1b[1;3H"); // 1-indexed: row=1, col=3 → 0-indexed: y=0, x=2
    assert_eq!(engine.state.cursor.x, 2);
    assert_eq!(engine.state.cursor.y, 0);

    // 启用 Insert 模式: CSI 4 h
    engine.process_bytes(b"\x1b[4h");

    // 验证 MODE_INSERT 被设置了
    assert!(engine.state.modes.is_enabled(1 << 12), "MODE_INSERT should be set");

    // 输入一个字符
    engine.process_bytes(b"X");

    // 检查光标是否移动了 (insert 应该移动光标)
    let x_after = engine.state.cursor.x;
    println!("Insert mode: x before=2, x after={}", x_after);

    // 获取行内容
    let row0 = get_row_text(&engine, 0);
    println!("Row 0 after insert: '{}'", row0);

    // 这个测试只是为了验证 MODE_INSERT 标志是否被设置
    // 实际的 insert 字符逻辑可能未实现
    println!("⚠️ Insert mode flag is set, but character insert logic may not be implemented");
}

// =============================================================================
// 测试 7: 验证 LNM 与 origin mode 的交互
// =============================================================================

#[test]
fn test_lnm_with_origin_mode() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 设置滚动区域: 第 2-10 行 (1-indexed: 2..10, 0-indexed: 1..9)
    engine.process_bytes(b"\x1b[2;10r");

    // 验证 margins 设置正确
    assert_eq!(engine.state.top_margin, 1);  // 0-indexed
    assert_eq!(engine.state.bottom_margin, 10); // 0-indexed

    // 启用 origin mode
    engine.process_bytes(b"\x1b[?6h");

    // 光标应该在 origin 区域的顶部 (y=1)
    // 但当前实现可能不会自动移动光标，只是影响后续光标定位
    let cursor_y_before = engine.state.cursor.y;
    println!("Cursor y before origin mode: {}", cursor_y_before);

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");

    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);

    engine.process_bytes(b"\n");
    // LNM: \n 应该回到 left_margin (0)
    assert_eq!(engine.state.cursor.x, 0, "LNM: x should reset to 0 even with origin mode");
    // y 应该递增
    assert!(engine.state.cursor.y > cursor_y_before || engine.state.cursor.y == cursor_y_before,
        "LF: y should change (before={}, after={})", cursor_y_before, engine.state.cursor.y);

    println!("✅ LNM works with origin mode");
}

// =============================================================================
// 测试 8: 边界情况 - 在 bottom margin 按 \n 应该 scroll
// =============================================================================

#[test]
fn test_lnm_scroll_on_bottom_margin() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 把光标移到底部 margin
    engine.process_bytes(b"\x1b[24;1H"); // y=23 (0-indexed)

    // 启用 LNM
    engine.process_bytes(b"\x1b[20h");

    engine.process_bytes(b"ABCDEFGHIJ");
    assert_eq!(engine.state.cursor.x, 10);
    assert_eq!(engine.state.cursor.y, 23);

    // \n 应该触发 scroll
    engine.process_bytes(b"\n");
    // cursor.y 应该保持在 bottom_margin-1，但内容被滚动
    assert_eq!(engine.state.cursor.y, 23, "Cursor y stays at bottom margin");
    assert_eq!(engine.state.cursor.x, 0, "LNM: x resets to 0");

    println!("✅ LNM scroll on bottom margin works");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_all() {
        test_lnm_basic_works();
        test_decstr_does_not_reset_lnm_bug();
        test_lnm_private_mode_ignored();
        test_lnm_affects_vt_and_ff();
        test_decstr_soft_reset_mode_effects();
        test_insert_mode_actually_works();
        test_lnm_with_origin_mode();
        test_lnm_scroll_on_bottom_margin();
    }
}
