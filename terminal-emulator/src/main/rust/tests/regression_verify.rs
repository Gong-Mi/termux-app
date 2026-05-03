use termux_rust::TerminalEngine;
use termux_rust::vte_parser::{Parser, Perform, Params};

// ---------- 1. 验证 vte_parser 对无效 UTF-8 的处理 ----------

struct PrintCollector {
    chars: Vec<char>,
}

impl Perform for PrintCollector {
    fn print(&mut self, c: char) {
        self.chars.push(c);
    }
    fn execute(&mut self, _byte: u8) {}
    fn csi_dispatch(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}

#[test]
fn test_invalid_utf8_continuation_byte_should_not_be_silently_dropped() {
    let mut parser = Parser::new();
    let mut handler = PrintCollector { chars: Vec::new() };
    
    // 0x80 是一个孤立的 continuation byte，不是有效的 UTF-8 首字节
    // 旧版 String::from_utf8_lossy 会将其替换成 U+FFFD (�)
    // 新版错误地直接跳过，导致终端没有任何输出
    parser.advance(&mut handler, &[0x80]);
    
    // 验证：不能跳过，必须至少有一个字符被处理（即使是 �）
    // 当前错误行为：handler.chars 为空
    assert!(!handler.chars.is_empty(), 
        "孤立的 continuation byte 0x80 被跳过了。无效 UTF-8 应该被替换成替换字符 (U+FFFD)，而不是直接丢弃。");
}

#[test]
fn test_truncated_utf8_should_not_be_silently_dropped() {
    let mut parser = Parser::new();
    let mut handler = PrintCollector { chars: Vec::new() };
    
    // 0xE0 0x80 是一个截断的 3 字节 UTF-8 序列（缺少第 3 个字节）
    parser.advance(&mut handler, &[0xE0, 0x80]);
    
    assert!(!handler.chars.is_empty(),
        "截断的 UTF-8 序列 [0xE0, 0x80] 被跳过了。截断的 UTF-8 应该被替换成替换字符 (U+FFFD)。");
}

#[test]
fn test_invalid_utf8_sequence_should_not_be_silently_dropped() {
    let mut parser = Parser::new();
    let mut handler = PrintCollector { chars: Vec::new() };
    
    // 0xE0 0x00 0x00：首字节声称是 3 字节序列，但后续不是 continuation bytes
    parser.advance(&mut handler, &[0xE0, 0x00, 0x00]);
    
    assert!(!handler.chars.is_empty(),
        "无效的 UTF-8 序列 [0xE0, 0x00, 0x00] 被跳过了。无效序列应该被替换成替换字符 (U+FFFD)。");
}

#[test]
fn test_fe_ff_bytes_should_not_be_silently_dropped() {
    let mut parser = Parser::new();
    let mut handler = PrintCollector { chars: Vec::new() };
    
    parser.advance(&mut handler, &[0xFE, 0xFF]);
    
    assert!(!handler.chars.is_empty(),
        "0xFE / 0xFF 被跳过了。这些无效字节应该被替换成替换字符 (U+FFFD)。");
}

// ---------- 2. 验证 alt_screen resize 后 buffer 与 rows 不一致 ----------

#[test]
fn test_alt_screen_buffer_size_after_resize() {
    let mut engine = TerminalEngine::new(80 as i64, 24 as i64, 100, 10, 20);
    
    // alt_screen 初始 buffer.len() = 24
    let initial_len = engine.state.alt_screen.buffer.len();
    assert_eq!(initial_len, 24);
    
    // resize 到 120x30
    engine.state.resize(120, 30);
    
    let alt_rows = engine.state.alt_screen.rows;
    let alt_buf_len = engine.state.alt_screen.buffer.len();
    
    // 关键：alt_screen.rows 不应该大于 buffer.len()
    // 否则 get_row(24) 和 get_row(0) 会映射到同一个 buffer 索引，导致内容覆盖
    assert!(
        alt_buf_len >= alt_rows as usize,
        "alt_screen 的 buffer 长度 ({}) 小于 rows ({})。这会导致行索引循环映射，写入第 {} 行时会覆盖第 0 行的内容。",
        alt_buf_len, alt_rows, alt_rows
    );
}
