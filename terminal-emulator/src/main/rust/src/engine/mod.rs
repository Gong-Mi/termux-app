pub mod context;
pub mod decset;
/// 终端引擎模块
///
/// 包含终端模拟器核心逻辑：
/// - TerminalEngine: 主引擎结构体
/// - TerminalContext: 线程安全的引擎包装
/// - ScreenState: 屏幕状态管理
/// - PerformHandler: VTE Parser 回调实现
/// - 共享缓冲区管理
/// - 终端事件枚举
pub mod events;
pub mod key_event;
pub mod local_socket;
pub mod perform_handler;
pub mod sgr;
pub mod shared_buffer;
pub mod state;

// 重新导出主要类型
pub use context::{TerminalContext, TerminalEngine};
pub use events::TerminalEvent;
pub use shared_buffer::{FlatScreenBuffer, SharedBufferPtr, SharedScreenBuffer};
pub use state::ScreenState;
