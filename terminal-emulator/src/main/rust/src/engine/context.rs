use super::io_runtime::{IoRuntime, SubmitError};
/// 终端引擎和上下文管理
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

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

impl Drop for TerminalEngine {
    fn drop(&mut self) {
        crate::utils::android_log(
            crate::utils::LogPriority::INFO,
            "TerminalEngine: Dropping engine and releasing resources",
        );
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
        let mut handler = PerformHandler {
            state: &mut self.state,
            events: &mut self.events,
        };
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
                let env_res = vm
                    .get_env()
                    .or_else(|_| vm.attach_current_thread_as_daemon());
                if let Ok(env) = env_res {
                    let mut env: jni::JNIEnv = env;
                    let _ = env.call_method(obj.as_obj(), "onScreenUpdated", "()V", &[]);
                }
            }
        }
    }
}

/// Terminal memory and IO have separate lifetimes. The IO callback holds only a
/// Weak reference; retaining a render lease does not keep a cancelled reader alive.
pub struct TerminalContext {
    pub lock: RwLock<TerminalEngine>,
    pub running: AtomicBool,
    io: Mutex<Option<IoRuntime>>,
    io_joined: AtomicBool,
}

/// Input budget is a policy limit, not a guarantee of eventual delivery.
pub const INPUT_CAPACITY: usize = 1024 * 1024;

impl TerminalContext {
    pub fn new(engine: TerminalEngine) -> Self {
        Self {
            lock: RwLock::new(engine),
            running: AtomicBool::new(true),
            io: Mutex::new(None),
            io_joined: AtomicBool::new(true),
        }
    }

    /// Transfer an owned descriptor exactly once. Repeated start is rejected;
    /// rejected descriptors are dropped, not leaked or allowed a second reader.
    pub fn start_io_owned(context: Arc<Self>, fd: OwnedFd) -> std::io::Result<()> {
        let mut slot = context.io.lock().unwrap_or_else(|e| e.into_inner());
        if !context.running.load(Ordering::Acquire) || slot.is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "IO already started or context revoked",
            ));
        }
        let weak = Arc::downgrade(&context);
        // A worker-local handoff between parse and notification phases. Never
        // hold this lock while calling Java; replies enter FIFO before reentry.
        let notifications = Arc::new(Mutex::new(None));
        let publish = Arc::clone(&notifications);
        let runtime = IoRuntime::start_with_callbacks(
            fd,
            INPUT_CAPACITY,
            move |bytes| {
                let Some(context) = weak.upgrade() else {
                    return Vec::new();
                };
                let (events, responses, callback) = {
                    let mut engine = context.lock.write().unwrap_or_else(|e| e.into_inner());
                    engine.process_bytes(bytes);
                    let responses = std::mem::take(&mut engine.state.pending_responses);
                    (
                        engine.take_events(),
                        responses,
                        engine.state.java_callback_obj.clone(),
                    )
                };
                *publish.lock().unwrap_or_else(|e| e.into_inner()) = Some((events, callback));
                responses.into_iter().map(String::into_bytes).collect()
            },
            move || {
                let pending = notifications
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
                if let Some((events, callback)) = pending {
                    Self::dispatch_io_events(events, callback);
                }
            },
            |outcome| {
                crate::utils::android_log(
                    crate::utils::LogPriority::INFO,
                    &format!("PTY_IO_OUTCOME: {outcome:?}; accepted output may remain undelivered"),
                );
            },
        )?;
        context.io_joined.store(false, Ordering::Release);
        *slot = Some(runtime);
        Ok(())
    }

    pub fn submit_input(&self, bytes: &[u8]) -> Result<(), SubmitError> {
        let slot = self.io.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some(runtime) if self.running.load(Ordering::Acquire) => runtime.submit(bytes),
            _ => Err(SubmitError::Closed),
        }
    }

    pub fn resize_pty(&self, rows: i32, cols: i32, cw: i32, ch: i32) {
        let slot = self.io.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(runtime) = slot.as_ref() {
            let _ = runtime.resize(
                rows as u16,
                cols as u16,
                cols.wrapping_mul(cw) as u16,
                rows.wrapping_mul(ch) as u16,
            );
        }
    }

    /// Revoke admission and request cancellation without joining on the caller.
    /// The reaper retains the context until the worker actually terminates.
    /// A foreign callback may delay join; cancellation is not a bounded callback.
    pub fn stop_io(context: &Arc<Self>) {
        let runtime = {
            let mut slot = context.io.lock().unwrap_or_else(|e| e.into_inner());
            context.running.store(false, Ordering::Release);
            let runtime = slot.take();
            if let Some(runtime) = runtime.as_ref() {
                runtime.cancel();
            }
            runtime
        };
        if let Some(mut runtime) = runtime {
            let context = Arc::clone(context);
            if let Err(error) = std::thread::Builder::new()
                .name("pty-io-reaper".into())
                .spawn(move || {
                    let outcome = runtime.join();
                    crate::utils::android_log(
                        crate::utils::LogPriority::INFO,
                        &format!("PTY_IO_STOPPED: {outcome:?}; cancellation is not full drain"),
                    );
                    context.io_joined.store(true, Ordering::Release);
                })
            {
                // Spawn failure drops/cancels runtime. Do not claim join completed.
                crate::utils::android_log(
                    crate::utils::LogPriority::ERROR,
                    &format!("PTY IO reaper spawn failed: {error}"),
                );
            }
        }
    }

    /// True only after no worker was started or the background join completed.
    pub fn io_is_joined(&self) -> bool {
        self.io_joined.load(Ordering::Acquire)
    }

    fn dispatch_io_events(events: Vec<TerminalEvent>, callback: Option<jni::objects::GlobalRef>) {
        if events
            .iter()
            .any(|event| matches!(event, TerminalEvent::ScreenUpdated))
        {
            crate::render_thread::request_render();
        }
        let Some(obj) = callback else {
            return;
        };
        let Some(vm) = crate::JAVA_VM.get() else {
            return;
        };
        let Ok(mut env) = vm
            .get_env()
            .or_else(|_| vm.attach_current_thread_as_daemon())
        else {
            crate::utils::android_log(
                crate::utils::LogPriority::ERROR,
                "PTY IO callback attach failed",
            );
            return;
        };
        // Preserve the old reader's two-pass ordering: screen notifications
        // precede bell/color/clipboard delivery, regardless of parser event order.
        for event in &events {
            if matches!(event, TerminalEvent::ScreenUpdated) {
                let _ = env.call_method(obj.as_obj(), "onScreenUpdated", "()V", &[]);
            }
        }
        // Extending title/sixel/exit delivery belongs to separate event work.
        let _ = env.with_local_frame(
            16,
            |env: &mut jni::JNIEnv| -> Result<(), jni::errors::Error> {
                for event in events {
                    match event {
                        TerminalEvent::Bell => {
                            let _ = env.call_method(obj.as_obj(), "onBell", "()V", &[]);
                        }
                        TerminalEvent::ColorsChanged => {
                            let _ = env.call_method(obj.as_obj(), "onColorsChanged", "()V", &[]);
                        }
                        TerminalEvent::CopytoClipboard(text) => {
                            if let Ok(j_text) = env.new_string(&text) {
                                let val = jni::objects::JValue::from(&j_text);
                                let _ = env.call_method(
                                    obj.as_obj(),
                                    "onCopyTextToClipboard",
                                    "(Ljava/lang/String;)V",
                                    &[val],
                                );
                            }
                        }
                        _ => {}
                    }
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
                Ok(())
            },
        );
    }
}
