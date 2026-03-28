use jni::JNIEnv;
use jni::objects::{JClass, JString, JObject};
use jni::sys::{jint, jlong, jbyteArray, jboolean, jintArray, jstring};
use once_cell::sync::OnceCell;
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::sync::atomic::Ordering;

// 声明子模块
pub mod terminal;
pub mod utils;
pub mod engine;
pub mod bootstrap;
pub mod fastpath;
pub mod pty;
pub mod vte_parser;

pub use crate::engine::{TerminalEngine, TerminalContext};
pub use crate::terminal::style::*;
pub use crate::terminal::modes::*;
pub use crate::terminal::colors::*;
pub use crate::terminal::sixel::{SixelDecoder, SixelState, SixelColor};
use crate::utils::{android_log, LogPriority};

pub static JAVA_VM: OnceCell<jni::JavaVM> = OnceCell::new();

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: std::ffi::c_void) -> jint {
    let _ = JAVA_VM.set(vm);
    android_log(LogPriority::INFO, "JNI_OnLoad: Termux-Rust library loaded");
    jni::sys::JNI_VERSION_1_6
}

// ============================================================================
// TerminalEmulator JNI 接口
// ============================================================================

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
    if let Ok(global_ref) = env.new_global_ref(callback) {
        engine.state.java_callback_obj = Some(global_ref);
    }
    let context = Box::new(TerminalContext::new(engine));
    Box::into_raw(context) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeProcess(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
    _callback: JObject,
) {
    if ptr == 0 || data.is_null() { return; }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &*(ptr as *const TerminalContext) };
        let mut engine = context.lock.write().unwrap();
        let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
        if let Ok(bytes) = env.convert_byte_array(&j_array) {
            engine.process_bytes(&bytes);
        }
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "nativeProcess: panic caught");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_processBatchRust(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    batch: jbyteArray,
    length: jint,
) {
    if ptr == 0 || batch.is_null() { return; }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &*(ptr as *const TerminalContext) };
        let mut engine = context.lock.write().unwrap();
        let j_array = unsafe { jni::objects::JByteArray::from_raw(batch) };
        if let Ok(bytes) = env.convert_byte_array(&j_array) {
            let len = length as usize;
            let actual_len = std::cmp::min(len, bytes.len());
            engine.process_bytes(&bytes[..actual_len]);
        }
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "processBatchRust: panic caught");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeResize(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cols: jint,
    rows: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.resize(cols, rows);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_processCodePointRust(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    code_point: jint,
) {
    if ptr == 0 { return; }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &*(ptr as *const TerminalContext) };
        let mut engine = context.lock.write().unwrap();
        engine.process_code_point(code_point as u32);
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "processCodePointRust: panic caught");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resizeEngineRustFull(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cols: jint,
    rows: jint,
    _cw: jint,
    _ch: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.resize(cols, rows);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        let context = unsafe { &*(ptr as *mut TerminalContext) };
        context.running.store(false, Ordering::SeqCst);
        unsafe { let _ = Box::from_raw(ptr as *mut TerminalContext); }
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
    
    let context_ptr = ptr as *mut TerminalContext;
    let context = unsafe { &*context_ptr };
    
    // 克隆 PTY FD 以便在独立进程中使用
    let pty_fd = unsafe { libc::dup(fd) };
    if pty_fd < 0 { return; }

    // 使用 fork 创建独立进程而非线程
    match unsafe { libc::fork() } {
        0 => {
            // 子进程：这是你会看到的 "termux-rust-engine" 进程
            
            // 1. 设置进程名称
            let process_name = std::ffi::CString::new("termux-rust-engine").unwrap();
            unsafe {
                libc::prctl(libc::PR_SET_NAME, process_name.as_ptr(), 0, 0, 0);
            }
            
            android_log(LogPriority::INFO, "Rust Independent Process started (termux-rust-engine)");
            
            let mut file = unsafe { std::fs::File::from_raw_fd(pty_fd) };
            let mut buffer = [0u8; 8192];
            
            while context.running.load(Ordering::SeqCst) {
                match file.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        // 注意：子进程拥有 context 的拷贝，但它是独立的地址空间。
                        // 在真正的多进程架构中，我们需要通过共享内存同步数据。
                        // 目前为了演示“可见进程名”，我们让子进程运行解析逻辑。
                        let mut engine = context.lock.write().unwrap();
                        engine.process_bytes(&buffer[..n]);
                        // 在多进程模式下，这里需要通过 Socket/AIDL 通知父进程
                        engine.notify_screen_updated();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            android_log(LogPriority::INFO, "Rust Independent Process exiting");
            std::process::exit(0);
        }
        child_pid if child_pid > 0 => {
            // 父进程：继续返回 Java UI
            android_log(LogPriority::INFO, &format!("Forked child process PID: {}", child_pid));
        }
        _ => {
            android_log(LogPriority::ERROR, "Failed to fork rust process");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTitleFromRust(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    let title = engine.state.title.clone().unwrap_or_default();
    if let Ok(j_str) = env.new_string(title) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorRowFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.cursor.y as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorColFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.cursor.x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorStyleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.cursor.style as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorStyleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong, cursor_style: jint) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.cursor.style = cursor_style as i32;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_doDecSetOrResetFromRust(_env: JNIEnv, _class: JClass, ptr: jlong, setting: jboolean, mode: jint) {
    if ptr == 0 { return; }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &*(ptr as *const TerminalContext) };
        let mut engine = context.lock.write().unwrap();
        engine.state.do_decset_or_reset(setting != 0, mode as u32);
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "doDecSetOrResetFromRust: panic caught");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_shouldCursorBeVisibleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.cursor.should_be_visible(engine.state.cursor_enabled) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorEnabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.cursor_enabled { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isReverseVideoFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAlternateBufferActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.use_alternate_buffer { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorKeysApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.application_cursor_keys { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isKeypadApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.modes.is_enabled(DECSET_BIT_APPLICATION_KEYPAD) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isMouseTrackingActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.mouse_tracking { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isInsertModeActiveFromRust(_env: JNIEnv, _class: JClass, _ptr: jlong) -> jboolean { 0 }

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.scroll_counter as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.cols as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, row: jint, text: jni::sys::jintArray, styles: jni::sys::jlongArray,
) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    let cols = engine.state.cols as usize;
    let mut text_buf = vec![0i32; cols];
    let mut style_buf = vec![0i64; cols];
    engine.state.copy_row_codepoints(row, &mut text_buf);
    engine.state.copy_row_styles_i64(row, &mut style_buf);
    drop(engine);

    unsafe {
        let j_text = jni::objects::JIntArray::from_raw(text);
        let j_styles = jni::objects::JLongArray::from_raw(styles);
        let _ = env.set_int_array_region(&j_text, 0, &text_buf);
        let _ = env.set_long_array_region(&j_styles, 0, &style_buf);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getSelectedTextFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, x1: jint, y1: jint, x2: jint, y2: jint,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    let text = engine.state.get_current_screen().get_selected_text(x1, y1, x2, y2);
    if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getWordAtLocationFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, x: jint, y: jint,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    let text = engine.state.get_current_screen().get_row(y).get_word_at(x as usize);
    if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTranscriptTextFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    let text = engine.state.get_current_screen().get_transcript_text();
    if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.scroll_counter = 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    if engine.state.auto_scroll_disabled { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_toggleAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.auto_scroll_disabled = !engine.state.auto_scroll_disabled;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendMouseEventFromRust(_env: JNIEnv, _class: JClass, ptr: jlong, button: jint, col: jint, row: jint, pressed: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.send_mouse_event(button as u32, col, row, pressed != 0);
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
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    // send_key_event 现在在 TerminalEngine 上实现
    engine.send_key_event(key_code, Some(rust_str), meta_state);
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
        let context = unsafe { &*(ptr as *const TerminalContext) };
        let mut engine = context.lock.write().unwrap();
        engine.state.paste(&s);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getActiveTranscriptRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.get_current_screen().active_transcript_rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColorsFromRust(env: JNIEnv, _class: JClass, ptr: jlong) -> jintArray {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    let colors = engine.state.colors.current_colors;
    drop(engine);

    if let Ok(j_array) = env.new_int_array(colors.len() as jint) {
        unsafe { let _ = env.set_int_array_region(&j_array, 0, std::mem::transmute::<&[u32], &[i32]>(&colors)); }
        j_array.into_raw()
    } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.colors.reset();
    engine.state.report_colors_changed();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_updateTerminalSessionClientFromRust(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    client: JObject,
) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    if client.is_null() {
        engine.state.java_callback_obj = None;
    } else {
        if let Ok(global_ref) = env.new_global_ref(client) {
            engine.state.java_callback_obj = Some(global_ref);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkStateInRust(_env: JNIEnv, _class: JClass, ptr: jlong, state: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.cursor.blink_state = state != 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkingEnabledInRust(_env: JNIEnv, _class: JClass, ptr: jlong, enabled: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    engine.state.cursor.blinking_enabled = enabled != 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr != 0 { unsafe { let _ = Box::from_raw(ptr as *mut TerminalContext); } }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorCol(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.cursor.x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorRow(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    engine.state.cursor.y as jint
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
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let engine = context.lock.read().unwrap();
    let debug_info = engine.state.get_debug_info();
    if let Ok(j_str) = env.new_string(debug_info) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    }
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
