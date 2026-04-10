//! Termux Rust 终端模拟器库
//!
//! 模块化的终端模拟器实现，提供：
//! - VTE 兼容的终端状态管理
//! - 256色/真彩色支持
//! - Sixel 图像渲染
//! - Vulkan/Skia GPU 渲染
//! - JNI 接口供 Java 层调用

use once_cell::sync::OnceCell;

// 声明子模块
pub mod terminal;
pub mod utils;
pub mod engine;
pub mod bootstrap;
pub mod pty;
pub mod vte_parser;
pub mod coordinator;
pub mod renderer;
pub mod vulkan_context;
pub mod render_thread;
pub mod jni_bindings;

// 重新导出主要类型，保持向后兼容
pub use crate::engine::{TerminalEngine, TerminalContext, TerminalEvent};
pub use crate::coordinator::{SessionCoordinator, SessionState};
pub use crate::terminal::style::*;
pub use crate::terminal::modes::*;
pub use crate::terminal::colors::*;
pub use crate::terminal::sixel::{SixelDecoder, SixelState, SixelColor};

pub static JAVA_VM: OnceCell<jni::JavaVM> = OnceCell::new();
