use std::sync::Arc;
use std::thread;
use std::time::Duration;
use termux_rust::engine::TerminalEngine;

// 解决 *mut SharedScreenBuffer 不满足 Send/Sync 的问题
// 在测试环境下，我们需要确保 TerminalEngine 可以跨线程使用
struct SendSyncEngine(TerminalEngine);
unsafe impl Send for SendSyncEngine {}
unsafe impl Sync for SendSyncEngine {}

#[test]
fn test_sixel_extended_parsing() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 发送带有参数的 Sixel 开始序列: DCS 100;100;1 q
    // 100x100 像素，透明背景
    engine.process_bytes(b"\x1bP100;100;1q");

    // 发送一些 Sixel 数据
    engine.process_bytes(b"??00??");

    // 发送换行符 '!'
    engine.process_bytes(b"!");

    // 发送更多数据
    engine.process_bytes(b"~~");

    // 结束序列 ST: ESC \
    engine.process_bytes(b"\x1b\\");

    // 验证状态
    assert_eq!(engine.state.sixel_decoder.width, 100);
    assert!(engine.state.sixel_decoder.transparent);
}

#[test]
fn test_unicode_boundary_conditions() {
    let mut engine = TerminalEngine::new(10, 5, 100, 10, 20);

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
                guard.0.state.copy_row_text(row, &mut text);
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
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 测试未闭合的 OSC 序列
    engine.process_bytes(b"\x1b]0;Broken Title");
    // 收到 BEL (0x07) 时应该触发标题更新
    engine.process_bytes(b"\x07Normal Text");

    assert_eq!(engine.state.title.as_deref(), Some("Broken Title"));

    let mut text = [0u16; 80];
    engine.state.copy_row_text(0, &mut text);
    assert_eq!(text[0] as u8, b'N');
}
