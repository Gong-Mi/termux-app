//! Termux Rust 终端模拟器库
//!
//! 模块化的终端模拟器实现，提供：
//! - VTE 兼容的终端状态管理
//! - 256色/真彩色支持
//! - Sixel 图像渲染
//! - Vulkan/Skia GPU 渲染
//! - JNI 接口供 Java 层调用

use once_cell::sync::OnceCell;
use std::sync::Mutex;

/// 全局存储 Termux 应用版本号（由 Java 层通过 JNI 传入）
pub static TERMUX_VERSION: OnceCell<Mutex<String>> = OnceCell::new();

/// 全局存储 Termux Prefix 路径（由 Java 层通过 JNI 传入，通常为 /data/data/com.termux/files/usr）
pub static TERMUX_PREFIX: OnceCell<Mutex<String>> = OnceCell::new();

/// 获取动态 Prefix 路径，若未初始化则回退到默认路径
pub fn get_termux_prefix() -> String {
    TERMUX_PREFIX
        .get()
        .and_then(|m| m.lock().ok())
        .map(|s| s.clone())
        .unwrap_or_else(|| "/data/data/com.termux/files/usr".to_string())
}

/// 验证并校正 Termux Prefix 路径。
///
/// termux-exec、apt 等 Termux 生态组件在底层硬编码了 `/data/data/com.termux`
/// 进行 W^X bypass 路径匹配。若传入 `/data/user/0/...` 等动态路径，
/// 会导致所有 ELF 执行返回 Permission denied（Android 10+）。
///
/// 此函数作为 Rust 侧的防火墙：接受唯一合法路径，其余一律回退到默认值。
pub fn validate_termux_prefix(input: &str) -> String {
    const REQUIRED_PREFIX: &str = "/data/data/com.termux/files/usr";
    if input == REQUIRED_PREFIX {
        input.to_string()
    } else {
        crate::utils::android_log(
            crate::utils::LogPriority::ERROR,
            &format!(
                "[PREFIX] Rejected invalid prefix '{}': termux-exec hardcodes /data/data/com.termux. \
                 Using fallback '{}'.",
                input, REQUIRED_PREFIX
            ),
        );
        REQUIRED_PREFIX.to_string()
    }
}

/// 获取动态 Home 路径（基于 Prefix）
pub fn get_termux_home() -> String {
    let prefix = get_termux_prefix();
    if let Some(parent) = std::path::Path::new(&prefix).parent() {
        parent.join("home").to_string_lossy().to_string()
    } else {
        "/data/data/com.termux/files/home".to_string()
    }
}

// 声明子模块
pub mod bootstrap;
pub mod coordinator;
pub mod engine;
pub mod env_builder;
pub mod jni_bindings;
pub mod pty;
pub mod render_thread;
pub mod renderer;
pub mod sve_scan;
pub mod terminal;
pub mod utils;
pub mod vte_parser;
pub mod vulkan_context;
pub mod wcwidth;

// 重新导出主要类型，保持向后兼容
pub use crate::coordinator::{SessionCoordinator, SessionState};
pub use crate::engine::{TerminalContext, TerminalEngine, TerminalEvent};
pub use crate::renderer::{HdrColorSpace, HdrImageOverlay, HdrOverlayManager};
pub use crate::terminal::colors::*;
pub use crate::terminal::modes::*;
pub use crate::terminal::sixel::{SixelColor, SixelDecoder, SixelState};
pub use crate::terminal::style::*;

pub static JAVA_VM: OnceCell<jni::JavaVM> = OnceCell::new();

/// 全局存储 Java 层传递的扩展环境变量（TERMUX_APP__* 等）
pub static EXTENDED_ENV: OnceCell<Mutex<std::collections::HashMap<String, String>>> =
    OnceCell::new();

#[cfg(test)]
mod prefix_validation_tests {
    use super::*;

    /// 合法前缀应被原样接受。
    #[test]
    fn test_valid_prefix_accepted() {
        assert_eq!(
            validate_termux_prefix("/data/data/com.termux/files/usr"),
            "/data/data/com.termux/files/usr"
        );
    }

    /// 动态 /data/user/0/ 前缀必须被拒绝并回退到硬编码路径。
    /// 这是回归测试：防止有人再次把 Java 层的 setTermuxPrefix 改成
    /// getFilesDir().getAbsolutePath() 导致 termux-exec W^X bypass 失效，
    /// 从而引发全站 Permission denied（见 commit 5121e9e0）。
    #[test]
    fn test_user_0_prefix_rejected() {
        assert_eq!(
            validate_termux_prefix("/data/user/0/com.termux/files/usr"),
            "/data/data/com.termux/files/usr"
        );
    }

    /// 任意其他非法前缀也应回退。
    #[test]
    fn test_arbitrary_invalid_prefix_rejected() {
        assert_eq!(
            validate_termux_prefix("/sdcard/com.termux/files/usr"),
            "/data/data/com.termux/files/usr"
        );
    }

    /// 未初始化时 fallback 必须是标准硬编码路径。
    /// 注意：此测试依赖 TERMUX_PREFIX 未被其他测试写入；
    ///       若失败说明测试执行顺序或全局状态管理需要调整。
    #[test]
    fn test_get_termux_prefix_fallback() {
        // OnceCell 无法重置，因此仅在未初始化时断言。
        if TERMUX_PREFIX.get().is_none() {
            assert_eq!(get_termux_prefix(), "/data/data/com.termux/files/usr");
        }
    }
}
