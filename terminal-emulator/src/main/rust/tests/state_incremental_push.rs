// 验证 syncState 增量推送替代轮询
//
// 优化目标：
// 1. Rust 引擎在 process_bytes 后比较前后状态快照
// 2. 只在状态变化时生成 StateChanged 事件（含 mask + 16 个值）
// 3. Java 层通过 onStateChanged(mask, values) 增量更新缓存，消除轮询
//
// Run: cargo test --test state_incremental_push -- --nocapture

use termux_rust::engine::TerminalEngine;
use termux_rust::engine::TerminalEvent;

/// 验证：无数据输入时状态不变化，不产生 StateChanged
#[test]
fn test_no_state_change_on_empty_input() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    engine.process_bytes(b"");
    let events = engine.take_events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, TerminalEvent::StateChanged { .. })),
        "空输入不应产生 StateChanged"
    );
    println!("✅ 空输入不产生 StateChanged");
}

/// 验证：输入普通文本时产生 StateChanged（光标位置变化）
#[test]
fn test_state_change_on_text_input() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    engine.process_bytes(b"hello");
    let events = engine.take_events();
    let state_changes: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            TerminalEvent::StateChanged { mask, values } => Some((*mask, *values)),
            _ => None,
        })
        .collect();

    assert!(
        !state_changes.is_empty(),
        "输入文本应产生 StateChanged 事件"
    );

    let (mask, values) = state_changes[0];
    // 光标列 (bit 0) 应从 0 变为 5
    assert!(mask & 0x01 != 0, "光标列应变化");
    assert_eq!(values[0], 5, "光标列应为 5（'hello' 长度）");
    // 光标行 (bit 1) 不应变化
    assert!(mask & 0x02 == 0, "光标行不应变化");

    println!(
        "✅ 文本输入产生 StateChanged：mask=0x{:04X}, cursor_col={}",
        mask, values[0]
    );
}

/// 验证：批量输入后只产生一次 StateChanged（合并到单个事件）
#[test]
fn test_single_state_changed_per_process_bytes() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    // 一次处理多行数据
    engine.process_bytes(b"line1\nline2\nline3\n");
    let events = engine.take_events();
    let state_change_count = events
        .iter()
        .filter(|e| matches!(e, TerminalEvent::StateChanged { .. }))
        .count();

    assert_eq!(
        state_change_count, 1,
        "单次 process_bytes 应只产生一个 StateChanged 事件"
    );
    println!("✅ 单次 process_bytes 只产生 1 个 StateChanged");
}

/// 验证：snapshot 值与 process_bytes 后的实际状态一致
#[test]
fn test_snapshot_consistency() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    engine.process_bytes(b"test");

    let snap = engine.state.snapshot();
    assert_eq!(snap.len(), 16, "Snapshot should have 16 elements");
    assert_eq!(snap[0], 4, "cursor col = 4");
    assert_eq!(snap[1], 0, "cursor row = 0");
    assert_eq!(snap[3], 1, "cursor enabled = true");
    assert_eq!(snap[11], 24, "rows = 24");
    assert_eq!(snap[12], 80, "cols = 80");
    assert_eq!(snap[15], 0, "insert mode = false");

    println!("✅ snapshot 一致性验证通过 (16 elements)");
}

/// 验证：插入模式 (MODE_INSERT) 变化产生 StateChanged (bit 15)
#[test]
fn test_insert_mode_change() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // 发送 CSI 4 h (SM - Set Mode: Insert)
    engine.process_bytes(b"\x1b[4h");
    let events = engine.take_events();
    let (mask, values) = events.iter()
        .find_map(|e| match e {
            TerminalEvent::StateChanged { mask, values } => Some((*mask, *values)),
            _ => None,
        })
        .expect("Insert mode change should trigger StateChanged");

    assert!(mask & 0x8000 != 0, "Mask bit 15 (Insert Mode) should be set");
    assert_eq!(values[15], 1, "Insert mode value should be 1");

    // 发送 CSI 4 l (RM - Reset Mode: Replace)
    engine.process_bytes(b"\x1b[4l");
    let events2 = engine.take_events();
    let (mask2, values2) = events2.iter()
        .find_map(|e| match e {
            TerminalEvent::StateChanged { mask, values } => Some((*mask, *values)),
            _ => None,
        })
        .expect("Insert mode reset should trigger StateChanged");

    assert!(mask2 & 0x8000 != 0, "Mask bit 15 (Insert Mode) should be set for reset");
    assert_eq!(values2[15], 0, "Insert mode value should be 0 after reset");

    println!("✅ 插入模式变化捕捉通过 (Mask bit 15)");
}

/// 验证：ScreenUpdated 事件在任何 process_bytes 后都应存在（回归测试）
#[test]
fn test_screen_updated_event_always_present() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    // 即使空输入也应该产生 ScreenUpdated (作为 Java 侧的心跳信号)
    engine.process_bytes(b"");
    let events = engine.take_events();
    assert!(
        events.iter().any(|e| matches!(e, TerminalEvent::ScreenUpdated)),
        "ScreenUpdated event should be present even for empty input"
    );

    engine.process_bytes(b"A");
    let events2 = engine.take_events();
    assert!(
        events2.iter().any(|e| matches!(e, TerminalEvent::ScreenUpdated)),
        "ScreenUpdated event should be present after text input"
    );

    println!("✅ ScreenUpdated 事件心跳信号回归通过");
}

/// 验证：mask 只标记实际变化的字段
#[test]
fn test_mask_only_changed_fields() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // 第一次输入：光标从 (0,0) → (3,0)
    engine.process_bytes(b"abc");
    let events1 = engine.take_events();
    let (mask1, _) = events1
        .iter()
        .find_map(|e| match e {
            TerminalEvent::StateChanged { mask, values } => Some((*mask, *values)),
            _ => None,
        })
        .expect("应有 StateChanged");

    // 只有光标列变化
    assert_eq!(mask1, 0x01, "首次输入只应改变光标列 (bit 0)");

    // 第二次输入：光标从 (3,0) → (6,0)
    engine.process_bytes(b"def");
    let events2 = engine.take_events();
    let (mask2, _) = events2
        .iter()
        .find_map(|e| match e {
            TerminalEvent::StateChanged { mask, values } => Some((*mask, *values)),
            _ => None,
        })
        .expect("应有 StateChanged");

    // 仍然只有光标列变化
    assert_eq!(mask2, 0x01, "第二次输入仍只改变光标列 (bit 0)");

    println!("✅ mask 精确标记变化字段");
}

/// 验证：状态无变化时 process_bytes 不产生 StateChanged
#[test]
fn test_no_state_change_when_state_unchanged() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // 处理一个换行符（光标从 (0,0) 移动到 (0,1)）
    engine.process_bytes(b"\n");
    let events1 = engine.take_events();
    let has_change1 = events1
        .iter()
        .any(|e| matches!(e, TerminalEvent::StateChanged { .. }));
    assert!(has_change1, "换行应改变光标行");

    // 再处理一个无效果的字节（如 NUL）
    // 注意：大多数字节都会改变状态，这里用空输入测试
    engine.process_bytes(b"");
    let events2 = engine.take_events();
    let has_change2 = events2
        .iter()
        .any(|e| matches!(e, TerminalEvent::StateChanged { .. }));
    assert!(!has_change2, "空输入不应改变状态");

    println!("✅ 空输入不产生 StateChanged");
}
