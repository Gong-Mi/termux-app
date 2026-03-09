//! Termux Rust JNI Library
//!
//! This library provides Rust implementations for terminal emulation functions,
//! including fast-path ASCII processing and a full terminal engine.

#![warn(clippy::all)]
#![allow(clippy::missing_safety_doc)]

use jni::JNIEnv;
use jni::sys::{
    JNINativeInterface_, jbyteArray, jcharArray, jclass, jint, jintArray, jlong, jlongArray,
    jobject, jobjectArray, jstring,
};

pub mod engine;
pub mod fastpath;
pub mod pty;
pub mod utils;

use engine::TerminalEngine;

// ==========================================
// 1. 无状态 / 工具类 JNI
// ==========================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_WcWidth_widthRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    ucs: jint,
) -> jint {
    utils::get_char_width(ucs as u32) as jint
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_processBatchRust(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    input: jbyteArray,
    offset: jint,
    length: jint,
    use_line_drawing: jni::sys::jboolean,
) -> jint {
    unsafe {
        let env = match JNIEnv::from_raw(env_ptr) {
            Ok(e) => e,
            Err(_) => return 0,
        };
        
        let internal = env.get_native_interface();
        let mut is_copy = jni::sys::JNI_FALSE;
        let input_ptr =
            ((**internal).GetPrimitiveArrayCritical.unwrap())(internal, input, &mut is_copy)
                as *const u8;

        if !input_ptr.is_null() {
            let input_len = ((**internal).GetArrayLength.unwrap())(internal, input) as usize;
            let start = offset as usize;
            let len = length as usize;

            let result = if start + len <= input_len {
                fastpath::scan_ascii_batch(
                    std::slice::from_raw_parts(input_ptr.add(start), len),
                    use_line_drawing != jni::sys::JNI_FALSE,
                )
            } else {
                0
            };

            ((**internal).ReleasePrimitiveArrayCritical.unwrap())(
                internal,
                input,
                input_ptr as *mut _,
                jni::sys::JNI_ABORT,
            );
            result as jint
        } else {
            0
        }
    }
}

// ============================================================================
// 有状态引擎 JNI - Full Takeover 模式
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_createEngineRustWithCallback(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    cols: jint,
    rows: jint,
    total_rows: jint,
    cell_width: jint,
    cell_height: jint,
    callback_obj: jobject,
) -> jlong {
    let mut engine = Box::new(TerminalEngine::new(cols, rows, total_rows, cell_width, cell_height));
    // 设置 Java 回调
    engine.state.set_java_callback(env_ptr, callback_obj);
    Box::into_raw(engine) as jlong
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_processEngineRust(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
    input: jbyteArray,
    offset: jint,
    length: jint,
) {
    unsafe {
        if engine_ptr == 0 || length == 0 {
            return;
        }
        let engine = &mut *(engine_ptr as *mut TerminalEngine);
        let env = match JNIEnv::from_raw(env_ptr) {
            Ok(e) => e,
            Err(_) => return,
        };

        let internal = env.get_native_interface();
        let mut is_copy = jni::sys::JNI_FALSE;
        let input_ptr =
            ((**internal).GetPrimitiveArrayCritical.unwrap())(internal, input, &mut is_copy)
                as *const u8;

        if !input_ptr.is_null() {
            let input_len = ((**internal).GetArrayLength.unwrap())(internal, input) as usize;
            let start = offset as usize;
            let len = length as usize;

            if start + len <= input_len {
                let slice = std::slice::from_raw_parts(input_ptr.add(start), len);
                engine.process_bytes(slice);
            }

            ((**internal).ReleasePrimitiveArrayCritical.unwrap())(
                internal,
                input,
                input_ptr as *mut _,
                jni::sys::JNI_ABORT,
            );
            
            // 通知 Java 刷新界面
            engine.state.report_screen_update();
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
    row: jint,
    dest_text: jcharArray,
    dest_style: jlongArray,
) {
    unsafe {
        if engine_ptr == 0 {
            return;
        }
        let engine = &mut *(engine_ptr as *mut TerminalEngine);
        let env = match JNIEnv::from_raw(env_ptr) {
            Ok(e) => e,
            Err(_) => return,
        };

        let internal = env.get_native_interface();
        let mut is_copy = jni::sys::JNI_FALSE;
        let text_ptr =
            ((**internal).GetPrimitiveArrayCritical.unwrap())(internal, dest_text, &mut is_copy)
                as *mut u16;
        let style_ptr =
            ((**internal).GetPrimitiveArrayCritical.unwrap())(internal, dest_style, &mut is_copy)
                as *mut i64;

        if !text_ptr.is_null() && !style_ptr.is_null() {
            let text_len = ((**internal).GetArrayLength.unwrap())(internal, dest_text) as usize;
            let style_len = ((**internal).GetArrayLength.unwrap())(internal, dest_style) as usize;
            let text_slice = std::slice::from_raw_parts_mut(text_ptr, text_len);
            let style_slice = std::slice::from_raw_parts_mut(style_ptr, style_len);

            engine.state.copy_row_text(row as usize, text_slice);
            engine.state.copy_row_styles(row as usize, style_slice);
        }

        if !style_ptr.is_null() {
            ((**internal).ReleasePrimitiveArrayCritical.unwrap())(
                internal,
                dest_style,
                style_ptr as *mut _,
                0,
            );
        }
        if !text_ptr.is_null() {
            ((**internal).ReleasePrimitiveArrayCritical.unwrap())(
                internal,
                dest_text,
                text_ptr as *mut _,
                0,
            );
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_resizeEngineRustFull(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
    new_cols: jint,
    new_rows: jint,
) {
    unsafe {
        if engine_ptr == 0 {
            return;
        }
        let engine = &mut *(engine_ptr as *mut TerminalEngine);
        engine.resize(new_cols, new_rows);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) {
    unsafe {
        if engine_ptr == 0 {
            return;
        }
        let _ = Box::from_raw(engine_ptr as *mut TerminalEngine);
    }
}

// ============================================================================
// Full Takeover 模式 - 额外 JNI 接口
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorColFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 { return -1; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    engine.state.cursor_x as jint
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorRowFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 { return -1; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    engine.state.cursor_y as jint
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorStyleFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 { return 0; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    engine.state.cursor_style as jint
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_shouldCursorBeVisibleFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 { return jni::sys::JNI_FALSE; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    if engine.state.cursor_enabled && engine.state.cursor_blink_state {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isReverseVideoFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 { return jni::sys::JNI_FALSE; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    if engine.state.reverse_video {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTitleFromRust(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jstring {
    if engine_ptr == 0 { return std::ptr::null_mut(); }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    let env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };
    match &engine.state.title {
        Some(title) => match env.new_string(title) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_reportFocusGainFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 { return; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    engine.state.report_focus_gain();
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_reportFocusLossFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 { return; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    engine.state.report_focus_loss();
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_pasteTextFromRust(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
    text: jstring,
) {
    if engine_ptr == 0 { return; }
    let engine = unsafe { &mut *(engine_ptr as *mut TerminalEngine) };
    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return,
    };

    if let Ok(rust_text) = env.get_string(&unsafe { jni::objects::JString::from_raw(text) }) {
        let text_str: String = rust_text.into();
        engine.state.paste_start(&text_str);
    }
}


#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 { return 0; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    engine.state.scroll_counter
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearScrollCounterFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 { return; }
    let engine = unsafe { &mut *(engine_ptr as *mut TerminalEngine) };
    engine.state.clear_scroll_counter();
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAutoScrollDisabledFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 { return jni::sys::JNI_FALSE; }
    let engine = unsafe { &*(engine_ptr as *mut TerminalEngine) };
    if engine.state.auto_scroll_disabled {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_toggleAutoScrollDisabledFromRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 { return; }
    let engine = unsafe { &mut *(engine_ptr as *mut TerminalEngine) };
    engine.state.toggle_auto_scroll_disabled();
}

// ==========================================
// 4. 进程管理 JNI
// ==========================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_createSubprocess(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    cmd: jstring,
    cwd: jstring,
    args: jobjectArray,
    env_vars: jobjectArray,
    process_id_array: jintArray,
    rows: jint,
    columns: jint,
    cell_width: jint,
    cell_height: jint,
) -> jint {
    unsafe {
        pty::create_subprocess(
            env_ptr,
            cmd,
            cwd,
            args,
            env_vars,
            process_id_array,
            rows,
            columns,
            cell_width,
            cell_height,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_setPtyWindowSize(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    fd: jint,
    rows: jint,
    cols: jint,
    cell_width: jint,
    cell_height: jint,
) {
    pty::set_pty_window_size(fd, rows, cols, cell_width, cell_height);
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_waitFor(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    pid: jint,
) -> jint {
    pty::wait_for(pid)
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_close(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    fd: jint,
) {
    let _ = nix::unistd::close(fd);
}
