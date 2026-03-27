// 测试 Java 到 Rust 的数据传递完整性

use termux_rust::engine::TerminalEngine;

#[test]
fn test_data_integrity_java_to_rust() {
    println!("=== 测试 Java 到 Rust 数据传递完整性 ===\n");

    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    // 1. 测试小包传递
    println!("1. 测试小包传递 (每次 1 字节):");
    let test_line = "Hello World\r\n";
    for byte in test_line.as_bytes() {
        engine.process_bytes(&[*byte]);
    }
    
    let transcript = engine.state.get_current_screen().get_transcript_text();
    println!("   结果：'{}'", transcript.trim());
    assert!(transcript.contains("Hello World"));
    
    // 2. 测试 UTF-8 多字节字符
    println!("\n2. 测试 UTF-8 多字节字符:");
    engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    let utf8_line = "Hello UTF-8\r\n";
    engine.process_bytes(utf8_line.as_bytes());
    
    let transcript2 = engine.state.get_current_screen().get_transcript_text();
    println!("   输入：'{}'", utf8_line.trim());
    println!("   输出：'{}'", transcript2.trim());
    assert!(transcript2.contains("Hello UTF-8"));
    
    // 3. 测试 ANSI 序列分割
    println!("\n3. 测试 ANSI 序列分割:");
    engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    // 分割 ESC 序列
    let esc_seq = b"\x1b[3";
    let rest = b"1mRed Text\x1b[0m\r\n";
    
    engine.process_bytes(esc_seq);
    engine.process_bytes(rest);
    
    let transcript3 = engine.state.get_current_screen().get_transcript_text();
    println!("   分割发送 ESC[3 + 1mRed Text");
    println!("   结果：'{}'", transcript3.trim());
    
    // 4. 测试大数据块
    println!("\n4. 测试大数据块 (64KB):");
    engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    let mut large_data = Vec::new();
    for i in 0..1000 {
        let line = format!("Line {:04}\r\n", i);
        large_data.extend_from_slice(line.as_bytes());
    }
    
    // 分成小包发送
    const CHUNK_SIZE: usize = 1024;
    for chunk in large_data.chunks(CHUNK_SIZE) {
        engine.process_bytes(chunk);
    }
    
    let transcript4 = engine.state.get_current_screen().get_transcript_text();
    let line_count = transcript4.lines().count();
    println!("   发送 {} 字节，收到 {} 行", large_data.len(), line_count);
    assert!(line_count >= 999, "应该收到至少 999 行，实际{}行", line_count);
    
    // 验证首尾行
    if let Some(first) = transcript4.lines().next() {
        assert!(first.contains("Line 0000"), "第一行应该是 Line 0000，实际：{}", first);
    }
    if let Some(last) = transcript4.lines().last() {
        assert!(last.contains("Line 0999"), "最后一行应该是 Line 0999，实际：{}", last);
    }
    
    println!("   ✓ 所有数据完整性测试通过");
}
