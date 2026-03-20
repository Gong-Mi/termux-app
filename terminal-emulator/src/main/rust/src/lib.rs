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
    total_rows: jint,
    cw: jint,
    ch: jint,
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
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
    if let Ok(bytes) = env.convert_byte_array(&j_array) {
        // android_log(LogPriority::VERBOSE, &format!("JNI: nativeProcess {} bytes", bytes.len()));
        context.engine.process_bytes(&bytes);
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    row: jint,
    text: jni::sys::jcharArray,
    styles: jni::sys::jlongArray,
) {
    // 渲染频繁调用，使用 VERBOSE 或不打印
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    android_log(LogPriority::INFO, "JNI: destroyEngineRust called");
    if ptr != 0 {
        unsafe {
            let _ = Box::from_raw(ptr as *mut TerminalContext);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getRowsFromRust(ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColsFromRust(ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cols as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.scroll_counter as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getActiveTranscriptRowsFromRust(ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.get_current_screen().active_transcript_rows as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorCol(ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorRow(ptr: jlong) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor.y as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        unsafe {
            let _ = Box::from_raw(ptr as *mut TerminalContext);
        }
    }
}
