/// 终端引擎和上下文管理
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::os::fd::FromRawFd;
use jni::objects::JValue;

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

impl Drop for TerminalEngine {
    fn drop(&mut self) {
        crate::utils::android_log(crate::utils::LogPriority::INFO, "TerminalEngine: Dropping engine and releasing resources");
    }
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
                    let _ = flat.sync_to_shared(self.state.shared_buffer_ptr.0);
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
                if let Ok(env) = env_res {
                    let mut env: jni::JNIEnv = env;
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
    pub pty_fd: std::sync::atomic::AtomicI32,
}

impl TerminalContext {
    pub fn new(engine: TerminalEngine) -> Self {
        Self {
            lock: RwLock::new(engine),
            running: AtomicBool::new(true),
            pty_fd: std::sync::atomic::AtomicI32::new(-1),
        }
    }

    pub fn start_io_thread(self: std::sync::Arc<Self>, pty_fd: i32) {
        self.pty_fd.store(pty_fd, Ordering::SeqCst);
        let context = self.clone();
        let dup_fd = unsafe { libc::dup(pty_fd) };
        if dup_fd < 0 {
            crate::utils::android_log(crate::utils::LogPriority::ERROR, "IO Thread: dup failed");
            return;
        }
        std::thread::spawn(move || {
            crate::utils::android_log(crate::utils::LogPriority::INFO, "CHECKPOINT: IO Thread STARTing");
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
            let env = &mut *guard;

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
                                if context.running.load(Ordering::Relaxed) {
                                    for event in events {
                                        if obj.as_obj().is_null() { break; }

                                        match event {
                                            TerminalEvent::ScreenUpdated => { let _ = env.call_method(obj.as_obj(), "onScreenUpdated", "()V", &[]); }
                                            TerminalEvent::Bell => { let _ = env.call_method(obj.as_obj(), "onBell", "()V", &[]); }
                                            TerminalEvent::ColorsChanged => { let _ = env.call_method(obj.as_obj(), "onColorsChanged", "()V", &[]); }
                                            TerminalEvent::CopytoClipboard(text) => {
                                                if let Ok(j_text) = env.new_string(text) {
                                                    let val = JValue::from(&j_text);
                                                    let _ = env.call_method(obj.as_obj(), "onCopyTextToClipboard", "(Ljava/lang/String;)V", &[val]);
                                                }
                                            }
                                            TerminalEvent::TitleChanged(title) => {
                                                if let Ok(j_title) = env.new_string(title) {
                                                    let val = JValue::from(&j_title);
                                                    let _ = env.call_method(obj.as_obj(), "reportTitleChange", "(Ljava/lang/String;)V", &[val]);
                                                }
                                            }
                                            TerminalEvent::TerminalResponse(resp) => {
                                                if let Ok(j_resp) = env.new_string(resp) {
                                                    let val = JValue::from(&j_resp);
                                                    let _ = env.call_method(obj.as_obj(), "write", "(Ljava/lang/String;)V", &[val]);
                                                }
                                            }
                                            TerminalEvent::SixelImage { rgba_data, width, height, start_x, start_y } => {
                                                if let Ok(j_data) = env.new_byte_array(rgba_data.len() as i32) {
                                                    let bytes: Vec<i8> = rgba_data.iter().map(|&b| b as i8).collect();
                                                    let _ = env.set_byte_array_region(&j_data, 0, &bytes);
                                                    let args = [
                                                        JValue::from(&j_data),
                                                        JValue::Int(width),
                                                        JValue::Int(height),
                                                        JValue::Int(start_x),
                                                        JValue::Int(start_y)
                                                    ];
                                                    let _ = env.call_method(obj.as_obj(), "onSixelImage", "([BIIII)V", &args);
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
            crate::utils::android_log(crate::utils::LogPriority::INFO, "CHECKPOINT: IO Thread EXITing (normal)");
        });
    }
}
