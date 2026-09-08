// android_logger 只在 Android 目标上是依赖；该目标在其他平台无法编译。
// 用 crate 级 cfg 让测试目标在宿主平台上编译为空，而不是让 --all-targets 直接失败。
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
