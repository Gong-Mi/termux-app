use termux_rust::vte_parser::{Parser, Perform, Params};
use termux_rust::vte_sve;
use std::time::Instant;

struct DummyHandler;
impl Perform for DummyHandler {
    fn print(&mut self, _c: char) {}
    fn execute(&mut self, _byte: u8) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn csi_dispatch(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
}

#[test]
fn benchmark_vte_parser_throughput() {
    // 准备 10MB 的测试数据 (包含 90% 纯文本和 10% 转义序列)
    let base_text = "This is a line of normal text that should be processed very fast by SVE. ".repeat(10);
    let escape_seq = "\x1b[31mRed\x1b[0m\x1b[1mBold\x1b[0m\n";
    let chunk = (base_text + escape_seq).into_bytes();
    let data = chunk.repeat(150000); // 约 100MB
    
    println!("Data size: {:.2} MB", data.len() as f64 / 1024.0 / 1024.0);

    // 1. 测试标准路径
    let mut parser = Parser::new();
    let mut handler = DummyHandler;
    
    let start = Instant::now();
    parser.advance(&mut handler, &data);
    let duration = start.elapsed();
    
    let speed = (data.len() as f64 / 1024.0 / 1024.0) / duration.as_secs_f64();
    println!("VTE Throughput: {:.2} MB/s (Time: {:?})", speed, duration);

    if vte_sve::has_sve_support() {
        println!("SVE is ENABLED and ACTIVE on this hardware.");
    } else {
        println!("SVE is NOT supported/detected, benchmark shows fallback performance.");
    }
}
