mod terminal_engine;
use terminal_engine::TerminalEngine;

fn main() {
    let mut engine = TerminalEngine::new(80, 24);
    
    // 发送带有 ANSI 颜色序列的测试数据
    // \x1b[31m 为红色, \x1b[32m 为绿色
    let test_data = b"Hello, \x1b[31mRust\x1b[0m \x1b[32mTerminal\x1b[0m!";
    engine.parse_bytes(test_data);
    
    println!("Testing Rust Engine Parsing...");
    
    // 检查第 0 行前几个字符
    for col in 0..24 {
        let cell = engine.get_cell(col, 0);
        if cell.char != '\0' {
            let color_info = match cell.fg_color {
                Some((r, g, b)) => format!(" (Color: {},{},{})", r, g, b),
                None => "".to_string(),
            };
            println!("Pos {}: {}{}", col, cell.char, color_info);
        }
    }
}
