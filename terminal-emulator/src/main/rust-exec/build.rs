use std::env;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    // 只在 Linux/Android 目标上编译 C 辅助文件
    if target.contains("linux") || target.contains("android") {
        cc::Build::new()
            .file("src/get_regs.c")
            .compile("get_regs");
    }
}
