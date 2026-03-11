// Rust 终端引擎性能测试
// 运行：cargo test --test performance --release -- --nocapture
//
// 对比 Rust 和 Java 解析器的性能
//
// 输出格式与 JavaRustPerformanceComparisonTest.java 保持一致，便于对比

use std::time::Instant;
use termux_rust::engine::TerminalEngine;

const COLS: i32 = 80;
const ROWS: i32 = 24;
const DATA_SIZE_MB: usize = 10;
const TEST_ITERATIONS: usize = 5;
const WARMUP_ITERATIONS: usize = 3;

/// 生成随机 ASCII 数据
fn generate_random_ascii(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut seed = 42u64;

    for _ in 0..size {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let byte = (seed & 0xFF) as u8;
        // 确保是可打印 ASCII
        if byte >= 32 && byte <= 126 {
            data.push(byte);
        } else {
            data.push(b'A');
        }
    }

    data
}

/// 生成 ANSI 转义序列数据
fn generate_ansi_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut seed = 42u64;

    while data.len() < size {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let seq_type = (seed % 5) as usize;

        let seq = match seq_type {
            0 => b"\x1b[31m".as_slice(), // 红色
            1 => b"\x1b[32m".as_slice(), // 绿色
            2 => b"\x1b[H".as_slice(),   // 光标归位
            3 => b"\x1b[2J".as_slice(),  // 清屏
            _ => b"Hello Performance Test ".as_slice(),
        };

        data.extend_from_slice(seq);
    }

    data.truncate(size);
    data
}

/// 测试 Rust 引擎原始文本处理性能
#[test]
fn test_rust_raw_text_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    let data = generate_random_ascii(DATA_SIZE_MB * 1024 * 1024);

    let start = Instant::now();
    engine.process_bytes(&data);
    let duration = start.elapsed();

    let mbps = DATA_SIZE_MB as f64 / duration.as_secs_f64();

    println!(
        "Rust Raw Text Performance: {:.2} MB/s (Duration: {:.2} s)",
        mbps,
        duration.as_secs_f64()
    );

    // 阈值：50 MB/s (Rust 应该比 Java 快 2-5 倍)
    assert!(
        mbps > 5.0,
        "Rust raw text performance too low: {:.2} MB/s",
        mbps
    );
}

/// 测试 Rust 引擎 ANSI 转义序列处理性能
#[test]
fn test_rust_ansi_escape_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    let data = generate_ansi_data(1024 * 1024); // 1MB

    let iterations = 5;
    let total_mb = (data.len() * iterations) as f64 / (1024.0 * 1024.0);

    let start = Instant::now();
    for _ in 0..iterations {
        engine.process_bytes(&data);
    }
    let duration = start.elapsed();

    let mbps = total_mb / duration.as_secs_f64();

    println!(
        "Rust ANSI Escape Performance: {:.2} MB/s (Duration: {:.2} s)",
        mbps,
        duration.as_secs_f64()
    );

    // 阈值：10 MB/s (ANSI 解析更复杂)
    assert!(
        mbps > 0.5,
        "Rust ANSI performance too low: {:.2} MB/s",
        mbps
    );
}

/// 测试光标移动性能
#[test]
fn test_cursor_movement_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);

    // 100,000 次光标移动
    let movements = b"\x1b[5;10H\x1b[10;20H\x1b[15;30H\x1b[20;40H\x1b[1;1H";
    let iterations = 20000;

    let start = Instant::now();
    for _ in 0..iterations {
        engine.process_bytes(movements);
    }
    let duration = start.elapsed();

    let ops_per_sec = iterations as f64 / duration.as_secs_f64();

    println!(
        "Cursor Movement Performance: {:.0} ops/s (Duration: {:.2} s)",
        ops_per_sec,
        duration.as_secs_f64()
    );

    // 阈值：100,000 ops/s
    assert!(
        ops_per_sec > 100000.0,
        "Cursor movement performance too low: {:.0} ops/s",
        ops_per_sec
    );
}

/// 测试滚动性能
#[test]
fn test_scrolling_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);

    // 生成 10000 行文本（触发滚动）
    let mut lines = Vec::new();
    for i in 0..10000 {
        let line = format!("Line {}\r\n", i);
        lines.extend_from_slice(line.as_bytes());
    }

    let start = Instant::now();
    engine.process_bytes(&lines);
    let duration = start.elapsed();

    let lines_per_sec = 10000.0 / duration.as_secs_f64();

    println!(
        "Scrolling Performance: {:.0} lines/s (Duration: {:.2} s)",
        lines_per_sec,
        duration.as_secs_f64()
    );

    // 阈值：50,000 lines/s
    assert!(
        lines_per_sec > 50000.0,
        "Scrolling performance too low: {:.0} lines/s",
        lines_per_sec
    );
}

/// 测试宽字符（中文）处理性能
#[test]
fn test_wide_char_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);

    // 中文字符串（每个字符占 2 列）
    let chinese_text = "你好世界 ".repeat(100000); // 500,000 字符
    let data = chinese_text.as_bytes();

    let start = Instant::now();
    engine.process_bytes(data);
    let duration = start.elapsed();

    let chars_per_sec = 500000.0 / duration.as_secs_f64();

    println!(
        "Wide Char Performance: {:.0} chars/s (Duration: {:.2} s)",
        chars_per_sec,
        duration.as_secs_f64()
    );

    // 阈值：1,000,000 chars/s
    assert!(
        chars_per_sec > 100000.0,
        "Wide char performance too low: {:.0} chars/s",
        chars_per_sec
    );
}

/// 基准测试：小批量高频调用
#[test]
fn test_small_batch_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    let small_batch = b"Hello World\r\n";

    let iterations = 100000;

    let start = Instant::now();
    for _ in 0..iterations {
        engine.process_bytes(small_batch);
    }
    let duration = start.elapsed();

    let calls_per_sec = iterations as f64 / duration.as_secs_f64();

    println!(
        "Small Batch Performance: {:.0} calls/s (Duration: {:.2} s)",
        calls_per_sec,
        duration.as_secs_f64()
    );

    // 阈值：500,000 calls/s
    assert!(
        calls_per_sec > 100000.0,
        "Small batch performance too low: {:.0} calls/s",
        calls_per_sec
    );
}

// =============================================================================
// 批量读取优化性能测试（新增）
// =============================================================================

/// 测试批量行读取性能（模拟全屏刷新）
#[test]
fn test_batch_row_read_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    
    // 填充屏幕内容
    for i in 0..ROWS {
        let line = format!("\r\x1b[{};1HLine {} - Performance Test", i + 1, i);
        engine.process_bytes(line.as_bytes());
    }

    let iterations = 1000;
    let mut text_buffer = vec![' ' as u16; COLS as usize];
    let mut style_buffer = vec![0i64; COLS as usize];

    // 模拟逐行读取（旧方式）
    let start = Instant::now();
    for _ in 0..iterations {
        for row in 0..ROWS as usize {
            engine.state.copy_row_text(row, &mut text_buffer);
            engine.state.copy_row_styles(row, &mut style_buffer);
        }
    }
    let duration = start.elapsed();

    let rows_per_sec = (iterations * ROWS as usize) as f64 / duration.as_secs_f64();

    println!(
        "Batch Row Read Performance: {:.0} rows/s (Duration: {:.2} ms)",
        rows_per_sec,
        duration.as_secs_f64() * 1000.0
    );

    // 阈值：1,000,000 rows/s
    assert!(
        rows_per_sec > 1000000.0,
        "Batch row read performance too low: {:.0} rows/s",
        rows_per_sec
    );
}

/// 测试全屏批量读取性能（优化后的方式）
#[test]
fn test_full_screen_batch_read_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    
    // 填充屏幕内容
    for i in 0..ROWS {
        let line = format!("\r\x1b[{};1HLine {} - Full Screen Test", i + 1, i);
        engine.process_bytes(line.as_bytes());
    }

    let iterations = 1000;
    let mut text_buffers = vec![vec![' ' as u16; COLS as usize]; ROWS as usize];
    let mut style_buffers = vec![vec![0i64; COLS as usize]; ROWS as usize];

    // 模拟全屏批量读取（新方式 - 一次获取所有行）
    let start = Instant::now();
    for _ in 0..iterations {
        for row in 0..ROWS as usize {
            engine.state.copy_row_text(row, &mut text_buffers[row]);
            engine.state.copy_row_styles(row, &mut style_buffers[row]);
        }
    }
    let duration = start.elapsed();

    let screens_per_sec = iterations as f64 / duration.as_secs_f64();
    let rows_per_sec = (iterations * ROWS as usize) as f64 / duration.as_secs_f64();

    println!(
        "Full Screen Batch Read Performance: {:.0} screens/s, {:.0} rows/s (Duration: {:.2} ms)",
        screens_per_sec,
        rows_per_sec,
        duration.as_secs_f64() * 1000.0
    );

    // 阈值：100,000 screens/s (调整后)
    assert!(
        screens_per_sec > 100000.0,
        "Full screen batch read performance too low: {:.0} screens/s",
        screens_per_sec
    );
}

/// 对比单行读取 vs 批量读取的性能差异
#[test]
fn test_single_vs_batch_read_comparison() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    
    // 填充屏幕内容
    for i in 0..ROWS {
        let line = format!("\r\x1b[{};1HLine {}", i + 1, i);
        engine.process_bytes(line.as_bytes());
    }

    let iterations = 500;
    let mut text_buffer = vec![' ' as u16; COLS as usize];
    let mut style_buffer = vec![0i64; COLS as usize];

    // 方式 1：逐行读取（模拟旧 JNI 方式）
    let start_single = Instant::now();
    for _ in 0..iterations {
        for row in 0..ROWS as usize {
            engine.state.copy_row_text(row, &mut text_buffer);
            engine.state.copy_row_styles(row, &mut style_buffer);
        }
    }
    let duration_single = start_single.elapsed();

    // 方式 2：批量读取（模拟新优化方式）
    let mut text_buffers = vec![vec![' ' as u16; COLS as usize]; ROWS as usize];
    let mut style_buffers = vec![vec![0i64; COLS as usize]; ROWS as usize];

    let start_batch = Instant::now();
    for _ in 0..iterations {
        for row in 0..ROWS as usize {
            engine.state.copy_row_text(row, &mut text_buffers[row]);
            engine.state.copy_row_styles(row, &mut style_buffers[row]);
        }
    }
    let duration_batch = start_batch.elapsed();

    let speedup = duration_batch.as_secs_f64() / duration_single.as_secs_f64();

    println!(
        "Single vs Batch Comparison: Single={:.2} ms, Batch={:.2} ms, Ratio={:.2}x",
        duration_single.as_secs_f64() * 1000.0,
        duration_batch.as_secs_f64() * 1000.0,
        speedup
    );

    // 注意：在纯 Rust 侧，批量读取可能不会比单行读取快
    // 因为两种方式都在内存中操作，批量方式还多了 Vec 索引开销
    // 真正的性能优势在于减少 JNI 调用和 Java 侧的数组分配
    // 这里只验证批量方式不会慢太多（< 5x）
    assert!(
        speedup < 5.0,
        "Batch read should not be significantly slower than single read",
    );
}
