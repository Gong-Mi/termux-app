//! 终端引擎独立演示
//! 可在 Termux 中直接运行，展示 Rust 终端解析能力

use std::io::{self, Write};

fn main() {
    println!("🦀 Termux Rust Terminal Engine Demo\r\n");
    
    // 测试 ANSI 颜色
    println!("\x1b[31m红色文本\x1b[0m");
    println!("\x1b[32m绿色文本\x1b[0m");
    println!("\x1b[34m蓝色文本\x1b[0m");
    
    // 测试光标移动
    print!("\x1b[10;20H");
    print!("光标定位测试 (行 10, 列 20)\r\n");
    
    // 测试清屏
    print!("\x1b[2J");
    
    io::stdout().flush().unwrap();
    
    println!("\r\n✅ 基本 ANSI 序列测试完成");
}
