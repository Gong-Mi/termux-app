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

pub static JAVA_VM: OnceCell<jni::JavaVM> = OnceCell::new();

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: std::ffi::c_void) -> jint {
    let _ = JAVA_VM.set(vm);
    jni::sys::JNI_VERSION_1_6
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeInit(
    _env: JNIEnv,
    _class: JClass,
    cols: jint,
    rows: jint,
    total_rows: jint,
) -> jlong {
    let engine = TerminalEngine::new(cols, rows, total_rows, 10, 20);
    let context = Box::new(TerminalContext { engine });
    Box::into_raw(context) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeProcess(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
    callback: JObject,
) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
    if let Ok(bytes) = env.convert_byte_array(&j_array) {
        if context.engine.state.java_callback_obj.is_none() {
            if let Ok(global_ref) = env.new_global_ref(callback) {
                context.engine.state.java_callback_obj = Some(global_ref);
            }
        }
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
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    context.engine.state.resize(cols, rows);
}

// --- 补全的 FromRust 系列方法 ---

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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    row: jint,
    text: jni::sys::jcharArray,
    styles: jni::sys::jlongArray,
) {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let row_idx = row;
    
    // 我们需要把字符和样式数据拷贝到 Java 数组中
    // 为了性能，我们通常使用本地临时缓冲区
    let cols = context.engine.state.cols as usize;
    let mut text_buf = vec![0u16; cols];
    let mut style_buf = vec![0i64; cols];
    
    context.engine.state.copy_row_text(row_idx, &mut text_buf);
    context.engine.state.copy_row_styles_i64(row_idx, &mut style_buf);
    
    unsafe {
        let j_text = jni::objects::JCharArray::from_raw(text);
        let j_styles = jni::objects::JLongArray::from_raw(styles);
        let _ = env.set_char_array_region(&j_text, 0, &text_buf);
        let _ = env.set_long_array_region(&j_styles, 0, &style_buf);
    }
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getSelectedTextFromRust(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    x1: jint, y1: jint, x2: jint, y2: jint,
) -> jstring {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    // 简化实现：仅提取单行
    let text = if y1 == y2 {
        let row = context.engine.state.get_current_screen().get_row(y1);
        row.get_selected_text(x1 as usize, x2 as usize)
    } else {
        String::from("Multiple lines selection not implemented in Rust yet")
    };
    
    if let Ok(j_str) = env.new_string(text) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    }
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
