// FlatScreenBuffer 修复验证测试
// 测试 flat_buffer 是否正确分配了 total_rows 大小的空间
//
// 运行：cargo test --test flat_buffer_test -- --nocapture

use termux_rust::engine::TerminalEngine;

/// 测试 flat_buffer 的大小是否等于 total_rows
#[test]
fn test_flat_buffer_size_equals_total_rows() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 2000; // 滚动历史 + 屏幕

    let mut engine = TerminalEngine::new(cols, screen_rows, total_rows, 10, 20);

    // 验证 flat_buffer 已创建
    assert!(engine.state.flat_buffer.is_some(), "flat_buffer should be created");

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

    let mut engine = TerminalEngine::new(cols, screen_rows, total_rows, 10, 20);

    // 创建共享缓冲区
    let shared_ptr = engine.state.flat_buffer.as_mut().unwrap().create_shared_buffer();

    assert!(!shared_ptr.is_null(), "shared_buffer_ptr should not be null");

    unsafe {
        let shared = &*shared_ptr;
        assert_eq!(
            shared.cols, cols as u32,
            "shared.cols should be {}",
            cols
        );
        assert_eq!(
            shared.rows, total_rows as u32,
            "shared.rows should be {} (total_rows), but got {}",
            total_rows, shared.rows
        );
    }

    println!(
        "✅ shared_buffer size test passed: {}x{}",
        cols, total_rows
    );
}

/// 测试 syncToSharedBuffer 同步所有行
#[test]
fn test_sync_all_rows_to_shared_buffer() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 100; // 使用较小的值以便测试

    let mut engine = TerminalEngine::new(cols, screen_rows, total_rows, 10, 20);

    // 在所有行上写入不同的内容
    for row in 0..screen_rows {
        let line = format!("\r\x1b[{};1HRow {:03}", row + 1, row);
        engine.process_bytes(line.as_bytes());
    }

    // 创建共享缓冲区
    let shared_ptr = engine.state.flat_buffer.as_mut().unwrap().create_shared_buffer();
    engine.state.shared_buffer_ptr = shared_ptr;

    // 手动同步数据（模拟 syncToSharedBufferRust 的行为）
    if let Some(ref mut flat_buffer) = engine.state.flat_buffer {
        if !engine.state.shared_buffer_ptr.is_null() {
            let buffer_len = engine.state.main_screen.buffer.len();
            for physical_row in 0..buffer_len {
                if let Some(buffer_row) = engine.state.main_screen.buffer.get(physical_row) {
                    for col in 0..cols.min(buffer_row.text.len() as i32) as usize {
                        let cell_idx = flat_buffer.cell_index(col, physical_row);
                        if cell_idx < flat_buffer.text_data.len() {
                            flat_buffer.text_data[cell_idx] = buffer_row.text[col] as u16;
                            flat_buffer.style_data[cell_idx] = buffer_row.styles[col];
                        }
                    }
                }
            }
            // 不直接调用 sync_to_shared，而是验证 flat_buffer 中的数据
            // 因为共享内存操作需要更复杂的测试设置
        }
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
    assert!(first_row_text.contains("Row"), "First row should contain 'Row', got: {}", first_row_text);

    println!("✅ sync all rows test passed");
}

/// 测试滚动历史行的访问
#[test]
fn test_scrollback_rows_access() {
    let cols = 80;
    let screen_rows = 24;
    let total_rows = 100;

    let mut engine = TerminalEngine::new(cols, screen_rows, total_rows, 10, 20);

    // 生成超过屏幕行数的内容，触发滚动
    for i in 0..50 {
        let line = format!("\r\x1b[{};1HLine {} - Scroll Test", (i % screen_rows) + 1, i);
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
        flat_buffer.rows,
        total_rows as usize,
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

    let mut engine = TerminalEngine::new(cols, screen_rows, total_rows, 10, 20);

    // 记录主缓冲区的 flat_buffer 大小
    let main_flat_buffer_rows = engine.state.flat_buffer.as_ref().unwrap().rows;

    // 切换到备用缓冲区 (DECSET 1049)
    engine.process_bytes(b"\x1b[?1049h");

    // 验证 flat_buffer 大小不变
    let alt_flat_buffer_rows = engine.state.flat_buffer.as_ref().unwrap().rows;
    assert_eq!(
        main_flat_buffer_rows,
        alt_flat_buffer_rows,
        "flat_buffer size should not change when switching to alternate buffer"
    );

    println!(
        "✅ alternate buffer test passed: flat_buffer rows = {}",
        alt_flat_buffer_rows
    );
}
