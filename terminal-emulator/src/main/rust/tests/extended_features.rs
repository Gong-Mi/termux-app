use std::sync::Arc;
use std::thread;
use std::time::Duration;
use termux_rust::engine::TerminalEngine;

// 解决 *mut SharedScreenBuffer 不满足 Send/Sync 的问题
// 在测试环境下，我们需要确保 TerminalEngine 可以跨线程使用
struct SendSyncEngine(TerminalEngine);
unsafe impl Send for SendSyncEngine {}
unsafe impl Sync for SendSyncEngine {}

// ============================================================================
// Sixel 图形序列集成测试
// 目标: 通过 TerminalEngine 验证完整的 DCS → Sixel → ST 端到端流程
// ============================================================================

#[test]
fn test_sixel_dcs_parameter_parsing() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // DCS Pq=7;Pi=2;Pa=1 q -> aspect_ratio=(1,1), transparent=true
    engine.process_bytes(b"\x1bP7;2;1q\x1b\\");

    let d = &engine.state.sixel_decoder;
    assert_eq!(d.aspect_ratio, (1, 1));
    assert!(d.transparent);
}

#[test]
fn test_sixel_basic_image_event() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // DCS + single sixel pixel (~ = all 6 bits set) + ST
    engine.process_bytes(b"\x1bPq~\x1b\\");

    // 提取 SixelImage 事件（跳过最后的 ScreenUpdated）
    let image_events: Vec<_> = engine
        .take_events()
        .into_iter()
        .filter(|e| matches!(e, termux_rust::engine::events::TerminalEvent::SixelImage { .. }))
        .collect();

    assert_eq!(image_events.len(), 1, "Expected exactly one SixelImage event");
    if let termux_rust::engine::events::TerminalEvent::SixelImage { width, height, rgba_data, .. } = &image_events[0] {
        assert_eq!(*width, 1);
        assert_eq!(*height, 6);
        assert_eq!(rgba_data.len(), 1 * 6 * 4); // 1 col * 6 rows * 4 bytes RGBA
    }
}

#[test]
fn test_sixel_rle_and_dimensions() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // DCS + repeat 10 times (~) + ST
    engine.process_bytes(b"\x1bPq!10~\x1b\\");

    let d = &engine.state.sixel_decoder;
    assert_eq!(d.width, 10);
    assert_eq!(d.height, 6);
}

#[test]
fn test_sixel_multiline_image() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // Two rows of sixel data separated by '-'
    // Row 0: ~~ (2 cols), Row 6: @ (1 col)
    engine.process_bytes(b"\x1bPq~~-@\x1b\\");

    let d = &engine.state.sixel_decoder;
    assert_eq!(d.height, 12);
    assert_eq!(d.pixel_data[0].len(), 2);   // row 0 has 2 columns
    // New rows are padded to self.width.max(1) at the time of ensure_height,
    // so row 6 gets width=2 (current width after ~~)
    assert_eq!(d.pixel_data[6].len(), 2);
    assert_eq!(d.pixel_data[0][0], 1);      // bit 0 of '~'
    assert_eq!(d.pixel_data[6][0], 1);      // bit 0 of '@'
}

#[test]
fn test_sixel_color_and_event_data() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // Set color 1 to red (RGB 100%), draw one pixel with it
    engine.process_bytes(b"\x1bPq#1;2;100;0;0#1~\x1b\\");

    let image_events: Vec<_> = engine
        .take_events()
        .into_iter()
        .filter(|e| matches!(e, termux_rust::engine::events::TerminalEvent::SixelImage { .. }))
        .collect();

    assert_eq!(image_events.len(), 1);
    if let termux_rust::engine::events::TerminalEvent::SixelImage { rgba_data, .. } = &image_events[0] {
        // ~ fills all 6 rows, all should be red
        assert_eq!(rgba_data.len(), 24);
        for row in 0..6 {
            let off = row * 4;
            assert_eq!(rgba_data[off], 255, "row {} R channel", row);
            assert_eq!(rgba_data[off + 1], 0, "row {} G channel", row);
            assert_eq!(rgba_data[off + 2], 0, "row {} B channel", row);
        }
    }
}

#[test]
fn test_sixel_carriage_return_behavior() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // ~ sets all 6 bits at col 0; $ resets col to 0; @ sets only bit 0
    // Sixel rendering ORs bits, it does not clear on CR
    engine.process_bytes(b"\x1bPq~$@\x1b\\");

    let d = &engine.state.sixel_decoder;
    assert_eq!(d.width, 1);
    // @ only sets bit 0, but ~ had already set all bits at col 0
    // The value in pixel_data is (current_color+1), not a bitmask.
    // Both ~ and @ use color 0, so they write the same value 1.
    assert_eq!(d.pixel_data[0][0], 1);
    assert_eq!(d.pixel_data[1][0], 1); // bit 1 remains from ~
}

#[test]
fn test_sixel_empty_sequence() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // DCS immediately followed by ST with no data
    engine.process_bytes(b"\x1bPq\x1b\\");

    let d = &engine.state.sixel_decoder;
    // start() resets width/height to 0; no process_data called to update them
    assert_eq!(d.height, 0);
    assert_eq!(d.width, 0);
    // But pixel_data is pre-allocated by start()
    assert_eq!(d.pixel_data.len(), 6);
}

#[test]
fn test_unicode_boundary_conditions() {
    let mut engine = TerminalEngine::new(10 as i64, 5 as i64, 100, 10, 20);

    // 在行尾测试宽字符自动换行
    engine.process_bytes("123456789测试".as_bytes());

    // "测试" 的第一个字应该换行到第二行，第二个字也紧随其后
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 4);
}

#[test]
fn test_concurrent_read_write_stress() {
    let engine = Arc::new(std::sync::RwLock::new(SendSyncEngine(TerminalEngine::new(
        80, 24, 2000, 10, 20,
    ))));

    let engine_write = Arc::clone(&engine);
    let writer = thread::spawn(move || {
        for i in 0..100 {
            let mut guard = engine_write.write().unwrap();
            let msg = format!("Line {}\r\n", i);
            guard.0.process_bytes(msg.as_bytes());
            thread::sleep(Duration::from_micros(10));
        }
    });

    let engine_read = Arc::clone(&engine);
    let reader = thread::spawn(move || {
        let mut text = vec![0u16; 80];
        for _ in 0..50 {
            let guard = engine_read.read().unwrap();
            for row in 0..24 {
                guard.0.state.copy_row_text(row as i64, &mut text);
            }
            thread::sleep(Duration::from_micros(20));
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();

    let final_guard = engine.read().unwrap();
    assert!(final_guard.0.state.cursor.y >= 0);
}

#[test]
fn test_osc_malformed_sequences() {
    let mut engine = TerminalEngine::new(80 as i64, 24 as i64, 100, 10, 20);

    // 测试未闭合的 OSC 序列
    engine.process_bytes(b"\x1b]0;Broken Title");
    // 收到 BEL (0x07) 时应该触发标题更新
    engine.process_bytes(b"\x07Normal Text");

    assert_eq!(engine.state.title.as_deref(), Some("Broken Title"));

    let mut text = [0u16; 80];
    engine.state.copy_row_text(0 as i64, &mut text);
    assert_eq!(text[0] as u8, b'N');
}

