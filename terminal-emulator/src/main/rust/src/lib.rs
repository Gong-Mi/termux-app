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
