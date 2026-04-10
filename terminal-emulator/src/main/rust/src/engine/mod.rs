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
pub mod shared_buffer;
pub mod state;
pub mod context;
pub mod perform_handler;
pub mod sgr;
pub mod decset;
pub mod key_event;

// 重新导出主要类型
pub use events::TerminalEvent;
pub use shared_buffer::{SharedBufferPtr, SharedScreenBuffer, FlatScreenBuffer};
pub use state::ScreenState;
pub use context::{TerminalEngine, TerminalContext};
