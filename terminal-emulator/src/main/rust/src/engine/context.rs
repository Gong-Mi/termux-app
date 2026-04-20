/// 终端引擎和上下文管理
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::os::fd::FromRawFd;


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
    pub fn new(session_id: i32, cols: i32, rows: i32, total_rows: i32, cw: i32, ch: i32) -> Self {
        Self {
            parser: Parser::new(),
            state: ScreenState::new(session_id, cols, rows, total_rows, cw, ch),
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

    pub fn start_io_thread(context: std::sync::Arc<Self>, dup_fd: i32) {
        std::thread::spawn(move || {
            crate::utils::android_log(crate::utils::LogPriority::INFO, "CHECKPOINT: IO Thread STARTing [ARCH_REWRITE]");
            let mut buffer = [0u8; 8192];
            let mut pty_file = unsafe { std::fs::File::from_raw_fd(dup_fd) };

            let vm = match crate::JAVA_VM.get() {
                Some(v) => v,
                None => {
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, "IO Thread: JAVA_VM not initialized");
                    return;
                }
            };

            let mut env = match vm.attach_current_thread_as_daemon() {
                Ok(g) => g,
                Err(e) => {
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("IO Thread: Failed to attach: {:?}", e));
                    return;
                }
            };

            crate::utils::android_log(crate::utils::LogPriority::DEBUG, "IO Thread: Attached and running");

            while context.running.load(Ordering::Relaxed) {
                let read_res = std::io::Read::read(&mut pty_file, &mut buffer);
                match read_res {
                    Ok(0) => {
                        // 抗抖动逻辑：可能是 execvp 切换瞬间，等待 100ms
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        let retry_res = std::io::Read::read(&mut pty_file, &mut buffer);
                        if let Ok(0) = retry_res {
                            crate::utils::android_log(crate::utils::LogPriority::WARN, "[IO_THREAD] Permanent EOF from PTY. Exiting.");
                            break;
                        }
                        continue;
                    },
                    Ok(n) => {
                        let (events, pending_responses, callback_obj) = {
                            let mut engine = context.lock.write().unwrap();
                            engine.process_bytes(&buffer[..n]);
                            let resps = std::mem::replace(&mut engine.state.pending_responses, Vec::new());
                            let cb = engine.state.java_callback_obj.clone();
                            (engine.take_events(), resps, cb)
                        };

                        let current_pty_fd = context.pty_fd.load(Ordering::Relaxed);
                        for resp in pending_responses {
                            let r: String = resp;
                            if current_pty_fd != -1 {
                                unsafe { libc::write(current_pty_fd, r.as_ptr() as *const libc::c_void, r.len()); }
                            }
                        }

                        for event in &events {
                            match event {
                                crate::engine::events::TerminalEvent::ScreenUpdated => {
                                    crate::render_thread::request_render();
                                    // 必须通知 Java 层屏幕已更新，否则 ScrollBar 和选区不会刷新
                                    if let Some(obj) = &callback_obj {
                                        let _ = env.call_method(obj.as_obj(), "onScreenUpdated", "()V", &[]);
                                    }
                                }
                                _ => {}
                            }
                        }

                        if let Some(obj) = callback_obj as Option<jni::objects::GlobalRef> {
                            if !obj.as_obj().is_null() {
                                let _ = env.with_local_frame(16, |env: &mut jni::JNIEnv| -> Result<(), jni::errors::Error> {
                                    for event in events {
                                        match event {
                                            crate::engine::events::TerminalEvent::Bell => { let _ = env.call_method(obj.as_obj(), "onBell", "()V", &[]); }
                                            crate::engine::events::TerminalEvent::ColorsChanged => { let _ = env.call_method(obj.as_obj(), "onColorsChanged", "()V", &[]); }
                                            crate::engine::events::TerminalEvent::CopytoClipboard(text) => {
                                                if let Ok(j_text) = env.new_string(&text) {
                                                    let val = jni::objects::JValue::from(&j_text);
                                                    let _ = env.call_method(obj.as_obj(), "onCopyTextToClipboard", "(Ljava/lang/String;)V", &[val]);
                                                }
                                            }
                                            _ => {}
                                        }
                                        if env.exception_check().unwrap_or(false) { let _ = env.exception_clear(); }
                                    }
                                    Ok(())
                                });
                            }
                        }
                    },
                    Err(_) => break,
                }
            }
            crate::utils::android_log(crate::utils::LogPriority::INFO, "CHECKPOINT: IO Thread EXITing (normal) [ARCH_REWRITE]");
        });
    }
}
