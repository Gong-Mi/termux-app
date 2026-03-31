use log::LevelFilter;
use android_logger::Config;

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
