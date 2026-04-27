/// 终端引擎和上下文管理
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::vte_parser::Parser;
use crate::engine::state::ScreenState;
use crate::engine::events::TerminalEvent;
use crate::engine::perform_handler::PerformHandler;

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
        let mut handler = PerformHandler { state: &mut self.state, events: &mut self.events };
        self.parser.advance(&mut handler, data);
        self.state.sync_screen_to_flat_buffer();
        if !self.state.shared_buffer_ptr.0.is_null() {
            unsafe {
                if let Some(flat) = &self.state.flat_buffer {
                    flat.sync_to_shared(self.state.shared_buffer_ptr.0);
                }
            }
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
                let env_res = vm.get_env().or_else(|_| vm.attach_current_thread_as_daemon());
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
}

impl TerminalContext {
    pub fn new(engine: TerminalEngine) -> Self {
        Self {
            lock: RwLock::new(engine),
            running: AtomicBool::new(true),
            blink: BlinkControl::new(),
        }
    }
}
