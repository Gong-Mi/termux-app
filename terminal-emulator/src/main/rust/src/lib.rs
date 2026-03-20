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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_createEngineRustWithCallback(
    mut env: JNIEnv,
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
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
    _callback: JObject,
) {
    if ptr == 0 || data.is_null() { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
    if let Ok(bytes) = env.convert_byte_array(&j_array) {
        context.engine.process_bytes(&bytes);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_processBatchRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    batch: jbyteArray,
    length: jint,
) {
    if ptr == 0 || batch.is_null() { return; }
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    let j_array = unsafe { jni::objects::JByteArray::from_raw(batch) };
    if let Ok(bytes) = env.convert_byte_array(&j_array) {
        let len = length as usize;
        let actual_len = std::cmp::min(len, bytes.len());
        context.engine.process_bytes(&bytes[..actual_len]);
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
    android_log(LogPriority::INFO, &format!("JNI: nativeResize to {}x{}", cols, rows));
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.resize(cols, rows);
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
    android_log(LogPriority::INFO, &format!("JNI: resizeEngineRustFull to {}x{}", cols, rows));
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.resize(cols, rows);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    android_log(LogPriority::INFO, "JNI: destroyEngineRust called");
    if ptr != 0 {
        unsafe { let _ = Box::from_raw(ptr as *mut TerminalContext); }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTitleFromRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jstring {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let title = context.engine.state.title.clone().unwrap_or_default();
    if let Ok(j_str) = env.new_string(title) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorRowFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.y as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorColFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorStyleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.style as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_shouldCursorBeVisibleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.cursor.should_be_visible(context.engine.state.cursor_enabled) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorEnabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.cursor_enabled { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isReverseVideoFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAlternateBufferActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.use_alternate_buffer { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorKeysApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.application_cursor_keys { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isKeypadApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.modes.is_enabled(DECSET_BIT_APPLICATION_KEYPAD) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isMouseTrackingActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.mouse_tracking { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isInsertModeActiveFromRust(_env: JNIEnv, _class: JClass, _ptr: jlong) -> jboolean { 0 }

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.scroll_counter as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cols as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong, row: jint, text: jni::sys::jcharArray, styles: jni::sys::jlongArray,
) {
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
    mut env: JNIEnv, _class: JClass, ptr: jlong, x1: jint, y1: jint, x2: jint, y2: jint,
) -> jstring {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let text = if y1 == y2 {
        context.engine.state.get_current_screen().get_row(y1).get_selected_text(x1 as usize, x2 as usize)
    } else { String::from("Multi-line selection not yet supported") };
    if let Ok(j_str) = env.new_string(text) { j_str.into_raw() } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.scroll_counter = 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    if context.engine.state.auto_scroll_disabled { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_toggleAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.auto_scroll_disabled = !context.engine.state.auto_scroll_disabled;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendMouseEventFromRust(_env: JNIEnv, _class: JClass, ptr: jlong, button: jint, col: jint, row: jint, pressed: jboolean) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.send_mouse_event(button as u32, col, row, pressed != 0);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendKeyCodeFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong, key_code: jint, char_str: jstring, meta_state: jint,
) {
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
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    if !text.is_null() {
        let j_str = unsafe { JString::from_raw(text) };
        if let Ok(rust_str) = env.get_string(&j_str) { context.engine.state.paste(&String::from(rust_str)); }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getActiveTranscriptRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.get_current_screen().active_transcript_rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColorsFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong) -> jintArray {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let colors = context.engine.state.colors.current_colors;
    if let Ok(j_array) = env.new_int_array(colors.len() as jint) {
        unsafe { let _ = env.set_int_array_region(&j_array, 0, std::mem::transmute::<&[u32], &[i32]>(&colors)); }
        j_array.into_raw()
    } else { std::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.colors.reset();
    context.engine.state.report_colors_changed();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_updateTerminalSessionClientFromRust(_env: JNIEnv, _class: JClass, _ptr: jlong, _client: JObject) {}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkStateInRust(_env: JNIEnv, _class: JClass, ptr: jlong, state: jboolean) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.cursor.blink_state = state != 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkingEnabledInRust(_env: JNIEnv, _class: JClass, ptr: jlong, enabled: jboolean) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.cursor.blinking_enabled = enabled != 0;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr != 0 { unsafe { let _ = Box::from_raw(ptr as *mut TerminalContext); } }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorCol(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorRow(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.y as jint
}
