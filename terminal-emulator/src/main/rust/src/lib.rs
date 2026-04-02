use jni::JNIEnv;
use jni::objects::{JClass, JString, JObject};
use jni::sys::{jint, jlong, jbyteArray, jboolean, jintArray, jstring};
use once_cell::sync::OnceCell;
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::sync::atomic::Ordering;
use libc;

// 声明子模块
pub mod terminal;
pub mod utils;
pub mod engine;
pub mod bootstrap;
pub mod fastpath;
pub mod pty;
pub mod vte_parser;
pub mod coordinator;

pub use crate::engine::{TerminalEngine, TerminalContext, TerminalEvent};
pub use crate::coordinator::{SessionCoordinator, SessionState};
pub use crate::terminal::style::*;
pub use crate::terminal::modes::*;
pub use crate::terminal::colors::*;
pub use crate::terminal::sixel::{SixelDecoder, SixelState, SixelColor};
use crate::utils::{android_log, LogPriority};

pub static JAVA_VM: OnceCell<jni::JavaVM> = OnceCell::new();

/// 核心修复：在不持有锁的情况下将事件刷新到 Java，彻底杜绝双向死锁
fn flush_events_to_java(env: &mut JNIEnv, callback_obj: &Option<jni::objects::GlobalRef>, events: Vec<TerminalEvent>) {
    if events.is_empty() { return; }
    let obj = match callback_obj {
        Some(o) => o.as_obj(),
        None => return,
    };

    for event in events {
        match event {
            TerminalEvent::ScreenUpdated => {
                let _ = env.call_method(obj, "onScreenUpdated", "()V", &[]);
            }
            TerminalEvent::Bell => {
                let _ = env.call_method(obj, "onBell", "()V", &[]);
            }
            TerminalEvent::ColorsChanged => {
                let _ = env.call_method(obj, "onColorsChanged", "()V", &[]);
            }
            TerminalEvent::TitleChanged(title) => {
                if let Ok(j_title) = env.new_string(title) {
                    let _ = env.call_method(obj, "reportTitleChange", "(Ljava/lang/String;)V", &[(&j_title).into()]);
                }
            }
            TerminalEvent::TerminalResponse(resp) => {
                if let Ok(j_resp) = env.new_string(resp) {
                    let _ = env.call_method(obj, "write", "(Ljava/lang/String;)V", &[(&j_resp).into()]);
                }
            }
            TerminalEvent::SixelImage { rgba_data, width, height, start_x, start_y } => {
                if let Ok(j_data) = env.new_byte_array(rgba_data.len() as i32) {
                    unsafe {
                        let _ = env.set_byte_array_region(&j_data, 0, std::mem::transmute::<&[u8], &[i8]>(&rgba_data));
                    }
                    let _ = env.call_method(obj, "onSixelImage", "([BIIII)V", &[
                        (&j_data).into(),
                        width.into(),
                        height.into(),
                        start_x.into(),
                        start_y.into(),
                    ]);
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: std::ffi::c_void) -> jint {
    let _ = JAVA_VM.set(vm);
    android_log(LogPriority::INFO, "JNI_OnLoad: Termux-Rust library loaded");
    jni::sys::JNI_VERSION_1_6
}

// ============================================================================
// TerminalEmulator JNI 接口
// ============================================================================

use std::sync::Arc;

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_createEngineRustWithCallback(
    env: JNIEnv,
    _class: JClass,
    cols: jint,
    rows: jint,
    cw: jint,
    ch: jint,
    total_rows: jint,
    callback: JObject,
) -> jlong {
    android_log(LogPriority::DEBUG, &format!("JNI: createEngineRustWithCallback ({}x{})", cols, rows));
    let mut engine = TerminalEngine::new(cols, rows, total_rows, cw, ch);
    if !callback.is_null() {
        if let Ok(global_ref) = env.new_global_ref(callback) {
            engine.state.java_callback_obj = Some(global_ref);
        }
    }
    let context = Arc::new(TerminalContext::new(engine));
    Arc::into_raw(context) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeProcess(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
    _callback: JObject,
) {
    if ptr == 0 || data.is_null() { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
            if let Ok(bytes) = env.convert_byte_array(&j_array) {
                engine.process_bytes(&bytes);
            }
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_processBatchRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    batch: jbyteArray,
    length: jint,
) {
    if ptr == 0 || batch.is_null() { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            let j_array = unsafe { jni::objects::JByteArray::from_raw(batch) };
            if let Ok(bytes) = env.convert_byte_array(&j_array) {
                let len = length as usize;
                let actual_len = std::cmp::min(len, bytes.len());
                engine.process_bytes(&bytes[..actual_len]);
            }
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeResize(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cols: jint,
    rows: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.resize(cols, rows);
        engine.events.push(TerminalEvent::ScreenUpdated);
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_processCodePointRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    code_point: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            engine.process_code_point(code_point as u32);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resizeEngineRustFull(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cols: jint,
    rows: jint,
    _cw: jint,
    _ch: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.resize(cols, rows);
        engine.events.push(TerminalEvent::ScreenUpdated);
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
        context.running.store(false, Ordering::SeqCst);
        // Arc will be dropped here as we don't call Arc::into_raw(context)
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeStartIoThread(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    fd: jint,
) {
    if ptr == 0 { return; }
    
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let context_thread = Arc::clone(&context);
    
    // 克隆 PTY FD 以便在独立线程中使用
    let pty_fd = unsafe { libc::dup(fd) };
    if pty_fd < 0 { 
        let _ = Arc::into_raw(context);
        return; 
    }

    std::thread::Builder::new()
        .name("RustEngine".to_string())
        .spawn(move || {
            let context = context_thread;
            // 设置线程名称（Android 限制 16 字符，包括 null 终止符）
            let thread_name = std::ffi::CString::new("RustEngine").unwrap();
            unsafe {
                // 方法 1: prctl (Linux 标准)
                libc::prctl(libc::PR_SET_NAME, thread_name.as_ptr(), 0, 0, 0);
                // 方法 2: pthread_setname_np (Android 更可靠)
                libc::pthread_setname_np(libc::pthread_self(), thread_name.as_ptr());
            }

            android_log(LogPriority::INFO, "Rust IO Thread started (RustEngine)");
            
            // 必须附加当前线程到 JVM，否则无法通过 JNI 调用 Java 方法
            let mut attached_env = crate::JAVA_VM.get().and_then(|vm| {
                vm.attach_current_thread_as_daemon().ok()
            });

            let mut file = unsafe { std::fs::File::from_raw_fd(pty_fd) };
            let mut buffer = [0u8; 8192];
            
            // 获取 context 的原始指针进行长时间持有
            while context.running.load(Ordering::SeqCst) {
                match file.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let (events, cb) = {
                            let mut engine = context.lock.write().unwrap();
                            engine.process_bytes(&buffer[..n]);
                            (engine.take_events(), engine.state.java_callback_obj.clone())
                        };
                        if let Some(ref mut env) = attached_env {
                            flush_events_to_java(env, &cb, events);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            android_log(LogPriority::INFO, "Rust IO Thread stopped");
        })
        .expect("Failed to spawn Rust IO thread");
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTitleFromRust(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let title = {
        let engine = context.lock.read().unwrap();
        engine.state.title.clone().unwrap_or_default()
    };
    let result = if let Ok(j_str) = env.new_string(title) { j_str.into_raw() } else { std::ptr::null_mut() };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorRowFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.y as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorColFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.x as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorStyleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.style as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorStyleFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong, cursor_style: jint) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.style = cursor_style as i32;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_doDecSetOrResetFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong, setting: jboolean, mode: jint) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            engine.state.do_decset_or_reset(setting != 0, mode as u32);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_shouldCursorBeVisibleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.cursor.should_be_visible(engine.state.cursor_enabled) { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorEnabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.cursor_enabled { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isReverseVideoFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO) { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAlternateBufferActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.use_alternate_buffer { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorKeysApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.application_cursor_keys { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isKeypadApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.modes.is_enabled(DECSET_BIT_APPLICATION_KEYPAD) { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isMouseTrackingActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.mouse_tracking { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isInsertModeActiveFromRust(_env: JNIEnv, _class: JClass, _ptr: jlong) -> jboolean { 0 }

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.scroll_counter as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.rows as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cols as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, row: jint, text: jni::sys::jintArray, styles: jni::sys::jlongArray,
) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (text_buf, style_buf) = {
        let engine = context.lock.read().unwrap();
        let cols = engine.state.cols as usize;
        let mut text_buf = vec![0i32; cols];
        let mut style_buf = vec![0i64; cols];
        engine.state.copy_row_codepoints(row, &mut text_buf);
        engine.state.copy_row_styles_i64(row, &mut style_buf);
        (text_buf, style_buf)
    };

    unsafe {
        let j_text = jni::objects::JIntArray::from_raw(text);
        let j_styles = jni::objects::JLongArray::from_raw(styles);
        let _ = env.set_int_array_region(&j_text, 0, &text_buf);
        let _ = env.set_long_array_region(&j_styles, 0, &style_buf);
    }
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getSelectedTextFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, x1: jint, y1: jint, x2: jint, y2: jint,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let text = {
        let engine = context.lock.read().unwrap();
        engine.state.get_current_screen().get_selected_text(x1, y1, x2, y2)
    };
    let result = if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getWordAtLocationFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, x: jint, y: jint,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let text = {
        let engine = context.lock.read().unwrap();
        engine.state.get_current_screen().get_row(y).get_word_at(x as usize)
    };
    let result = if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTranscriptTextFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let text = {
        let engine = context.lock.read().unwrap();
        engine.state.get_current_screen().get_transcript_text()
    };
    let result = if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearScrollCounterFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.scroll_counter = 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        if engine.state.auto_scroll_disabled { 1 } else { 0 }
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_toggleAutoScrollDisabledFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.auto_scroll_disabled = !engine.state.auto_scroll_disabled;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendMouseEventFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong, button: jint, col: jint, row: jint, pressed: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.send_mouse_event(button as u32, col, row, pressed != 0);
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendKeyCodeFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong, key_code: jint, char_str: jstring, meta_state: jint,
) {
    if ptr == 0 { return; }
    let rust_str = if !char_str.is_null() {
        let j_str = unsafe { JString::from_raw(char_str) };
        env.get_string(&j_str).ok().map(|s| String::from(s)).unwrap_or_default()
    } else { String::new() };
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        // send_key_event 现在在 TerminalEngine 上实现
        engine.send_key_event(key_code, Some(rust_str), meta_state);
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_pasteTextFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong, text: jstring,
) {
    if ptr == 0 { return; }
    let rust_str = if !text.is_null() {
        let j_str = unsafe { JString::from_raw(text) };
        env.get_string(&j_str).ok().map(|s| String::from(s))
    } else { None };

    if let Some(s) = rust_str {
        let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            engine.state.paste(&s);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
        let _ = Arc::into_raw(context);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getActiveTranscriptRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.get_current_screen().active_transcript_rows as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColorsFromRust(env: JNIEnv, _class: JClass, ptr: jlong) -> jintArray {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let colors = {
        let engine = context.lock.read().unwrap();
        engine.state.colors.current_colors
    };

    let result = if let Ok(j_array) = env.new_int_array(colors.len() as jint) {
        unsafe { let _ = env.set_int_array_region(&j_array, 0, std::mem::transmute::<&[u32], &[i32]>(&colors)); }
        j_array.into_raw()
    } else { std::ptr::null_mut() };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    
    // 修复：在锁外回调，避免死锁
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.colors.reset();
        // 不再调用 report_colors_changed()，而是手动添加事件
        // 这样事件会在锁释放后通过 flush_events_to_java 处理
        let mut events = engine.take_events();
        events.push(crate::engine::TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    }; // 锁在此处释放
    
    // 在锁外安全回调 Java
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_updateTerminalSessionClientFromRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    client: JObject,
) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        if client.is_null() {
            engine.state.java_callback_obj = None;
        } else {
            if let Ok(global_ref) = env.new_global_ref(client) {
                engine.state.java_callback_obj = Some(global_ref);
            }
        }
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkStateInRust(mut env: JNIEnv, _class: JClass, ptr: jlong, state: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.blink_state = state != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkingEnabledInRust(mut env: JNIEnv, _class: JClass, ptr: jlong, enabled: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.blinking_enabled = enabled != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr != 0 { 
        let _context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
        // Arc will be dropped here
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorCol(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.x as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorRow(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.y as jint
    };
    let _ = Arc::into_raw(context);
    result
}

// ============================================================================
// TerminalEmulator.java - 调试辅助方法
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getDebugInfoFromRust(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jstring {
    if ptr == 0 { 
        let empty = env.new_string("TerminalEmulator[destroyed]").ok();
        return empty.map_or(std::ptr::null_mut(), |s| s.into_raw());
    }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let debug_info = {
        let engine = context.lock.read().unwrap();
        engine.state.get_debug_info()
    };
    let result = if let Ok(j_str) = env.new_string(debug_info) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    };
    let _ = Arc::into_raw(context);
    result
}

// ============================================================================
// JNI.java - PTY 处理
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_setPtyWindowSize(
    _env: JNIEnv,
    _class: JClass,
    fd: jint,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
) {
    crate::pty::set_pty_window_size(fd, rows, cols, cw, ch);
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_createSessionAsync(
    mut env: JNIEnv,
    _class: JClass,
    cmd: jstring,
    cwd: jstring,
    args: jni::sys::jobjectArray,
    env_vars: jni::sys::jobjectArray,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
    transcript_rows: jint,
    callback: JObject,
) {
    let cmd_str = if !cmd.is_null() {
        let js = unsafe { JString::from_raw(cmd) };
        env.get_string(&js).map(|s| s.into()).unwrap_or_default()
    } else {
        String::new()
    };

    let cwd_str = if !cwd.is_null() {
        let js = unsafe { JString::from_raw(cwd) };
        env.get_string(&js).map(|s| s.into()).unwrap_or_default()
    } else {
        String::new()
    };

    let mut argv = Vec::new();
    let args_obj = unsafe { jni::objects::JObjectArray::from_raw(args) };
    if !args_obj.is_null() {
        if let Ok(len) = env.get_array_length(&args_obj) {
            for i in 0..len {
                if let Ok(arg_obj) = env.get_object_array_element(&args_obj, i) {
                    let arg_java: JString = arg_obj.into();
                    if let Ok(s) = env.get_string(&arg_java) {
                        argv.push(String::from(s));
                    }
                }
            }
        }
    }

    let mut envp = Vec::new();
    let env_vars_obj = unsafe { jni::objects::JObjectArray::from_raw(env_vars) };
    if !env_vars_obj.is_null() {
        if let Ok(len) = env.get_array_length(&env_vars_obj) {
            for i in 0..len {
                if let Ok(env_obj) = env.get_object_array_element(&env_vars_obj, i) {
                    let env_java: JString = env_obj.into();
                    if let Ok(s) = env.get_string(&env_java) {
                        envp.push(String::from(s));
                    }
                }
            }
        }
    }

    let callback_ref = if !callback.is_null() {
        env.new_global_ref(callback).ok()
    } else {
        None
    };

    std::thread::spawn(move || {
        // 注册 session 到协调器
        let coordinator = SessionCoordinator::get();
        let session_id = coordinator.register_session();
        
        android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.1. Background thread started in Rust");
        android_log(LogPriority::INFO, &format!("[TRACE_SESSION] Session ID: {}", session_id));

        // 1. 创建子进程
        let pty_res = crate::pty::create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, cols, cw, ch);
        let (pty_fd, pid) = match pty_res {
            Ok(res) => {
                android_log(LogPriority::DEBUG, &format!("[TRACE_SESSION] 5.2. PTY created (fd={}, pid={})", res.0, res.1));
                res
            },
            Err(_) => {
                android_log(LogPriority::ERROR, "[TRACE_SESSION] 5.2. ERROR: PTY creation failed");
                coordinator.unregister_session(session_id);
                return;
            }
        };

        // 2. 创建 Engine
        android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.3. Creating TerminalEngine");
        let mut engine = TerminalEngine::new(cols, rows, transcript_rows, cw, ch);
        if let Some(ref cb) = callback_ref {
            engine.state.java_callback_obj = Some(cb.clone());
        }
        
        let context = Arc::new(TerminalContext::new(engine));
        let context_ptr = Arc::into_raw(context.clone());

        // 3. 启动 IO 线程
        android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.4. Starting IO thread");
        context.start_io_thread(pty_fd);

        // 4. 回调 Java
        if let Some(ref cb) = callback_ref {
            android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.5. Attempting to callback Java");
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let _ = env.call_method(
                        cb.as_obj(),
                        "onEngineInitialized",
                        "(JII)V",
                        &[
                            jni::objects::JValue::Long(context_ptr as jlong),
                            jni::objects::JValue::Int(pty_fd),
                            jni::objects::JValue::Int(pid),
                        ],
                    );
                    android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.6. Java callback executed");
                }
            }
        }
        android_log(LogPriority::INFO, &format!("Async session creation complete (pid={}, engine={:p})", pid, context_ptr));
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_waitFor(
    _env: JNIEnv,
    _class: JClass,
    pid: jint,
) -> jint {
    crate::pty::wait_for(pid)
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_close(
    _env: JNIEnv,
    _class: JClass,
    fd: jint,
) {
    unsafe { libc::close(fd); }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_createSubprocess(
    env: JNIEnv,
    _class: JClass,
    cmd: jstring,
    cwd: jstring,
    args: jni::sys::jobjectArray,
    env_vars: jni::sys::jobjectArray,
    process_id_array: jintArray,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
) -> jint {
    unsafe {
        crate::pty::create_subprocess(
            env.get_native_interface(),
            cmd,
            cwd,
            args,
            env_vars,
            process_id_array,
            rows,
            cols,
            cw,
            ch,
        )
    }
}

// ============================================================================
// WcWidth.java - Unicode 字符宽度计算
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_WcWidth_widthRust(_env: JNIEnv, _class: JClass, ucs: jint) -> jint {
    crate::utils::get_char_width(ucs as u32) as jint
}

// ============================================================================
// KeyHandler.java - 键盘按键处理
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getKeyCode(
    env: JNIEnv,
    _class: JClass,
    key_code: jint,
    key_mode: jint,
    cursor_app: jboolean,
    keypad: jboolean,
) -> jstring {
    let result = crate::terminal::key_handler::get_code(
        key_code,
        key_mode as u32,
        cursor_app != 0,
        keypad != 0,
    );
    
    match result {
        Some(s) => env.new_string(s).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getKeyCodeFromTermcap(
    mut env: JNIEnv,
    _class: JClass,
    termcap: JString,
    cursor_app: jboolean,
    keypad: jboolean,
) -> jstring {
    let termcap_str: String = env.get_string(&termcap).unwrap().into();
    
    let result = crate::terminal::key_handler::get_code_from_termcap(
        &termcap_str,
        cursor_app != 0,
        keypad != 0,
    );
    
    match result {
        Some(s) => env.new_string(s).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    }
}
