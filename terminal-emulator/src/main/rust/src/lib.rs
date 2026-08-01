//! Termux Rust 终端模拟器库
//!
//! 模块化的终端模拟器实现，提供：
//! - VTE 兼容的终端状态管理
//! - 256色/真彩色支持
//! - Sixel 图像渲染
//! - Vulkan/Skia GPU 渲染
//! - JNI 接口供 Java 层调用

// CI lint triage (2026-08): this crate predates the `-D warnings` gates and
// accumulated style-level lint debt. Mechanical lints are allowed crate-wide
// so the gates stay meaningful for real issues; remove these progressively.
#![allow(clippy::collapsible_if)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::byte_char_slices)]
#![allow(clippy::new_without_default)]
#![allow(clippy::single_match)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::manual_range_patterns)]
#![allow(clippy::manual_pattern_char_comparison)]
#![allow(clippy::manual_strip)]
#![allow(clippy::len_zero)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::empty_line_after_doc_comments)]
#![allow(clippy::get_first)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::let_unit_value)]
#![allow(clippy::mem_replace_with_default)]
#![allow(clippy::result_unit_err)]
#![allow(clippy::manual_split_once)]
// Arc<skia_safe::FontMgr>: skia font managers are refcounted and the cache
// asserts thread safety via explicit unsafe impl Send/Sync in renderer.rs.
#![allow(clippy::arc_with_non_send_sync)]
#![allow(rustdoc::bare_urls)]
#![allow(rustdoc::invalid_html_tags)]

use once_cell::sync::OnceCell;

#[macro_export]
macro_rules! safe_write {
    ($lock:expr) => {
        match $lock.write() {
            Ok(g) => g,
            Err(p) => {
                $crate::utils::android_log($crate::utils::LogPriority::ERROR, "RwLock poisoned! Recovering...");
                p.into_inner()
            }
        }
    };
}

#[macro_export]
macro_rules! safe_read {
    ($lock:expr) => {
        match $lock.read() {
            Ok(g) => g,
            Err(p) => {
                $crate::utils::android_log($crate::utils::LogPriority::ERROR, "RwLock poisoned! Recovering...");
                p.into_inner()
            }
        }
    };
}

// 声明子模块
pub mod wcwidth;
pub mod terminal;
pub mod utils;
pub mod engine;
pub mod bootstrap;
pub mod pty;
pub mod vte_parser;
pub mod vte_sve;
pub mod coordinator;
pub mod renderer;
pub mod vulkan_context;
pub mod render_thread;
pub mod jni;

// 重新导出主要类型，保持向后兼容
pub use crate::engine::{TerminalEngine, TerminalContext, TerminalEvent};
pub use crate::coordinator::{SessionCoordinator, SessionState};
pub use crate::terminal::style::*;
pub use crate::terminal::modes::*;
pub use crate::terminal::colors::*;
pub use crate::terminal::sixel::{SixelDecoder, SixelState, SixelColor};

pub use ::jni::JavaVM;
pub static JAVA_VM: OnceCell<JavaVM> = OnceCell::new();


#[cfg(test)]
mod metrics_tests {
    use super::utils::METRICS;
    use std::time::Duration;

    #[test]
    fn test_performance_metrics_collection() {
        // 测试记录字节
        METRICS.record_bytes(1024 * 1024);
        // 测试记录渲染耗时
        METRICS.record_render(Duration::from_millis(16));
        
        // 验证不会崩溃
        METRICS.try_report();
        
        assert!(METRICS.total_bytes_processed.load(std::sync::atomic::Ordering::Relaxed) >= 0);
    }
}
