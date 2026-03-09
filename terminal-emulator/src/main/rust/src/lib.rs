//! Termux Rust JNI Library
//!
//! This library provides Rust implementations for terminal emulation functions,
//! including fast-path ASCII processing and a full terminal engine.
//!
//! # Safety
//!
//! Most functions in this library are `unsafe` because they interact with raw JNI pointers.
//! The caller must ensure that the JNI environment pointer and Java object handles are valid.

#![warn(clippy::all)]
#![allow(clippy::missing_safety_doc)] // JNI functions have implicit safety requirements

use jni::JNIEnv;
use jni::objects::JByteArray;
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
        let input = JByteArray::from_raw(input);

        let input_bytes = match env.convert_byte_array(&input) {
            Ok(b) => b,
            Err(_) => return 0,
        };

        let start = offset as usize;
        let len = length as usize;
        if start + len > input_bytes.len() {
            return 0;
        }

        fastpath::scan_ascii_batch(
            &input_bytes[start..start + len],
            use_line_drawing != jni::sys::JNI_FALSE,
        ) as jint
    }
}

use rayon::prelude::*;

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_mapLineDrawingParallelNative(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    src: jbyteArray,
    src_offset: jint,
    dest: jcharArray,
    length: jint,
) {
    unsafe {
        let env = match JNIEnv::from_raw(env_ptr) {
            Ok(e) => e,
            Err(_) => return,
        };

        let len = length as usize;
        let mut src_vec = vec![0i8; len];
        let _ = env.get_byte_array_region(JByteArray::from_raw(src), src_offset, &mut src_vec);

        let dest_vec: Vec<u16> = src_vec
            .par_iter()
            .map(|&b| utils::map_line_drawing(b as u8) as u16)
            .collect();

        let _ = env.set_char_array_region(jni::objects::JCharArray::from_raw(dest), 0, &dest_vec);
    }
}

// ==========================================
// 2. 有状态引擎 JNI
// ==========================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_createEngineRust(
    _env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    cols: jint,
    rows: jint,
    total_rows: jint,
) -> jlong {
    let engine = Box::new(TerminalEngine::new(cols, rows, total_rows));
    Box::into_raw(engine) as jlong
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_createEngineRustWithCallback(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    cols: jint,
    rows: jint,
    total_rows: jint,
    callback_obj: jobject,
) -> jlong {
    let mut engine = Box::new(TerminalEngine::new(cols, rows, total_rows));
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
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_resizeEngineRust(
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

// ==========================================
// 3. 纯 C 接口 (供 Python/单元测试调用)
// ==========================================

#[unsafe(no_mangle)]
pub extern "C" fn test_create_engine(cols: i32, rows: i32) -> jlong {
    let engine = Box::new(TerminalEngine::new(cols, rows, rows));
    Box::into_raw(engine) as jlong
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_process_data(engine_ptr: jlong, data: *const u8, len: usize) {
    unsafe {
        let engine = &mut *(engine_ptr as *mut TerminalEngine);
        let slice = std::slice::from_raw_parts(data, len);
        engine.process_bytes(slice);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_get_cursor_x(engine_ptr: jlong) -> i32 {
    unsafe { (*(engine_ptr as *mut TerminalEngine)).state.cursor_x }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_get_cursor_y(engine_ptr: jlong) -> i32 {
    unsafe { (*(engine_ptr as *mut TerminalEngine)).state.cursor_y }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_resize(engine_ptr: jlong, cols: i32, rows: i32) {
    unsafe {
        let engine = &mut *(engine_ptr as *mut TerminalEngine);
        engine.resize(cols, rows);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn test_destroy_engine(engine_ptr: jlong) {
    unsafe {
        if engine_ptr != 0 {
            let _ = Box::from_raw(engine_ptr as *mut TerminalEngine);
        }
    }
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
