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
const DATA_SIZE_MB: usize = 5;
const ANSI_ITERATIONS: usize = 3;
const CURSOR_ITERATIONS: usize = 20000;
const SCROLL_LINES: usize = 10000;
const WIDE_CHAR_COUNT: usize = 500000;
const SMALL_BATCH_ITERATIONS: usize = 100000;

/// 生成随机 ASCII 数据（与 Java 测试使用相同 seed）
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

/// 生成 ANSI 转义序列数据（与 Java 测试使用相同 seed）
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

// =============================================================================
// Raw Text 性能测试
// =============================================================================

/// 测试 Rust 引擎原始文本处理性能
#[test]
fn test_rust_raw_text_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    let data = generate_random_ascii(DATA_SIZE_MB * 1024 * 1024);

    let start = Instant::now();
    engine.process_bytes(&data);
    let duration = start.elapsed();

    let mbps = DATA_SIZE_MB as f64 / duration.as_secs_f64();

    // 输出与 Java 一致的格式
    println!("Rust Raw Text Performance: {:.2} MB/s (Duration: {:.2} s)", mbps, duration.as_secs_f64());
    println!("RUST_RAW_TEXT_MBPS={:.2}", mbps);

    // 阈值：5 MB/s (Rust 应该比 Java 快 2-5 倍)
    assert!(mbps > 5.0, "Rust raw text performance too low: {:.2} MB/s", mbps);
}

// =============================================================================
// ANSI Escape 性能测试
// =============================================================================

/// 测试 Rust 引擎 ANSI 转义序列处理性能
#[test]
fn test_rust_ansi_escape_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    let data = generate_ansi_data(1024 * 1024); // 1MB

    let start = Instant::now();
    for _ in 0..ANSI_ITERATIONS {
        engine.process_bytes(&data);
    }
    let duration = start.elapsed();

    let total_mb = (data.len() * ANSI_ITERATIONS) as f64 / (1024.0 * 1024.0);
    let mbps = total_mb / duration.as_secs_f64();

    // 输出与 Java 一致的格式
    println!("Rust ANSI Escape Performance: {:.2} MB/s (Duration: {:.2} s)", mbps, duration.as_secs_f64());
    println!("RUST_ANSI_MBPS={:.2}", mbps);

    // 阈值：0.5 MB/s
    assert!(mbps > 0.5, "Rust ANSI performance too low: {:.2} MB/s", mbps);
}

// =============================================================================
// 光标移动性能测试
// =============================================================================

/// 测试光标移动性能
#[test]
fn test_cursor_movement_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);

    // 光标移动序列
    let movements = b"\x1b[5;10H\x1b[10;20H\x1b[15;30H\x1b[20;40H\x1b[1;1H";

    let start = Instant::now();
    for _ in 0..CURSOR_ITERATIONS {
        engine.process_bytes(movements);
    }
    let duration = start.elapsed();

    let ops_per_sec = CURSOR_ITERATIONS as f64 / duration.as_secs_f64();
    let kops = ops_per_sec / 1000.0;

    // 输出与 Java 一致的格式
    println!("Cursor Movement Performance: {:.0} ops/s (Duration: {:.2} s)", ops_per_sec, duration.as_secs_f64());
    println!("RUST_CURSOR_OPS={:.0}", kops);

    // 阈值：100,000 ops/s
    assert!(ops_per_sec > 100000.0, "Cursor movement performance too low: {:.0} ops/s", ops_per_sec);
}

// =============================================================================
// 滚动性能测试
// =============================================================================

/// 测试滚动性能
#[test]
fn test_scrolling_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);

    // 生成 SCROLL_LINES 行文本（触发滚动）
    let mut lines = Vec::new();
    for i in 0..SCROLL_LINES {
        let line = format!("Line {}\r\n", i);
        lines.extend_from_slice(line.as_bytes());
    }

    let start = Instant::now();
    engine.process_bytes(&lines);
    let duration = start.elapsed();

    let lines_per_sec = SCROLL_LINES as f64 / duration.as_secs_f64();
    let klines = lines_per_sec / 1000.0;

    // 输出与 Java 一致的格式
    println!("Scrolling Performance: {:.0} lines/s (Duration: {:.2} s)", lines_per_sec, duration.as_secs_f64());
    println!("RUST_SCROLL_LINES={:.0}", klines);

    // 阈值：50,000 lines/s
    assert!(lines_per_sec > 50000.0, "Scrolling performance too low: {:.0} lines/s", lines_per_sec);
}

// =============================================================================
// 宽字符（中文）性能测试
// =============================================================================

/// 测试宽字符（中文）处理性能
#[test]
fn test_wide_char_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);

    // 中文字符串（每个字符占 2 列）
    let chinese_text = "你好世界 ".repeat(WIDE_CHAR_COUNT / 5); // 500,000 字符
    let data = chinese_text.as_bytes();

    let start = Instant::now();
    engine.process_bytes(data);
    let duration = start.elapsed();

    let chars_per_sec = WIDE_CHAR_COUNT as f64 / duration.as_secs_f64();
    let kchars = chars_per_sec / 1000.0;

    // 输出与 Java 一致的格式
    println!("Wide Char Performance: {:.0} chars/s (Duration: {:.2} s)", chars_per_sec, duration.as_secs_f64());
    println!("RUST_WIDECHAR_OPS={:.0}", kchars);

    // 阈值：100,000 chars/s
    assert!(chars_per_sec > 100000.0, "Wide char performance too low: {:.0} chars/s", chars_per_sec);
}

// =============================================================================
// 小批量高频调用性能测试
// =============================================================================

/// 基准测试：小批量高频调用
#[test]
fn test_small_batch_performance() {
    let mut engine = TerminalEngine::new(COLS, ROWS, 100, 10, 20);
    let small_batch = b"Hello World\r\n";

    let start = Instant::now();
    for _ in 0..SMALL_BATCH_ITERATIONS {
        engine.process_bytes(small_batch);
    }
    let duration = start.elapsed();

    let calls_per_sec = SMALL_BATCH_ITERATIONS as f64 / duration.as_secs_f64();
    let kcalls = calls_per_sec / 1000.0;

    // 输出与 Java 一致的格式
    println!("Small Batch Performance: {:.0} calls/s (Duration: {:.2} s)", calls_per_sec, duration.as_secs_f64());
    println!("RUST_SMALLBATCH_OPS={:.0}", kcalls);

    // 阈值：100,000 calls/s
    assert!(calls_per_sec > 100000.0, "Small batch performance too low: {:.0} calls/s", calls_per_sec);
}

// =============================================================================
// 批量读取优化性能测试
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

    // 模拟逐行读取
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
    assert!(rows_per_sec > 1000000.0, "Batch row read performance too low: {:.0} rows/s", rows_per_sec);
}

/// 测试全屏批量读取性能
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

    // 方式 1：逐行读取
    let start_single = Instant::now();
    for _ in 0..iterations {
        for row in 0..ROWS as usize {
            engine.state.copy_row_text(row, &mut text_buffer);
            engine.state.copy_row_styles(row, &mut style_buffer);
        }
    }
    let duration_single = start_single.elapsed();

    // 方式 2：批量读取
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

    // 批量方式不应该比单行方式慢太多
    assert!(speedup < 5.0, "Batch read should not be significantly slower than single read");
}
