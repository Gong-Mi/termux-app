use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
/// 终端引擎和上下文管理
use std::sync::{Mutex, RwLock};

use crate::engine::events::TerminalEvent;
use crate::engine::perform_handler::PerformHandler;
use crate::engine::state::ScreenState;
use crate::vte_parser::Parser;

/// 终端引擎 - 主结构体
pub struct TerminalEngine {
    pub parser: Parser,
    pub state: ScreenState,
    pub events: Vec<TerminalEvent>,
}

impl TerminalEngine {
    pub fn new(cols: i64, rows: i64, total_rows: i64, cw: i32, ch: i32) -> Self {
        Self {
            parser: Parser::new(),
            state: ScreenState::new(cols, rows, total_rows, cw, ch),
            events: Vec::with_capacity(16),
        }
    }

    pub fn take_events(&mut self) -> Vec<TerminalEvent> {
        std::mem::replace(&mut self.events, Vec::with_capacity(16))
    }

    pub fn process_bytes(&mut self, data: &[u8]) {
        let prev = self.state.snapshot();
        {
            let mut handler = PerformHandler {
                state: &mut self.state,
                events: &mut self.events,
            };
            self.parser.advance(&mut handler, data);
        }

        // 【性能优化】仅同步一次，而不是在 parser 内部多次同步
        self.state.sync_screen_to_flat_buffer();

        // 收集待发送的事件
        if self
            .state
            .has_pending
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            for event in self.state.pending_events.lock().drain(..) {
                self.events.push(event);
            }
            self.state
                .has_pending
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }

        let curr = self.state.snapshot();
        let mut mask = 0u32;
        for i in 0..16 {
            if prev[i] != curr[i] {
                mask |= 1 << i;
            }
        }
        if mask != 0 {
            self.events
                .push(TerminalEvent::StateChanged { mask, values: curr });
        }
        self.events.push(TerminalEvent::ScreenUpdated);
    }

    pub fn process_code_point(&mut self, code_point: u32) {
        let mut utf8_buf = [0u8; 4];
        let utf8_str = char::from_u32(code_point)
            .unwrap_or('\u{FFFD}')
            .encode_utf8(&mut utf8_buf);
        self.process_bytes(utf8_str.as_bytes());
    }

    pub fn notify_screen_updated(&self) {
        if let Some(obj) = &self.state.java_callback_obj {
            if let Some(vm) = crate::JAVA_VM.get() {
                let env_res = vm
                    .get_env()
                    .or_else(|_| vm.attach_current_thread_as_daemon());
                if let Ok(mut env) = env_res {
                    let _ = env.call_method(obj.as_obj(), "onScreenUpdated", "()V", &[]);
                }
            }
        }
    }
}

/// 光标闪烁线程控制
pub struct BlinkControl {
    pub running: AtomicBool,
    pub rate_ms: AtomicU32,
    pub handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl BlinkControl {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            rate_ms: AtomicU32::new(0),
            handle: Mutex::new(None),
        }
    }

    /// 停止已有的闪烁线程并等待其结束
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

/// 终端上下文 - 线程安全的引擎包装
pub struct TerminalContext {
    pub lock: RwLock<TerminalEngine>,
    pub running: AtomicBool,
    pub blink: BlinkControl,
    /// IO 线程使用的 PTY fd（dup 后的），用于 destroyEngine 时唤醒阻塞的 read()
    pub pty_fd: AtomicI32,
}

impl TerminalContext {
    pub fn new(engine: TerminalEngine) -> Self {
        Self {
            lock: RwLock::new(engine),
            running: AtomicBool::new(true),
            blink: BlinkControl::new(),
            pty_fd: AtomicI32::new(-1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助函数：从事件列表中提取 TerminalResponse 的字符串内容
    fn extract_responses(events: &[TerminalEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| {
                if let TerminalEvent::TerminalResponse(resp) = e {
                    Some(resp.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// 回归测试：批量处理 OSC 11 + CSI 6n 时，响应顺序必须严格保持。
    ///
    /// 背景：Rust 版本使用区块串行模型（批量读取 PTY -> process_bytes ->
    /// 批量 flush）。如果 pending_events 的 drain 顺序或 VTE 解析器的
    /// 回调顺序有误，OSC 11 响应和 CSI 6n 响应可能乱序，导致 termenv
    /// 的 "双查询读取" 逻辑读到错误的结果，最终引发命令行泄漏。
    #[test]
    fn test_osc11_csi6n_response_order_bel() {
        let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

        // 模拟 gh/termenv 发送的两个查询，用 BEL 终止 OSC
        let input = b"\x1b]11;?\x07\x1b[6n";
        engine.process_bytes(input);
        let events = engine.take_events();
        let responses = extract_responses(&events);

        // 必须产生恰好两个响应
        assert_eq!(
            responses.len(),
            2,
            "Expected exactly 2 TerminalResponse events, got {}: {:?}",
            responses.len(),
            responses
        );

        // 第一个必须是 OSC 11 背景色响应（黑色 = 0000/0000/0000）
        assert_eq!(
            responses[0], "\x1b]11;rgb:0000/0000/0000\x07",
            "First response must be OSC 11 with matching BEL terminator"
        );

        // 第二个必须是 CSI 6n 光标位置响应（默认 1;1R）
        assert_eq!(
            responses[1], "\x1b[1;1R",
            "Second response must be CSI cursor position report"
        );
    }

    /// 同上，但 OSC 查询使用 ST（\x1b\\）终止。
    /// 验证终端响应复用与查询相同的 terminator（commit 7f46a5e2 修复的内容）。
    #[test]
    fn test_osc11_csi6n_response_order_st() {
        let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

        // 模拟 gh/termenv 发送的两个查询，用 ST 终止 OSC
        let input = b"\x1b]11;?\x1b\\\x1b[6n";
        engine.process_bytes(input);
        let events = engine.take_events();
        let responses = extract_responses(&events);

        assert_eq!(
            responses.len(),
            2,
            "Expected exactly 2 TerminalResponse events, got {}: {:?}",
            responses.len(),
            responses
        );

        // 第一个必须是 OSC 11 响应，且 terminator 必须是 ST（\x1b\\），
        // 不能错误地回退到 BEL。
        assert_eq!(
            responses[0], "\x1b]11;rgb:0000/0000/0000\x1b\\",
            "First response must be OSC 11 with matching ST terminator"
        );

        assert_eq!(
            responses[1], "\x1b[1;1R",
            "Second response must be CSI cursor position report"
        );
    }

    /// 验证大区块混合输入下，多个查询的响应顺序仍然正确。
    /// 这模拟 IO 线程 FIONREAD 批量排空后的真实场景。
    #[test]
    fn test_bulk_mixed_queries_response_order() {
        let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

        // 构造一个包含普通文本 + OSC 10 + OSC 11 + CSI 6n + 更多文本的批量输入
        let mut input = Vec::new();
        input.extend_from_slice(b"hello ");
        input.extend_from_slice(b"\x1b]10;?\x07"); // 查询前景色
        input.extend_from_slice(b" world ");
        input.extend_from_slice(b"\x1b]11;?\x1b\\"); // 查询背景色（ST）
        input.extend_from_slice(b"\x1b[6n"); // 查询光标位置
        input.extend_from_slice(b" end");

        engine.process_bytes(&input);
        let events = engine.take_events();
        let responses = extract_responses(&events);

        assert_eq!(
            responses.len(),
            3,
            "Expected 3 TerminalResponse events, got {}: {:?}",
            responses.len(),
            responses
        );

        // 顺序必须严格对应输入顺序
        assert_eq!(
            responses[0], "\x1b]10;rgb:ffff/ffff/ffff\x07",
            "OSC 10 foreground"
        );
        assert_eq!(
            responses[1], "\x1b]11;rgb:0000/0000/0000\x1b\\",
            "OSC 11 background"
        );
        // 光标位置："hello "(6) + " world "(7) = 13（0-based），回复 1-based 即 1;14R
        assert_eq!(responses[2], "\x1b[1;14R", "CSI 6n cursor");
    }
}
