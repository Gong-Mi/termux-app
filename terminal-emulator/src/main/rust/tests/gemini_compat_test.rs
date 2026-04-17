// Gemini 协议异常证明测试 (Bug Reproduction)
// 运行：cargo test --test gemini_compat_test -- --nocapture

use termux_rust::TerminalEngine;
use termux_rust::terminal::style::EFFECT_UNDERLINE;

fn get_screen_as_text(engine: &TerminalEngine) -> Vec<String> {
    let mut rows = Vec::new();
    let cols = engine.state.cols as usize;
    for r in 0..engine.state.rows {
        let mut text_buf = vec![0u16; cols];
        engine.state.copy_row_text(r, &mut text_buf);
        let row_str: String = text_buf.iter().map(|&c| char::from_u32(c as u32).unwrap_or(' ')).collect();
        rows.push(row_str.trim_end().to_string());
    }
    rows
}

/// 证明问题 1: SGR 样式污染
/// Gemini 发送 \x1b[>4;2m (键盘协议)，Rust 引擎却把它当成了“下划线”样式
#[test]
fn proof_sgr_pollution() {
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);
    
    // 初始状态下不应有下划线效果
    assert!((engine.state.effect & EFFECT_UNDERLINE) == 0, "Initial state should not have underline");

    println!("发送探测指令: \\x1b[>4;2m");
    engine.process_bytes(b"\x1b[>4;2m");

    // 如果问题存在，这里的 assert 会触发失败
    let has_underline = (engine.state.effect & EFFECT_UNDERLINE) != 0;
    
    if has_underline {
        println!("❌ 确认异常: 键盘探测序列被错误识别为 SGR 下划线样式！");
    }

    assert!(!has_underline, "Detection sequence polluted SGR state (Underline effect triggered)");
}

/// 证明问题 2: 内容泄露 (Leak)
/// Gemini 的清理序列只清除了当前行，导致下方旧内容可见
#[test]
fn proof_content_leak() {
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);
    
    // 写入两行旧内容
    engine.process_bytes(b"OLD LINE 1\r\nOLD LINE 2");
    
    // 光标现在在第 2 行。Gemini 发送 2K (清行) + \r (回车)
    println!("执行 Gemini 启动清理序列: \\x1b[2K\\r");
    engine.process_bytes(b"\x1b[2K\r");

    let screen = get_screen_as_text(&engine);
    
    // 检查第 1 行是否依然存在
    let line1_exists = screen[0].contains("OLD LINE 1");
    
    if line1_exists {
        println!("❌ 确认异常: 清理序列后旧内容 'OLD LINE 1' 依然在屏幕上可见！");
    }

    // 这个断言反映了用户的真实体感：执行完 gemini 后，下面还有旧东西
    assert!(!line1_exists, "Old content leaked after Gemini startup sequence");
}

/// 证明问题 3: DA 响应格式错误
/// Gemini 请求次级设备属性，我们却给了初级的格式
#[test]
fn proof_da_response_invalid() {
    let mut engine = TerminalEngine::new(80, 10, 100, 10, 20);
    
    // 模拟应用请求 CSI > c
    engine.process_bytes(b"\x1b[>c");
    
    // 预期的响应应该以 \x1b[> 开头 (DA2)
    // 但目前 Rust 引擎返回的是 \x1b[?6c (DA1)
    
    // 注意：这里需要检查 engine 产生的 TerminalEvent 或回传数据
    // 暂时通过打印观察，或假设它逻辑错误
    println!("发送 DA2 查询: \\x1b[>c");
}
