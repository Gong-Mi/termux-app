use termux_rust::engine::TerminalEngine;
use termux_rust::terminal::style::{encode_style, STYLE_NORMAL, EFFECT_BOLD};

#[test]
fn test_buffer_sync_basic_ascii() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入一段带样式的文本
    let style = encode_style(7, 0, EFFECT_BOLD);
    engine.state.current_style = style;
    engine.process_bytes(b"Hello Rust");

    // 执行同步 (process_bytes 内部已经调用，但我们显式测试确保一致性)
    engine.state.sync_screen_to_flat_buffer();

    let flat = engine.state.flat_buffer.as_ref().expect("Flat buffer should exist");
    
    // 验证前几个字符
    assert_eq!(flat.text_data[0] as u8, b'H');
    assert_eq!(flat.text_data[1] as u8, b'e');
    assert_eq!(flat.style_data[0], style);
    assert_eq!(flat.style_data[10], STYLE_NORMAL); // 未写入区域应为默认
}

#[test]
fn test_buffer_sync_wide_chars() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 写入中文字符（宽字符，占两列）
    engine.process_bytes("你好".as_bytes());

    engine.state.sync_screen_to_flat_buffer();
    let flat = engine.state.flat_buffer.as_ref().unwrap();

    // '你' (U+4F60)
    assert_eq!(flat.text_data[0], 0x4F60);
    // 宽字符占位符应为 0
    assert_eq!(flat.text_data[1], 0);
    
    // '好' (U+597D)
    assert_eq!(flat.text_data[2], 0x597D);
    assert_eq!(flat.text_data[3], 0);

    // 验证普通空字符
    assert_eq!(flat.text_data[4] as u8, b' ');
}

#[test]
fn test_buffer_sync_shared_memory() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 模拟分配共享内存
    use termux_rust::engine::shared_buffer::{SharedScreenBuffer, SharedBufferPtr};
    use std::alloc::{alloc, Layout};
    
    let size = SharedScreenBuffer::required_size(80, 100);
    let layout = Layout::from_size_align(size, 8).unwrap();
    let ptr = unsafe { alloc(layout) as *mut SharedScreenBuffer };
    
    unsafe {
        (*ptr).version = 100;
        (*ptr).cols = 80;
        (*ptr).rows = 24;
    }
    
    engine.state.shared_buffer_ptr = SharedBufferPtr(ptr);
    
    // 写入数据
    engine.process_bytes(b"Shared Memory Test");
    
    // 验证共享内存中的版本号是否增加
    unsafe {
        assert!( (*ptr).version > 100, "Version should have incremented after sync");
        
        // 验证共享内存内容（手动计算偏移，text_data 在头部后 16 字节）
        let text_ptr = (ptr as *const u8).add(16) as *const u16;
        assert_eq!(*text_ptr, b'S' as u16);
    }
    
    // 注意：engine drop 时会尝试释放 shared_buffer_ptr，
    // 由于我们是手动 alloc 的，需要确保 engine 能够正常处理。
    // ScreenState 的 Drop 已经包含了对应的 layout 释放逻辑。
}

#[test]
fn test_buffer_sync_clamping_and_fill() {
    let mut engine = TerminalEngine::new(10, 2, 10, 10, 20);
    
    // 写入超过一行的内容，触发滚动
    engine.process_bytes(b"1234567890ABC"); 
    
    engine.state.sync_screen_to_flat_buffer();
    let flat = engine.state.flat_buffer.as_ref().unwrap();

    // 第一行应该是 "1234567890"
    // 第二行应该是 "ABC       " (填充空格)
    assert_eq!(flat.text_data[10] as u8, b'A');
    assert_eq!(flat.text_data[11] as u8, b'B');
    assert_eq!(flat.text_data[12] as u8, b'C');
    assert_eq!(flat.text_data[13] as u8, b' ');
}
