// android_logger 是 Android 专属依赖（见 Cargo.toml 的
// [target.'cfg(target_os = "android")'.dependencies]）。非 Android 目标上这个文件
// 编不过（E0432: unresolved import `android_logger`），而 `cargo clippy --all-targets`
// / `cargo test --all-targets` 会把每个 tests/*.rs 都当独立 target 编译，于是整条
// 宿主侧门禁都会被这一个文件卡住。这里用 cfg 门把整个测试文件在非 Android 上关掉，
// 而不是把 android_logger 提升成跨平台依赖（它在 Linux 上没有可用的后端）。
#![cfg(target_os = "android")]

use android_logger::Config;
use log::LevelFilter;

#[test]
fn test_logger_config_is_valid() {
    // 验证能够通过编译并初始化配置
    let config = Config::default()
        .with_max_level(LevelFilter::Debug)
        .with_tag("TermuxRustSmokeTest");

    // 如果在非 Android 环境下运行，android_logger 通常会优雅降级或静默失败
    // 但我们的目的是验证其 API 稳定性和链接性
    android_logger::init_once(config);

    log::info!("Logger smoke test: API call check.");
    log::error!("If this runs in Termux, check logcat for output!");
}
