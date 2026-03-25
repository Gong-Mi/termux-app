// Resize 性能基准测试 - 对比快速路径 vs 慢速路径
// 运行：cargo test --test resize_benchmark -- --nocapture

use termux_rust::TerminalEngine;
use std::time::Instant;

/// 性能测试配置
const ITERATIONS: usize = 1000;
const TEST_ROWS: i32 = 30;

fn setup_engine(cols: i32, rows: i32) -> TerminalEngine {
    let mut engine = TerminalEngine::new(cols, rows, 100, 10, 20);
    
    // 写入测试内容
    for i in 0..TEST_ROWS {
        let line = format!("Line {:03} with some content to make it longer\r\n", i);
        engine.process_bytes(line.as_bytes());
    }
    
    engine
}

/// 测试快速路径性能（仅行数变化）
#[test]
fn benchmark_resize_fast_path() {
    println!("\n=== Benchmark: Resize Fast Path (Rows Only) ===");
    
    let mut engine = setup_engine(80, 24);
    
    let start = Instant::now();
    
    for _ in 0..ITERATIONS {
        // 仅改变行数 - 应该使用快速路径 O(1)
        engine.state.resize(80, 12);
        engine.state.resize(80, 18);
        engine.state.resize(80, 24);
    }
    
    let duration = start.elapsed();
    let ops_per_sec = (ITERATIONS * 3) as f64 / duration.as_secs_f64();
    
    println!("  Iterations: {}", ITERATIONS);
    println!("  Total time: {:?}", duration);
    println!("  Operations/sec: {:.2}", ops_per_sec);
    println!("  Avg time/resize: {:.2} ns", 
             duration.as_nanos() as f64 / (ITERATIONS * 3) as f64);
}

/// 测试慢速路径性能（列数变化）
#[test]
fn benchmark_resize_slow_path() {
    println!("\n=== Benchmark: Resize Slow Path (Columns Change) ===");
    
    let mut engine = setup_engine(80, 24);
    
    let start = Instant::now();
    
    for _ in 0..ITERATIONS {
        // 改变列数 - 使用慢速路径 O(n)
        engine.state.resize(60, 24);
        engine.state.resize(100, 24);
        engine.state.resize(80, 24);
    }
    
    let duration = start.elapsed();
    let ops_per_sec = (ITERATIONS * 3) as f64 / duration.as_secs_f64();
    
    println!("  Iterations: {}", ITERATIONS);
    println!("  Total time: {:?}", duration);
    println!("  Operations/sec: {:.2}", ops_per_sec);
    println!("  Avg time/resize: {:.2} ns", 
             duration.as_nanos() as f64 / (ITERATIONS * 3) as f64);
}

/// 对比快速路径和慢速路径
#[test]
fn benchmark_resize_comparison() {
    println!("\n=== Benchmark: Fast vs Slow Path Comparison ===");
    println!("  Testing {} iterations each\n", ITERATIONS);
    
    // 快速路径测试
    let mut engine_fast = setup_engine(80, 24);
    let start_fast = Instant::now();
    
    for _ in 0..ITERATIONS {
        engine_fast.state.resize(80, 12);
        engine_fast.state.resize(80, 24);
    }
    
    let duration_fast = start_fast.elapsed();
    
    // 慢速路径测试
    let mut engine_slow = setup_engine(80, 24);
    let start_slow = Instant::now();
    
    for _ in 0..ITERATIONS {
        // 先改列再改回，确保使用慢速路径
        engine_slow.state.resize(79, 12);
        engine_slow.state.resize(80, 24);
    }
    
    let duration_slow = start_slow.elapsed();
    
    // 计算性能提升
    let speedup = duration_slow.as_micros() as f64 / duration_fast.as_micros() as f64;
    let fast_ns = duration_fast.as_nanos() as f64 / (ITERATIONS * 2) as f64;
    let slow_ns = duration_slow.as_nanos() as f64 / (ITERATIONS * 2) as f64;
    
    println!("  Fast Path (rows only):");
    println!("    Total time: {:?}", duration_fast);
    println!("    Avg time/resize: {:.2} ns", fast_ns);
    
    println!("\n  Slow Path (columns change):");
    println!("    Total time: {:?}", duration_slow);
    println!("    Avg time/resize: {:.2} ns", slow_ns);
    
    println!("\n  ┌─────────────────────────────────────────┐");
    println!("  │ Performance Summary                     │");
    println!("  ├─────────────────────────────────────────┤");
    println!("  │ Fast Path: {:>8.2} ns/resize          │", fast_ns);
    println!("  │ Slow Path: {:>8.2} ns/resize          │", slow_ns);
    println!("  │ Speedup:   {:>8.2}x                    │", speedup);
    println!("  └─────────────────────────────────────────┘");
    
    // 验证：快速路径应该显著快于慢速路径
    assert!(speedup > 1.0, "Fast path should be faster than slow path");
    
    if speedup > 10.0 {
        println!("\n  ✅ Fast path is {:.1}x faster - optimization successful!", speedup);
    } else if speedup > 5.0 {
        println!("\n  ✅ Fast path is {:.1}x faster - good improvement!", speedup);
    } else {
        println!("\n  ⚠️  Fast path is only {:.1}x faster - expected more speedup", speedup);
    }
}

/// 测试不同尺寸的 resize 性能
#[test]
fn benchmark_resize_various_sizes() {
    println!("\n=== Benchmark: Various Resize Sizes ===");
    
    let test_cases = vec![
        (80, 24, 80, 12),   // 80x24 → 80x12 (快)
        (80, 24, 80, 48),   // 80x24 → 80x48 (快)
        (80, 24, 40, 24),   // 80x24 → 40x24 (慢)
        (80, 24, 120, 24),  // 80x24 → 120x24 (慢)
        (80, 24, 40, 12),   // 80x24 → 40x12 (慢)
    ];
    
    for (from_cols, from_rows, to_cols, to_rows) in test_cases {
        let is_fast = from_cols == to_cols;
        let path_type = if is_fast { "FAST" } else { "SLOW" };
        
        let mut engine = setup_engine(from_cols, from_rows);
        
        let start = Instant::now();
        
        for _ in 0..(ITERATIONS / 5) {
            engine.state.resize(to_cols, to_rows);
            engine.state.resize(from_cols, from_rows);
        }
        
        let duration = start.elapsed();
        let avg_ns = duration.as_nanos() as f64 / (ITERATIONS / 5 * 2) as f64;
        
        println!("  {} {}x{} → {}x{}: {:>8.2} ns/resize",
                 path_type, from_cols, from_rows, to_cols, to_rows, avg_ns);
    }
}

/// 内存分配测试
#[test]
fn benchmark_memory_allocations() {
    println!("\n=== Benchmark: Memory Allocations ===");
    
    // 这个测试主要验证快速路径是否真的避免了内存分配
    // 在 Rust 中，我们可以观察性能差异来推断
    
    let mut engine = setup_engine(80, 24);
    
    // 快速路径：应该很少或没有分配
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        engine.state.resize(80, 12);
        engine.state.resize(80, 24);
    }
    let fast_duration = start.elapsed();
    
    // 慢速路径：每次都分配新 buffer
    let mut engine = setup_engine(80, 24);
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        engine.state.resize(79, 12);
        engine.state.resize(80, 24);
    }
    let slow_duration = start.elapsed();
    
    println!("  Fast path total: {:?}", fast_duration);
    println!("  Slow path total: {:?}", slow_duration);
    println!("  Ratio: {:.2}x", 
             slow_duration.as_micros() as f64 / fast_duration.as_micros() as f64);
    
    // 如果快速路径真的避免了分配，应该有明显差异
    assert!(slow_duration > fast_duration, 
            "Slow path should take longer due to allocations");
}

fn main() {
    println!("Resize Performance Benchmark Suite");
    println!("==================================\n");
    
    benchmark_resize_fast_path();
    benchmark_resize_slow_path();
    benchmark_resize_comparison();
    benchmark_resize_various_sizes();
    benchmark_memory_allocations();
    
    println!("\n==================================");
    println!("All benchmarks completed!");
}
