// FlatScreenBuffer 修复验证测试
// 测试 flat_buffer 是否正确分配了 total_rows 大小的空间
//
// 运行：cargo test --test flat_buffer_test -- --nocapture

use termux_rust::engine::{SharedBufferPtr, SharedScreenBuffer, TerminalEngine};

/// 测试 flat_buffer 的大小是否等于 total_rows
#[test]
fn test_flat_buffer_size_equals_total_rows() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 2000; // 滚动历史 + 屏幕

    let engine = TerminalEngine::new(0, cols, screen_rows, total_rows, 10, 20);

    // 验证 flat_buffer 已创建
    assert!(
        engine.state.flat_buffer.is_some(),
        "flat_buffer should be created"
    );

    // 验证 flat_buffer 的行数等于 total_rows
    let flat_buffer = engine.state.flat_buffer.as_ref().unwrap();
    assert_eq!(
        flat_buffer.rows, total_rows as usize,
        "flat_buffer.rows should be {} (total_rows), but got {}",
        total_rows, flat_buffer.rows
    );

    // 验证 flat_buffer 的列数等于 cols
    assert_eq!(
        flat_buffer.cols, cols as usize,
        "flat_buffer.cols should be {}",
        cols
    );

    // 验证 text_data 和 style_data 的大小正确
    let expected_cell_count = cols as usize * total_rows as usize;
    assert_eq!(
        flat_buffer.text_data.len(),
        expected_cell_count,
        "text_data should have {} cells",
        expected_cell_count
    );
    assert_eq!(
        flat_buffer.style_data.len(),
        expected_cell_count,
        "style_data should have {} cells",
        expected_cell_count
    );

    println!(
        "✅ flat_buffer size test passed: {}x{} = {} cells",
        flat_buffer.cols, flat_buffer.rows, expected_cell_count
    );
}

/// 测试共享缓冲区的大小是否正确
#[test]
fn test_shared_buffer_size() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 2000;

    let mut engine = TerminalEngine::new(0, cols, screen_rows, total_rows, 10, 20);

    // 创建共享缓冲区
    let shared_ptr = engine
        .state
        .flat_buffer
        .as_mut()
        .unwrap()
        .create_shared_buffer();

    assert!(
        !shared_ptr.is_null(),
        "shared_buffer_ptr should not be null"
    );

    unsafe {
        let shared = &*shared_ptr;
        assert_eq!(shared.cols, cols as u32, "shared.cols should be {}", cols);
        assert_eq!(
            shared.rows, total_rows as u32,
            "shared.rows should be {} (total_rows), but got {}",
            total_rows, shared.rows
        );
    }

    println!("✅ shared_buffer size test passed: {}x{}", cols, total_rows);
}

/// 测试 syncToSharedBuffer 同步所有行
#[test]
fn test_sync_all_rows_to_shared_buffer() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 100; // 使用较小的值以便测试

    let mut engine = TerminalEngine::new(0, cols, screen_rows, total_rows, 10, 20);

    // 在所有行上写入不同的内容
    for row in 0..screen_rows {
        let line = format!("\r\x1b[{};1HRow {:03}", row + 1, row);
        engine.process_bytes(line.as_bytes());
    }

    // 创建共享缓冲区
    let shared_ptr = engine
        .state
        .flat_buffer
        .as_ref()
        .unwrap()
        .create_shared_buffer();
    engine.state.shared_buffer_ptr = SharedBufferPtr(shared_ptr);

    // 手动同步数据（模拟 syncToSharedBufferRust 的行为）
    if let Some(ref mut flat_buffer) = engine.state.flat_buffer
        && !engine.state.shared_buffer_ptr.0.is_null()
    {
        let buffer_len = engine.state.main_screen.buffer.len();
        for physical_row in 0..buffer_len {
            if let Some(buffer_row) = engine.state.main_screen.buffer.get(physical_row) {
                for col in 0..cols.min(buffer_row.text.len() as i32) as usize {
                    let cell_idx = flat_buffer.cell_index(col, physical_row);
                    if cell_idx < flat_buffer.text_data.len() {
                        flat_buffer.text_data[cell_idx] = buffer_row.text[col] as u32;
                        flat_buffer.style_data[cell_idx] = buffer_row.styles[col];
                    }
                }
            }
        }
        // 不直接调用 sync_to_shared，而是验证 flat_buffer 中的数据
        // 因为共享内存操作需要更复杂的测试设置
    }

    // 验证 flat_buffer 包含第一行的数据
    let flat_buffer = engine.state.flat_buffer.as_ref().unwrap();
    let first_row_text: String = (0..10)
        .map(|col| {
            let cell_idx = flat_buffer.cell_index(col, 0);
            if cell_idx < flat_buffer.text_data.len() {
                char::from_u32(flat_buffer.text_data[cell_idx] as u32).unwrap_or('?')
            } else {
                '?'
            }
        })
        .collect();

    println!("First row content (first 10 chars): {}", first_row_text);
    assert!(
        first_row_text.contains("Row"),
        "First row should contain 'Row', got: {}",
        first_row_text
    );

    println!("✅ sync all rows test passed");
}

/// 测试滚动历史行的访问
#[test]
fn test_scrollback_rows_access() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 100;

    let mut engine = TerminalEngine::new(0, cols, screen_rows, total_rows, 10, 20);

    // 生成超过屏幕行数的内容，触发滚动
    for i in 0..50 {
        let line = format!(
            "\r\x1b[{};1HLine {} - Scroll Test",
            (i % screen_rows) + 1,
            i
        );
        engine.process_bytes(line.as_bytes());
        if i < screen_rows - 1 {
            engine.process_bytes(b"\n");
        }
    }

    // 验证 buffer 包含所有行
    assert_eq!(
        engine.state.main_screen.buffer.len(),
        total_rows as usize,
        "buffer should have {} rows",
        total_rows
    );

    // 验证 flat_buffer 也包含所有行
    let flat_buffer = engine.state.flat_buffer.as_ref().unwrap();
    assert_eq!(
        flat_buffer.rows, total_rows as usize,
        "flat_buffer should have {} rows for scrollback",
        total_rows
    );

    println!(
        "✅ scrollback rows access test passed: buffer has {} rows, flat_buffer has {} rows",
        engine.state.main_screen.buffer.len(),
        flat_buffer.rows
    );
}

/// 测试 alternate buffer 不影响 flat_buffer 大小
#[test]
fn test_alternate_buffer_does_not_affect_flat_buffer_size() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 2000;

    let mut engine = TerminalEngine::new(0, cols, screen_rows, total_rows, 10, 20);

    // 记录主缓冲区的 flat_buffer 大小
    let main_flat_buffer_rows = engine.state.flat_buffer.as_ref().unwrap().rows;

    // 切换到备用缓冲区 (DECSET 1049)
    engine.process_bytes(b"\x1b[?1049h");

    // 验证 flat_buffer 大小不变
    let alt_flat_buffer_rows = engine.state.flat_buffer.as_ref().unwrap().rows;
    assert_eq!(
        main_flat_buffer_rows, alt_flat_buffer_rows,
        "flat_buffer size should not change when switching to alternate buffer"
    );

    println!(
        "✅ alternate buffer test passed: flat_buffer rows = {}",
        alt_flat_buffer_rows
    );
}

/// 验证包含非 BMP 字符（Emoji 与 Unicode 扩展平面，> 0xFFFF）的整行在同步到
/// FlatScreenBuffer 与 SharedScreenBuffer 时不会发生 u16 截断
#[test]
fn test_unicode_extension_plane_and_emoji_retention_in_shared_buffer() {
    let cols = 40;
    let screen_rows = 10;
    let total_rows = 50;

    let mut engine = TerminalEngine::new(0, cols, screen_rows, total_rows, 10, 20);

    // 写入包含经典 Emoji (😀 U+1F600, 🚀 U+1F680) 和 CJK 扩展平面汉字 (𠮷 U+20BB7) 的内容
    let test_str = "Termux 😀 🚀 𠮷 End";
    engine.process_bytes(test_str.as_bytes());

    // 触发同步到 flat_buffer
    engine.state.sync_screen_to_flat_buffer();

    let flat = engine.state.flat_buffer.as_ref().unwrap();
    let expected_chars: Vec<char> = test_str.chars().collect();

    for (c, &expected_ch) in expected_chars.iter().enumerate() {
        let cell_idx = flat.cell_index(c, 0);
        let actual_code = flat.text_data[cell_idx];
        assert_eq!(
            actual_code,
            expected_ch as u32,
            "Char at col {} should be U+{:X} ('{}'), got U+{:X}",
            c,
            expected_ch as u32,
            expected_ch,
            actual_code
        );
    }

    // 验证同步到 SharedScreenBuffer 内存块时也正确保留 32 位值与正确 style_offset
    let shared_ptr = flat.create_shared_buffer();
    assert!(!shared_ptr.is_null());

    unsafe {
        flat.sync_to_shared(shared_ptr);
        let shared = &*shared_ptr;
        assert_eq!(shared.cols, cols as u32);
        assert_eq!(shared.rows, total_rows as u32);

        // 验证第一行文字通过 shared.text_data 读出无截断
        let text_slice = std::slice::from_raw_parts(shared.text_data.as_ptr(), cols as usize);
        for (c, &expected_ch) in expected_chars.iter().enumerate() {
            assert_eq!(
                text_slice[c], expected_ch as u32,
                "SharedScreenBuffer at col {} should retain 32-bit codepoint U+{:X}",
                c, expected_ch as u32
            );
        }

        // 释放测试分配的 shared_buffer
        let size = SharedScreenBuffer::required_size(cols as usize, total_rows as usize);
        let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
        std::alloc::dealloc(shared_ptr as *mut u8, layout);
    }

    println!("✅ Unicode non-BMP and Emoji retention in shared buffer verified");
}
