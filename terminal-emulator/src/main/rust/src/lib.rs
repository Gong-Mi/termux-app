use jni::JNIEnv;
use jni::objects::{JClass, JString, JObject};
use jni::sys::{jint, jlong, jbyteArray, jboolean, jintArray, jstring};
use once_cell::sync::OnceCell;

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
pub use crate::terminal::sixel::{SixelDecoder, SixelState};
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
    let context = Box::new(TerminalContext { engine });
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
    // 使用 catch_unwind 防止 Rust panic 导致 JVM 崩溃
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: 指针必须由 Java 层正确传递，且在使用期间保持有效
        // Java 层已通过 isAlive() 检查，但仍有竞争条件可能，因此需要额外保护
        let context = unsafe { &mut *(ptr as *mut TerminalContext) };
        let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
        if let Ok(bytes) = env.convert_byte_array(&j_array) {
            context.engine.process_bytes(&bytes);
        }
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "nativeProcess: panic caught, possible use-after-free");
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
    // 使用 catch_unwind 防止 Rust panic 导致 JVM 崩溃
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: 指针必须由 Java 层正确传递，且在使用期间保持有效
        // Java 层已通过 isAlive() 检查，但仍有竞争条件可能，因此需要额外保护
        let context = unsafe { &mut *(ptr as *mut TerminalContext) };
        let j_array = unsafe { jni::objects::JByteArray::from_raw(batch) };
        if let Ok(bytes) = env.convert_byte_array(&j_array) {
            let len = length as usize;
            let actual_len = std::cmp::min(len, bytes.len());
            context.engine.process_bytes(&bytes[..actual_len]);
        }
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "processBatchRust: panic caught, possible use-after-free");
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
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.resize(cols, rows);
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
        let context = unsafe { &mut *(ptr as *mut TerminalContext) };
        context.engine.process_code_point(code_point as u32);
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "processCodePointRust: panic caught, possible use-after-free");
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
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.resize(cols, rows);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        unsafe { let _ = Box::from_raw(ptr as *mut TerminalContext); }
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
    let title = context.engine.state.title.clone().unwrap_or_default();
    if let Ok(j_str) = env.new_string(title) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorRowFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.y as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorColFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorStyleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.style as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorStyleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong, cursor_style: jint) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.cursor.style = cursor_style as i32;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_doDecSetOrResetFromRust(_env: JNIEnv, _class: JClass, ptr: jlong, setting: jboolean, mode: jint) {
    if ptr == 0 { return; }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &mut *(ptr as *mut TerminalContext) };
        context.engine.state.do_decset_or_reset(setting != 0, mode as u32);
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "doDecSetOrResetFromRust: panic caught");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_shouldCursorBeVisibleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.cursor.should_be_visible(context.engine.state.cursor_enabled) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorEnabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.cursor_enabled { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isReverseVideoFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAlternateBufferActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.use_alternate_buffer { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorKeysApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.application_cursor_keys { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isKeypadApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.modes.is_enabled(DECSET_BIT_APPLICATION_KEYPAD) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isMouseTrackingActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.mouse_tracking { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isInsertModeActiveFromRust(_env: JNIEnv, _class: JClass, _ptr: jlong) -> jboolean { 0 }

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.scroll_counter as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cols as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, row: jint, text: jni::sys::jcharArray, styles: jni::sys::jlongArray,
) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let cols = context.engine.state.cols as usize;
    let mut text_buf = vec![0u16; cols];
    let mut style_buf = vec![0i64; cols];
    context.engine.state.copy_row_text(row, &mut text_buf);
    context.engine.state.copy_row_styles_i64(row, &mut style_buf);
    unsafe {
        let j_text = jni::objects::JCharArray::from_raw(text);
        let j_styles = jni::objects::JLongArray::from_raw(styles);
        let _ = env.set_char_array_region(&j_text, 0, &text_buf);
        let _ = env.set_long_array_region(&j_styles, 0, &style_buf);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getSelectedTextFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, x1: jint, y1: jint, x2: jint, y2: jint,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let text = context.engine.state.get_current_screen().get_selected_text(x1, y1, x2, y2);
    if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getWordAtLocationFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, x: jint, y: jint,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let text = context.engine.state.get_current_screen().get_row(y).get_word_at(x as usize);
    if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTranscriptTextFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let text = context.engine.state.get_current_screen().get_transcript_text();
    if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.scroll_counter = 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.auto_scroll_disabled { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_toggleAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.auto_scroll_disabled = !context.engine.state.auto_scroll_disabled;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendMouseEventFromRust(_env: JNIEnv, _class: JClass, ptr: jlong, button: jint, col: jint, row: jint, pressed: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.send_mouse_event(button as u32, col, row, pressed != 0);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendKeyCodeFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong, key_code: jint, char_str: jstring, meta_state: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    let rust_str = if !char_str.is_null() {
        let j_str = unsafe { JString::from_raw(char_str) };
        env.get_string(&j_str).ok().map(|s| String::from(s)).unwrap_or_default()
    } else { String::new() };
    context.engine.state.send_key_event(key_code, Some(rust_str), meta_state);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_pasteTextFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong, text: jstring,
) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    if !text.is_null() {
        let j_str = unsafe { JString::from_raw(text) };
        if let Ok(rust_str) = env.get_string(&j_str) { context.engine.state.paste(&String::from(rust_str)); }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getActiveTranscriptRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.get_current_screen().active_transcript_rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColorsFromRust(env: JNIEnv, _class: JClass, ptr: jlong) -> jintArray {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let colors = context.engine.state.colors.current_colors;
    if let Ok(j_array) = env.new_int_array(colors.len() as jint) {
        unsafe { let _ = env.set_int_array_region(&j_array, 0, std::mem::transmute::<&[u32], &[i32]>(&colors)); }
        j_array.into_raw()
    } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.colors.reset();
    context.engine.state.report_colors_changed();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_updateTerminalSessionClientFromRust(_env: JNIEnv, _class: JClass, _ptr: jlong, _client: JObject) {}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkStateInRust(_env: JNIEnv, _class: JClass, ptr: jlong, state: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.cursor.blink_state = state != 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkingEnabledInRust(_env: JNIEnv, _class: JClass, ptr: jlong, enabled: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.cursor.blinking_enabled = enabled != 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr != 0 { unsafe { let _ = Box::from_raw(ptr as *mut TerminalContext); } }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorCol(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorRow(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.y as jint
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
    // 简单实现：返回 1 作为默认值
    // 详细实现在 Java 层缓存处理
    let c = ucs as u32;
    if c < 2048 {
        return 1;
    }
    1
}
