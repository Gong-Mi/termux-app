// VTE 解析器细粒度性能测试
// 运行：cargo test --test vte_parser_benchmark -- --nocapture

use termux_rust::vte_parser::{Parser, Params, Perform};
use std::time::{Duration, Instant};

// ============================================================
// 辅助：测量一个操作的耗时
// ============================================================
fn measure<F: FnOnce()>(f: F) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

// ============================================================
// 空执行器 - 用于测量解析器纯开销
// ============================================================
struct NullHandler;
impl Perform for NullHandler {
    fn print(&mut self, _c: char) {}
    fn execute(&mut self, _byte: u8) {}
    fn csi_dispatch(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _finished: bool) {}
}

// ============================================================
// 计数执行器 - 统计各类操作频率
// ============================================================
#[derive(Default)]
struct StatsHandler {
    print_count: usize,
    execute_count: usize,
    csi_count: usize,
    esc_count: usize,
    osc_count: usize,
    param_count: usize,
}

impl Perform for StatsHandler {
    fn print(&mut self, _c: char) { self.print_count += 1; }
    fn execute(&mut self, byte: u8) { 
        self.execute_count += 1;
        // 调用默认实现以模拟完整逻辑
        match byte {
            0x07 => self.bell(),
            0x08 => self.backspace(),
            0x09 => self.tab(),
            0x0A | 0x0B | 0x0C => self.linefeed(),
            0x0D => self.carriage_return(),
            _ => {}
        }
    }
    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        self.csi_count += 1;
        self.param_count += params.len;
    }
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) { self.esc_count += 1; }
    fn osc_dispatch(&mut self, params: &[&[u8]], _finished: bool) {
        self.osc_count += 1;
        self.param_count += params.len();
    }
}

// ============================================================
// 辅助：数据生成器
// ============================================================
fn gen_pure_ascii(kb: usize) -> Vec<u8> {
    vec![b'A'; kb * 1024]
}

fn gen_heavy_sgr(kb: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(kb * 1024);
    while data.len() < kb * 1024 {
        data.extend_from_slice(b"\x1b[31;1;4mText\x1b[0m");
    }
    data
}

fn gen_cursor_move(kb: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(kb * 1024);
    while data.len() < kb * 1024 {
        data.extend_from_slice(b"\x1b[10;20H");
    }
    data
}

fn gen_complex_csi(kb: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(kb * 1024);
    while data.len() < kb * 1024 {
        data.extend_from_slice(b"\x1b[38;2;255;100;50;48;2;0;0;0m#");
    }
    data
}

// ============================================================
// 测试 1: 解析器各阶段开销分解
// ============================================================
#[test]
fn benchmark_parser_breakdown() {
    println!("\n========== VTE 解析器深度性能分解 ==========\n");
    
    let iterations = 50;
    let data_size_kb = 1024; // 1MB 每轮
    
    let datasets = [
        ("纯 ASCII (打印为主)", gen_pure_ascii(data_size_kb)),
        ("密集 SGR (参数解析为主)", gen_heavy_sgr(data_size_kb)),
        ("光标移动 (CSI 派发为主)", gen_cursor_move(data_size_kb)),
        ("复杂真彩色 (长参数负载)", gen_complex_csi(data_size_kb)),
    ];

    println!("{:<25} | {:>10} | {:>10} | {:>10} | {:>10}", "负载类型", "MB/s", "ms/MB", "ns/Char", "主要瓶颈预测");
    println!("{:-<90}", "");

    for (label, data) in datasets {
        let mut parser = Parser::new();
        let mut handler = NullHandler;
        
        let char_count = String::from_utf8_lossy(&data).chars().count();
        
        // 预热
        parser.advance(&mut handler, &data);
        
        let start = Instant::now();
        for _ in 0..iterations {
            parser.advance(&mut handler, &data);
        }
        let elapsed = start.elapsed();
        
        let total_mb = (data_size_kb * iterations) as f64 / 1024.0;
        let total_chars = char_count * iterations;
        let mbps = total_mb / elapsed.as_secs_f64();
        let ms_per_mb = (elapsed.as_secs_f64() * 1000.0) / total_mb;
        let ns_per_char = (elapsed.as_nanos() as f64) / total_chars as f64;
        
        let bottleneck = match label {
            l if l.contains("ASCII") => "UTF-8 + Print",
            l if l.contains("SGR") => "State Trans + Params",
            l if l.contains("光标") => "CSI Logic",
            _ => "Int Parsing",
        };

        println!("{:<25} | {:>10.2} | {:>8.2} ms | {:>8.1} ns | {}", label, mbps, ms_per_mb, ns_per_char, bottleneck);
    }
}

// ============================================================
// 测试 2: 内部指令分布统计
// ============================================================
#[test]
fn benchmark_instruction_distribution() {
    println!("\n========== 解析器指令分布分析 (1MB 负载) ==========\n");

    let datasets = [
        ("密集 SGR 样式", gen_heavy_sgr(1024)),
        ("复杂真彩色", gen_complex_csi(1024)),
    ];

    println!("{:<20} | {:>10} | {:>10} | {:>10} | {:>10}", "负载类型", "Print", "CSI", "Params", "Avg P/CSI");
    println!("{:-<75}", "");

    for (label, data) in datasets {
        let mut parser = Parser::new();
        let mut stats = StatsHandler::default();
        
        parser.advance(&mut stats, &data);
        
        let avg_params = if stats.csi_count > 0 {
            stats.param_count as f64 / stats.csi_count as f64
        } else { 0.0 };

        println!("{:<20} | {:>10} | {:>10} | {:>10} | {:>10.1}", 
                 label, stats.print_count, stats.csi_count, stats.param_count, avg_params);
    }
}

// ============================================================
// 测试 3: UTF-8 解码对解析的影响
// ============================================================
#[test]
fn benchmark_utf8_impact() {
    println!("\n========== UTF-8 字符处理开销分析 (Per Char) ==========\n");
    
    let iterations = 100_000;
    let ascii_char = 'A';
    let emoji_char = '🚀';

    let measure_char = |c: char, count: usize| {
        let mut parser = Parser::new();
        let mut handler = NullHandler;
        let s = c.to_string().repeat(100);
        let bytes = s.as_bytes();
        
        let start = Instant::now();
        for _ in 0..(count / 100) {
            parser.advance(&mut handler, bytes);
        }
        start.elapsed().as_nanos() as f64 / count as f64
    };

    let ns_ascii = measure_char(ascii_char, iterations);
    let ns_emoji = measure_char(emoji_char, iterations);

    println!("  ASCII 字符处理 (1字节): {:>10.2} ns/char", ns_ascii);
    println!("  Emoji 字符处理 (4字节): {:>10.2} ns/char", ns_emoji);
    println!("  多字节解码带来的单字额外开销: {:.1}%", 
             (ns_emoji / ns_ascii - 1.0) * 100.0);
}

// ============================================================
// 测试 4: 批次大小对解析性能的影响
// ============================================================
#[test]
fn benchmark_batch_size_influence() {
    println!("\n========== 批次大小对解析性能的影响 (1MB 总量) ==========\n");

    let total_size = 1024 * 1024;
    let data = vec![b'A'; total_size];
    
    let batch_sizes = [1, 16, 64, 256, 1024, 4096, 16384];
    
    println!("{:<15} | {:>10} | {:>10}", "批次大小 (Bytes)", "耗时 (ms)", "单次调用开销 (ns)");
    println!("{:-<45}", "");

    for &size in &batch_sizes {
        let mut parser = Parser::new();
        let mut handler = NullHandler;
        
        let start = Instant::now();
        for chunk in data.chunks(size) {
            parser.advance(&mut handler, chunk);
        }
        let elapsed = start.elapsed();
        
        let calls = total_size / size;
        let ns_per_call = if calls > 0 {
            (elapsed.as_nanos() as f64) / calls as f64
        } else { 0.0 };

        println!("{:<15} | {:>10.2} | {:>10.0}", size, elapsed.as_secs_f64() * 1000.0, ns_per_call);
    }
}

// ============================================================
// 测试 5: Params 结构体性能 (数字解析开销)
// ============================================================
#[test]
fn benchmark_params_overhead() {
    println!("\n========== Params 参数解析性能测试 (1M 次操作) ==========\n");

    let iterations = 1_000_000;
    
    let t_add_digit = measure(|| {
        let mut params = Params::default();
        for _ in 0..iterations {
            params.add_digit(b'5');
        }
    });

    let t_finish_param = measure(|| {
        let mut params = Params::default();
        for _ in 0..iterations {
            params.finish_param();
            if params.len >= 16 { params.reset(); }
        }
    });

    println!("  add_digit (数字累加):    {:>10.2} ns/op", t_add_digit.as_nanos() as f64 / iterations as f64);
    println!("  finish_param (参数切分): {:>10.2} ns/op", t_finish_param.as_nanos() as f64 / iterations as f64);
}
