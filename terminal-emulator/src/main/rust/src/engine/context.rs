/// 终端引擎和上下文管理
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::os::fd::FromRawFd;
use std::io::Read;

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
    pub fn new(cols: i32, rows: i32, total_rows: i32, cw: i32, ch: i32) -> Self {
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

/// 终端上下文 - 线程安全的引擎包装
pub struct TerminalContext {
    pub lock: RwLock<TerminalEngine>,
    pub running: AtomicBool,
}

impl TerminalContext {
    pub fn new(engine: TerminalEngine) -> Self {
        Self {
            lock: RwLock::new(engine),
            running: AtomicBool::new(true),
        }
    }

    pub fn start_io_thread(self: std::sync::Arc<Self>, pty_fd: i32) {
        let context = self.clone();
        // 关键修复：dup FD，避免与 Java 侧的 FileOutputStream 争夺同一个 FD
        // Java 也使用相同的 pty_fd 进行写入，如果 Rust 侧通过 from_raw_fd 拥有它，
        // 当 File drop 时会关闭 FD，导致 Java 的写入失败
        let dup_fd = unsafe { libc::dup(pty_fd) };
        if dup_fd < 0 {
            crate::utils::android_log(crate::utils::LogPriority::ERROR, "IO Thread: dup failed");
            return;
        }
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            let mut pty_file = unsafe { std::fs::File::from_raw_fd(dup_fd) };

            let vm = match crate::JAVA_VM.get() {
                Some(v) => v,
                None => {
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, "IO Thread: JAVA_VM not initialized");
                    return;
                }
            };

            let mut guard = match vm.attach_current_thread() {
                Ok(g) => g,
                Err(e) => {
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("IO Thread: Failed to attach to JVM: {:?}", e));
                    return;
                }
            };

            crate::utils::android_log(crate::utils::LogPriority::DEBUG, "IO Thread: Attached and running");

            while context.running.load(Ordering::Relaxed) {
                match std::io::Read::read(&mut pty_file, &mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let (events, callback_obj) = {
                            let mut engine = context.lock.write().unwrap();
                            engine.process_bytes(&buffer[..n]);
                            (engine.take_events(), engine.state.java_callback_obj.clone())
                        };

                        if !events.is_empty() {
                            if let Some(obj) = &callback_obj {
                                let env = &mut *guard;
                                if context.running.load(Ordering::Relaxed) {
                                    for event in events {
                                        if obj.as_obj().is_null() { break; }

                                        match event {
                                            TerminalEvent::ScreenUpdated => { let _ = env.call_method(obj.as_obj(), "onScreenUpdated", "()V", &[]); }
                                            TerminalEvent::Bell => { let _ = env.call_method(obj.as_obj(), "onBell", "()V", &[]); }
                                            TerminalEvent::ColorsChanged => { let _ = env.call_method(obj.as_obj(), "onColorsChanged", "()V", &[]); }
                                            TerminalEvent::CopytoClipboard(text) => {
                                                if let Ok(j_text) = env.new_string(text) {
                                                    let _ = env.call_method(obj.as_obj(), "onCopyTextToClipboard", "(Ljava/lang/String;)V", &[(&j_text).into()]);
                                                }
                                            }
                                            TerminalEvent::TitleChanged(title) => {
                                                if let Ok(j_title) = env.new_string(title) {
                                                    let _ = env.call_method(obj.as_obj(), "reportTitleChange", "(Ljava/lang/String;)V", &[(&j_title).into()]);
                                                }
                                            }
                                            TerminalEvent::TerminalResponse(resp) => {
                                                if let Ok(j_resp) = env.new_string(resp) {
                                                    let _ = env.call_method(obj.as_obj(), "write", "(Ljava/lang/String;)V", &[(&j_resp).into()]);
                                                }
                                            }
                                            TerminalEvent::SixelImage { rgba_data, width, height, start_x, start_y } => {
                                                if let Ok(j_data) = env.new_byte_array(rgba_data.len() as i32) {
                                                    unsafe { let _ = env.set_byte_array_region(&j_data, 0, std::mem::transmute::<&[u8], &[i8]>(&rgba_data)); }
                                                    let _ = env.call_method(obj.as_obj(), "onSixelImage", "([BIIII)V", &[
                                                        (&j_data).into(), width.into(), height.into(), start_x.into(), start_y.into()
                                                    ]);
                                                }
                                            }
                                        }

                                        if env.exception_check().unwrap_or(false) {
                                            let _ = env.exception_describe();
                                            let _ = env.exception_clear();
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            crate::utils::android_log(crate::utils::LogPriority::DEBUG, "IO Thread: Exiting");
        });
    }
}
