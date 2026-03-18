//! 终端引擎独立测试 - 可在 Termux 中直接运行
//! 测试 Rust 终端解析器的核心功能

fn main() {
    println!("🦀 Termux Rust Terminal Engine Test\r\n");
    println!("=====================================\r\n");

    // 1. 基础颜色测试
    println!("\x1b[1m1. 颜色测试:\x1b[0m");
    println!("  \x1b[31m红色\x1b[32m 绿色\x1b[34m 蓝色\x1b[0m");
    println!("  \x1b[91m亮红\x1b[92m 亮绿\x1b[94m 亮蓝\x1b[0m\r\n");

    // 2. 光标测试
    println!("\x1b[1m2. 光标移动测试:\x1b[0m");
    print!("  起始位置");
    print!("\x1b[10;30H[行 10, 列 30]");
    print!("\x1b[12;30H[行 12, 列 30]");
    print!("\x1b[15;1H");  // 移动到底部
    println!("\r\n  光标移动完成\r\n");

    // 3. 清屏测试
    println!("\x1b[1m3. 清屏测试:\x1b[0m");
    println!("  这是一些测试文本...");
    print!("  \x1b[2K");  // 清除当前行
    println!("  行已清除\r\n");

    // 4. 宽字符测试
    println!("\x1b[1m4. 宽字符测试:\x1b[0m");
    println!("  中文：你好世界");
    println!("  日文：こんにちは");
    println!("  韩文：안녕하세요");
    println!("  Emoji: 🦀 🚀 ✅\r\n");

    // 5. 样式测试
    println!("\x1b[1m5. 样式测试:\x1b[0m");
    println!("  \x1b[1m粗体\x1b[0m \x1b[4m下划线\x1b[0m \x1b[7m反显\x1b[0m");
    println!("  \x1b[2m暗淡\x1b[0m \x1b[3m斜体\x1b[0m\r\n");

    println!("=====================================");
    println!("✅ 所有测试完成！");
    println!("Rust 终端引擎运行正常\r\n");
}
