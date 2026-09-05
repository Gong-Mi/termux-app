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
pub mod handles;
pub mod io_runtime;
pub mod key_event;
pub mod perform_handler;
pub mod sgr;
pub mod shared_buffer;
pub mod state;

// 重新导出主要类型
pub use context::{TerminalContext, TerminalEngine};
pub use events::TerminalEvent;
pub use shared_buffer::{FlatScreenBuffer, SharedBufferPtr, SharedScreenBuffer};
pub use state::ScreenState;

/// Java Long values are opaque tokens, never addresses. The registry also owns
/// the renderer binding so publication and revocation share one linearization.
pub static ENGINE_HANDLES: once_cell::sync::Lazy<handles::EngineHandles<TerminalContext>> =
    once_cell::sync::Lazy::new(handles::EngineHandles::new);

/// Revoke new JNI/render leases, then cancel IO and join off the caller thread.
/// An in-flight foreign callback can delay completion; cancellation is not drain.
pub fn destroy_engine(handle: i64) {
    let Some(context) = ENGINE_HANDLES.remove(handle) else {
        return;
    };
    crate::coordinator::discard_engine_data(handle);
    TerminalContext::stop_io(&context);
}
