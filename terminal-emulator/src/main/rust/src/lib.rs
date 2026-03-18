//! Termux Rust JNI Library
//!
//! This library provides Rust implementations for terminal emulation functions,
//! including fast-path ASCII processing and a full terminal engine.

#![warn(clippy::all)]
#![allow(clippy::missing_safety_doc)]
#![allow(unsafe_op_in_unsafe_fn)]

use jni::sys::{
    jbooleanArray, jbyteArray, jcharArray, jclass, jint, jintArray, jlong, jlongArray, jobject,
    jobjectArray, jsize, jstring, JNI_VERSION_1_6,
};
use jni::{JNIEnv, JavaVM};
use std::panic;
use std::sync::OnceLock;

pub mod bootstrap;
pub mod engine;
pub mod fastpath;
pub mod pty;
pub mod utils;

use engine::TerminalEngine;

/// 全局 JavaVM 引用，用于在回调中安全获取 JNIEnv
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _reserved: *mut std::ffi::c_void) -> jint {
    let _ = JAVA_VM.set(vm);
    JNI_VERSION_1_6
}

/// 辅助宏：包装 JNI 调用，捕获 Panic 并防止程序崩溃
macro_rules! catch_panic {
    ($($tokens:tt)*) => {
        match panic::catch_unwind(panic::AssertUnwindSafe(move || {
            $($tokens)*
        })) {
            Ok(v) => v,
            Err(e) => {
                // 将错误信息打印到 logcat，方便调试 Rust Crash
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown Rust Panic".to_string()
                };
                eprintln!("[Termux-Rust-Panic] {}", msg);
                Default::default()
            }
        }
    }
}

// ==========================================
// 1. 无状态 / 工具类 JNI
// ==========================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_WcWidth_widthRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    ucs: jint,
) -> jint {
    catch_panic! {
        utils::get_char_width(ucs as u32) as jint
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_processBatchRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    input: jbyteArray,
    length: jint,
) {
    catch_panic! {
        eprintln!("[Termux-Rust] JNI processBatchRust called: ptr={:p}, len={}", engine_ptr as *const (), length);
        if engine_ptr == 0 || length <= 0 || input.is_null() {
            eprintln!("[Termux-Rust] processBatchRust: invalid arguments");
            return;
        }

        let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
            Ok(e) => e,
            Err(_) => {
                eprintln!("[Termux-Rust] processBatchRust: failed to get JNIEnv");
                return;
            },
        };

        let j_array = unsafe { jni::objects::JByteArray::from_raw(input) };
        let mut data_vec = vec![0i8; length as usize];
        
        if env.get_byte_array_region(&j_array, 0, &mut data_vec).is_err() {
            eprintln!("[Termux-Rust] processBatchRust: failed to get byte array region");
            return;
        }

        // 转换为 u8
        let data_u8: Vec<u8> = data_vec.iter().map(|&b| b as u8).collect();
        
        let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
        {
            eprintln!("[Termux-Rust] processBatchRust: acquiring write lock...");
            let mut engine = match engine_lock.write() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("[Termux-Rust] processBatchRust: Lock poisoned: {:?}", e);
                    return;
                }
            };
            eprintln!("[Termux-Rust] processBatchRust: processing bytes...");
            engine.process_bytes(&data_u8);
            
            eprintln!("[Termux-Rust] processBatchRust: syncing to shared...");
            engine.sync_state_to_flat();
        }
        eprintln!("[Termux-Rust] processBatchRust: done.");
    }
}

// ============================================================================
// 有状态引擎 JNI - Full Takeover 模式
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_createEngineRustWithCallback(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    cols: jint,
    rows: jint,
    cell_width: jint,
    cell_height: jint,
    transcript_rows: jint,
    callback_obj: jobject,
) -> jlong {
    let mut engine_inner = TerminalEngine::new(cols, rows, transcript_rows, cell_width, cell_height);

    // 创建全局引用
    if let Ok(env) = unsafe { JNIEnv::from_raw(env_ptr) } {
        if let Ok(global_obj) =
            env.new_global_ref(unsafe { jni::objects::JObject::from_raw(callback_obj) })
        {
            // 设置 Java 回调
            engine_inner.state.set_java_callback(global_obj);
            eprintln!("[Termux-JNI] Global callback reference established.");
        }
    }

    let engine_ptr = Box::into_raw(Box::new(std::sync::RwLock::new(engine_inner))) as jlong;
    eprintln!("[Termux-Rust] Engine created at ptr: {:p}", engine_ptr as *const ());
    engine_ptr
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_processEngineRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    input: jbyteArray,
    offset: jint,
    length: jint,
) {
    if engine_ptr == 0 || length <= 0 || input.is_null() {
        return;
    }

    let env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return,
    };

    let j_array = unsafe { jni::objects::JByteArray::from_raw(input) };
    let len = length as usize;
    let mut data_vec = vec![0i8; len];
    
    if env.get_byte_array_region(&j_array, offset, &mut data_vec).is_err() {
        return;
    }
    
    // 转换为 u8 向量
    let data_u8: Vec<u8> = data_vec.iter().map(|&b| b as u8).collect();

    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    {
        let mut guard = match engine_lock.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.process_bytes(&data_u8);
    }

    // 解析完成后通知 Java 刷新界面
    if let Ok(guard) = engine_lock.read() {
        guard.state.report_screen_update();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_writeASCIIBatchNative(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    src: jbyteArray,
    src_offset: jint,
    dest_text: jcharArray,
    dest_style: jlongArray,
    dest_offset: jint,
    length: jint,
    style: jlong,
    use_line_drawing: jni::sys::jboolean,
) {
    unsafe {
        let env = match JNIEnv::from_raw(env_ptr) {
            Ok(e) => e,
            Err(_) => return,
        };

        let len = length as usize;
        let offset = dest_offset as usize;
        let use_ld = use_line_drawing != jni::sys::JNI_FALSE;

        let internal = env.get_native_interface();
        let mut is_copy = jni::sys::JNI_FALSE;

        let src_ptr = ((**internal).GetPrimitiveArrayCritical.unwrap())(internal, src, &mut is_copy)
            as *const i8;
        let text_ptr =
            ((**internal).GetPrimitiveArrayCritical.unwrap())(internal, dest_text, &mut is_copy)
                as *mut u16;
        let style_ptr =
            ((**internal).GetPrimitiveArrayCritical.unwrap())(internal, dest_style, &mut is_copy)
                as *mut i64;

        if !src_ptr.is_null() && !text_ptr.is_null() && !style_ptr.is_null() {
            let src_slice = std::slice::from_raw_parts(src_ptr.add(src_offset as usize), len);
            let text_slice = std::slice::from_raw_parts_mut(text_ptr.add(offset), len);
            let style_slice = std::slice::from_raw_parts_mut(style_ptr.add(offset), len);

            for i in 0..len {
                let b = src_slice[i] as u8;
                text_slice[i] = if use_ld {
                    utils::map_line_drawing(b) as u16
                } else {
                    b as u16
                };
                style_slice[i] = style;
            }
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
        if !src_ptr.is_null() {
            ((**internal).ReleasePrimitiveArrayCritical.unwrap())(
                internal,
                src,
                src_ptr as *mut _,
                jni::sys::JNI_ABORT,
            );
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    row: jint,
    dest_text: jcharArray,
    dest_style: jlongArray,
) {
    if engine_ptr == 0 || dest_text.is_null() || dest_style.is_null() {
        return;
    }

    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };

    // 步骤 1: 防御性数据获取
    let (text_vec, style_vec) = {
        let guard = match engine_lock.read() {
            Ok(g) => g,
            Err(_) => return,
        };

        let cols = guard.state.cols as usize;
        let mut t_vec = vec![0u16; cols];
        let mut s_vec = vec![0i64; cols];

        // 核心检查：确保行号在 Rust 当前缓冲区范围内
        let buffer = guard.state.get_current_buffer();
        let internal_row = guard.state.external_to_internal_row(row);

        if internal_row < buffer.len() {
            // 安全拷贝：这可能会产生 Surrogate 对，导致长度超出 cols，
            // 但我们的 Java 侧 buffer 分配了 columns * 3，所以是安全的。
            guard.state.copy_row_text(row as usize, &mut t_vec);
            guard.state.copy_row_styles(row as usize, &mut s_vec);
        } else {
            // 越界保护：填充空格
            for i in 0..cols { t_vec[i] = ' ' as u16; }
        }
        (t_vec, s_vec)
    }; 

    // 步骤 2: 安全写回 Java
    let env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return,
    };

    let j_text = unsafe { jni::objects::JCharArray::from_raw(dest_text) };
    let j_style = unsafe { jni::objects::JLongArray::from_raw(dest_style) };

    let text_len = match env.get_array_length(&j_text) {
        Ok(l) => l as usize,
        Err(_) => return,
    };
    let style_len = match env.get_array_length(&j_style) {
        Ok(l) => l as usize,
        Err(_) => return,
    };

    let copy_text = std::cmp::min(text_len, text_vec.len());
    let copy_styles = std::cmp::min(style_len, style_vec.len());

    let _ = env.set_char_array_region(&j_text, 0, &text_vec[..copy_text]);
    let _ = env.set_long_array_region(&j_style, 0, &style_vec[..copy_styles]);
}
// ============================================================================
// 批量读取优化 - 减少 JNI 调用次数
// ============================================================================

/// 内部通用的批量读取实现，不依赖 JNI 导出签名，避免套娃调用失败
unsafe fn internal_read_screen_batch(
    env_ptr: *mut jni::sys::JNIEnv,
    engine_ptr: jlong,
    dest_text: jobjectArray,
    dest_style: jobjectArray,
    dest_line_wraps: jni::sys::jbooleanArray,
    start_row: jint,
    num_rows: jint,
) {
    if engine_ptr == 0 || num_rows <= 0 || dest_text.is_null() {
        return;
    }

    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return,
    };

    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return,
    };

    let cols = guard.state.cols as usize;
    let mut text_vec = vec![0u16; cols];
    let mut style_vec = vec![0i64; cols];

    // 绑定到变量以延长生命周期
    let j_dest_text = unsafe { jni::objects::JObjectArray::from_raw(dest_text) };
    let j_dest_style = unsafe { jni::objects::JObjectArray::from_raw(dest_style) };

    for i in 0..num_rows as usize {
        let row = start_row as usize + i;
        guard.state.copy_row_text(row, &mut text_vec);
        guard.state.copy_row_styles(row, &mut style_vec);
        let line_wrap = guard.state.get_line_wrap(row);

        // 使用安全包装器写回 Java
        if let Ok(j_text_row_obj) = env.get_object_array_element(&j_dest_text, i as jsize) {
            let j_text_row = unsafe { jni::objects::JCharArray::from_raw(j_text_row_obj.as_raw() as jcharArray) };
            let _ = env.set_char_array_region(&j_text_row, 0, &text_vec);
            // 显式释放局部引用，防止循环中堆积
            drop(j_text_row_obj); 
        }
        if let Ok(j_style_row_obj) = env.get_object_array_element(&j_dest_style, i as jsize) {
            let j_style_row = unsafe { jni::objects::JLongArray::from_raw(j_style_row_obj.as_raw() as jlongArray) };
            let _ = env.set_long_array_region(&j_style_row, 0, &style_vec);
            // 显式释放局部引用
            drop(j_style_row_obj);
        }
        if !dest_line_wraps.is_null() {
            let j_wraps = unsafe { jni::objects::JBooleanArray::from_raw(dest_line_wraps) };
            let _ = env.set_boolean_array_region(&j_wraps, i as jsize, &[if line_wrap { jni::sys::JNI_TRUE } else { jni::sys::JNI_FALSE }]);
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_readScreenBatchFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    dest_text: jobjectArray,
    dest_style: jobjectArray,
    dest_line_wraps: jni::sys::jbooleanArray,
    start_row: jint,
    num_rows: jint,
) {
    internal_read_screen_batch(env_ptr, engine_ptr, dest_text, dest_style, dest_line_wraps, start_row, num_rows);
}

/// 读取整个屏幕的优化版本（固定从第 0 行开始）
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_readFullScreenFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    dest_text: jobjectArray,
    dest_style: jobjectArray,
    dest_line_wraps: jni::sys::jbooleanArray,
) {
    if engine_ptr == 0 {
        return;
    }
    let rows = {
        let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
        let guard = engine_lock.read().unwrap();
        guard.state.rows as jint
    };

    internal_read_screen_batch(env_ptr, engine_ptr, dest_text, dest_style, dest_line_wraps, 0, rows);
}

// ============================================================================
// 状态查询方法
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_calculateChecksumFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jlong {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    guard.state.calculate_checksum() as jlong
}

/// 获取终端行数
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getRowsFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    guard.state.rows as jint
}

/// 获取终端列数
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColsFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    guard.state.cols as jint
}

/// 获取选定区域的文本
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getSelectedTextFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    x1: jint,
    y1: jint,
    x2: jint,
    y2: jint,
) -> jstring {
    if engine_ptr == 0 {
        return std::ptr::null_mut();
    }
    
    let text = {
        let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
        let guard = match engine_lock.read() {
            Ok(g) => g,
            Err(_) => return std::ptr::null_mut(),
        };
        guard.state.get_selected_text(x1, y1, x2, y2)
    };

    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    match env.new_string(text) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// DirectByteBuffer 零拷贝优化 (阶段 2)
// ============================================================================

/// 创建共享内存缓冲区并返回 DirectByteBuffer
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_createSharedBufferRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jobject {
    if engine_ptr == 0 {
        return std::ptr::null_mut();
    }
    let engine_lock = &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>);
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;
    let mut env = match JNIEnv::from_raw(env_ptr) {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    // 创建共享缓冲区
    if let Some(ref mut flat_buffer) = engine.state.flat_buffer {
        let shared_ptr = flat_buffer.create_shared_buffer();
        engine.state.shared_buffer_ptr = shared_ptr;

        if !shared_ptr.is_null() {
            // 关键修复：创建后立即同步数据，防止 Java 侧看到全 0
            // 我们手动内联同步逻辑以确保在持有 guard 时完成
            let cols = engine.state.cols as usize;
            let screen_rows = engine.state.rows as usize;
            let buffer_len = engine.state.buffer.len();
            let screen_first_row = engine.state.screen_first_row;

            for logic_row in 0..screen_rows {
                let physical_row = (screen_first_row + logic_row) % buffer_len;
                if let Some(buffer_row) = engine.state.buffer.get(physical_row) {
                    let row_start_idx = logic_row * cols;
                    let row_text_len = buffer_row.text.len();
                    let mut dest_col = 0;
                    while dest_col < cols && dest_col < row_text_len {
                        let cell_idx = row_start_idx + dest_col;
                        let ucs = buffer_row.text[dest_col] as u32;
                        
                        if ucs <= 0xFFFF {
                            flat_buffer.text_data[cell_idx] = ucs as u16;
                            flat_buffer.style_data[cell_idx] = buffer_row.styles[dest_col];
                            dest_col += 1;
                        } else {
                            let u = ucs - 0x10000;
                            flat_buffer.text_data[cell_idx] = ((u >> 10) & 0x3FF) as u16 | 0xD800;
                            flat_buffer.style_data[cell_idx] = buffer_row.styles[dest_col];
                            if dest_col + 1 < cols {
                                flat_buffer.text_data[cell_idx + 1] = (u & 0x3FF) as u16 | 0xDC00;
                                flat_buffer.style_data[cell_idx + 1] = buffer_row.styles[dest_col];
                                dest_col += 2;
                            } else {
                                dest_col += 1;
                            }
                        }
                    }
                }
            }
            // 将 flat_buffer 的本地数据刷入共享内存指针
            flat_buffer.sync_to_shared(shared_ptr);

            let buffer_size =
                engine::SharedScreenBuffer::required_size(flat_buffer.cols, flat_buffer.rows);

            // 创建 DirectByteBuffer
            match env.new_direct_byte_buffer(shared_ptr as *mut u8, buffer_size) {
                Ok(buffer) => buffer.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        } else {
            std::ptr::null_mut()
        }
    } else {
        std::ptr::null_mut()
    }
}

/// 同步 Rust 数据到共享缓冲区
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_syncToSharedBufferRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>);
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;

    // 将当前屏幕数据同步到共享缓冲区
    if let Some(ref mut flat_buffer) = engine.state.flat_buffer {
        if !engine.state.shared_buffer_ptr.is_null() {
            let cols = engine.state.cols as usize;
            let screen_rows = engine.state.rows as usize; // 仅同步可见行数
            let buffer_len = engine.state.buffer.len();
            let screen_first_row = engine.state.screen_first_row;

            // 核心修复：平铺同步逻辑区 (Logic Row 0..screen_rows)
            // 映射到共享内存的 0..screen_rows
            for logic_row in 0..screen_rows {
                let physical_row = (screen_first_row + logic_row) % buffer_len;
                
                if let Some(buffer_row) = engine.state.buffer.get(physical_row) {
                    let row_start_idx = logic_row * cols; // 在 shared buffer 中的起始位置
                    let mut col = 0;
                    let row_text_len = buffer_row.text.len();

                    let mut dest_col = 0;
                    while col < row_text_len && dest_col < cols {
                        let cell_idx = row_start_idx + dest_col;
                        let ucs = buffer_row.text[col] as u32;
                        let style = buffer_row.styles[col];

                        if ucs <= 0xFFFF {
                            flat_buffer.text_data[cell_idx] = ucs as u16;
                            flat_buffer.style_data[cell_idx] = style;
                            dest_col += 1;
                        } else {
                            // 处理代理对
                            let u = ucs - 0x10000;
                            let hi = ((u >> 10) & 0x3FF) as u16 | 0xD800;
                            let lo = (u & 0x3FF) as u16 | 0xDC00;
                            
                            flat_buffer.text_data[cell_idx] = hi;
                            flat_buffer.style_data[cell_idx] = style;
                            
                            if dest_col + 1 < cols {
                                flat_buffer.text_data[cell_idx + 1] = lo;
                                flat_buffer.style_data[cell_idx + 1] = style;
                                dest_col += 2;
                            } else {
                                dest_col += 1;
                            }
                        }
                        col += 1;
                    }
                }
            }

            // 同步到共享内存指针
            flat_buffer.sync_to_shared(engine.state.shared_buffer_ptr);
        }
    }
}

/// 从共享缓冲区读取版本号 (返回 int)
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getSharedBufferVersionRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    let engine = &*guard;

    if !engine.state.shared_buffer_ptr.is_null() {
        let shared = unsafe { &*engine.state.shared_buffer_ptr };
        return shared.version as jni::sys::jint;
    }
    0
}

/// 清除共享缓冲区版本标志
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearSharedBufferVersionRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;

    if !engine.state.shared_buffer_ptr.is_null() {
        let shared = unsafe { &mut *engine.state.shared_buffer_ptr };
        shared.version = 0;
    }
}

/// 释放共享缓冲区
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroySharedBufferRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>);
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;

    if !engine.state.shared_buffer_ptr.is_null() {
        let buffer_size = engine::SharedScreenBuffer::required_size(
            engine.state.cols as usize,
            engine.state.rows as usize,
        );
        let layout = std::alloc::Layout::from_size_align(buffer_size, 8).unwrap();
        std::alloc::dealloc(engine.state.shared_buffer_ptr as *mut u8, layout);
        engine.state.shared_buffer_ptr = std::ptr::null_mut();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_resizeEngineRustFull(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    new_cols: jint,
    new_rows: jint,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>);
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;
    engine.state.resize(new_cols, new_rows);
}
/// 销毁原生引擎
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    eprintln!("[Termux-Rust] Destroying engine at ptr: {:p}", engine_ptr as *const ());
    let _ = Box::from_raw(engine_ptr as *mut std::sync::RwLock<TerminalEngine>);
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColorsFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jintArray {
    if engine_ptr == 0 {
        return std::ptr::null_mut();
    }

    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return std::ptr::null_mut(),
    };
    let engine = &*guard;

    // 创建新的 Java 数组 (259 个颜色)
    let color_count = 259;
    let j_array = match env.new_int_array(color_count as jint) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };

    // 准备数据
    let mut color_data = vec![0i32; color_count];
    for i in 0..color_count {
        color_data[i] = engine.state.colors.current_colors[i] as i32;
    }

    // 填充数据
    if env.set_int_array_region(&j_array, 0, &color_data).is_err() {
        return std::ptr::null_mut();
    }

    j_array.into_raw()
}

/// 重置颜色
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>);
    let mut guard = engine_lock.write().unwrap();
    guard.state.colors.reset();
}

/// 获取当前前景色
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getForeColorFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 256;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return 256,
    };
    guard.state.fore_color as jint
}

/// 获取当前背景色
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getBackColorFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 257;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return 257,
    };
    guard.state.back_color as jint
}

/// 获取当前效果标志
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getEffectFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    guard.state.effect as jint
}

// ============================================================================
// Full Takeover 模式 - 额外 JNI 接口
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorColFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return -1;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return -1,
    };
    guard.state.cursor_x as jint
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorRowFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return -1;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return -1,
    };
    guard.state.cursor_y as jint
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorStyleFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    guard.state.cursor_style as jint
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_shouldCursorBeVisibleFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return jni::sys::JNI_FALSE,
    };
    if guard.state.cursor_enabled && guard.state.cursor_blink_state {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isReverseVideoFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return jni::sys::JNI_FALSE,
    };
    if guard.state.reverse_video {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAlternateBufferActiveFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    let engine = &*guard;
    if engine.state.is_alternate_buffer_active() {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTitleFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jstring {
    if engine_ptr == 0 {
        return std::ptr::null_mut();
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let title = {
        let guard = match engine_lock.read() {
            Ok(g) => g,
            Err(_) => return std::ptr::null_mut(),
        };
        guard.state.title.clone()
    };

    let env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return std::ptr::null_mut(),
    };

    match title {
        Some(t) => match env.new_string(t) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_reportFocusGainFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    let engine = &*guard;
    engine.state.report_focus_gain();
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_reportFocusLossFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    let engine = &*guard;
    engine.state.report_focus_loss();
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_pasteTextFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    text: jstring,
) {
    if engine_ptr == 0 || text.is_null() {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = match engine_lock.write() {
        Ok(g) => g,
        Err(_) => return,
    };
    let engine = &mut *guard;
    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return,
    };

    if let Ok(rust_text) = env.get_string(&unsafe { jni::objects::JString::from_raw(text) }) {
        let text_str: String = rust_text.into();
        engine.state.paste(&text_str);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    guard.state.scroll_counter
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearScrollCounterFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = match engine_lock.write() {
        Ok(g) => g,
        Err(_) => return,
    };
    guard.state.clear_scroll_counter();
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAutoScrollDisabledFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    let engine = &*guard;
    if engine.state.auto_scroll_disabled {
        jni::sys::JNI_TRUE
    } else {
        jni::sys::JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_toggleAutoScrollDisabledFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;
    engine.state.toggle_auto_scroll_disabled();
}

// ==========================================
// 4. 进程管理 JNI
// ==========================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_createSubprocess(
    env_ptr: *mut jni::sys::JNIEnv,
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
    _env_ptr: *mut jni::sys::JNIEnv,
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
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    pid: jint,
) -> jint {
    pty::wait_for(pid)
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_close(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    fd: jint,
) {
    let _ = nix::unistd::close(fd);
}

// ============================================================================
// 键盘和鼠标事件处理 JNI
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendMouseEventFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    mouse_button: jint,
    column: jint,
    row: jint,
    pressed: jni::sys::jboolean,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;
    engine.state.send_mouse_event(
        mouse_button as u32,
        column as i32,
        row as i32,
        pressed != jni::sys::JNI_FALSE,
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendKeyCodeFromRust(
    env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    key_code: jint,
    key_char: jstring,
    key_mod: jint,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;
    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return,
    };

    let key_char_str = if !key_char.is_null() {
        match env.get_string(&unsafe { jni::objects::JString::from_raw(key_char) }) {
            Ok(s) => {
                let s: String = s.into();
                if s.is_empty() { None } else { Some(s) }
            }
            Err(_) => None,
        }
    } else {
        None
    };

    engine
        .state
        .send_key_event(key_code as i32, key_char_str, key_mod as i32);
}

// ============================================================================
// 滚动历史支持
// ============================================================================

/// 获取 Rust 侧滚动历史行数
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getActiveTranscriptRowsFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = match engine_lock.read() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    // 返回实际有效的滚动历史行数
    guard.state.active_transcript_rows as jint
}

/// 获取当前 DECSET 标志位
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_getDecsetFlagsFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jint {
    if engine_ptr == 0 {
        return 0;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    guard.state.decset_flags
}

/// 检查插入模式是否激活
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isInsertModeActiveFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    if guard.state.insert_mode { jni::sys::JNI_TRUE } else { jni::sys::JNI_FALSE }
}

// ============================================================================
// 光标控制相关 JNI
// ============================================================================

/// 设置光标闪烁状态
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkStateInRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    visible: jni::sys::jboolean,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    guard.state.cursor_blink_state = visible != jni::sys::JNI_FALSE;
}

/// 设置光标闪烁启用状态
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkingEnabledInRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    enabled: jni::sys::jboolean,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    guard.state.cursor_blinking_enabled = enabled != jni::sys::JNI_FALSE;
}

/// 检查光标是否启用
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorEnabledFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_TRUE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    if guard.state.cursor_enabled { jni::sys::JNI_TRUE } else { jni::sys::JNI_FALSE }
}

/// 检查光标键是否处于应用模式
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorKeysApplicationModeFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    // DECSET bit 1: 应用光标键模式
    if (guard.state.decset_flags & 0x01) != 0 { jni::sys::JNI_TRUE } else { jni::sys::JNI_FALSE }
}

/// 检查键盘是否处于应用模式
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isKeypadApplicationModeFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    // DECNKM: 数字键盘应用模式
    if guard.state.application_keypad { jni::sys::JNI_TRUE } else { jni::sys::JNI_FALSE }
}

/// 检查鼠标跟踪是否激活
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_isMouseTrackingActiveFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
) -> jni::sys::jboolean {
    if engine_ptr == 0 {
        return jni::sys::JNI_FALSE;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let guard = engine_lock.read().unwrap();
    if guard.state.mouse_tracking { jni::sys::JNI_TRUE } else { jni::sys::JNI_FALSE }
}

// ============================================================================
// 鼠标事件和客户端更新
// ============================================================================

/// 发送鼠标事件到 Rust
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendMouseEventToRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    button: jint,
    x: jint,
    y: jint,
    pressed: jni::sys::jboolean,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    let engine = &mut *guard;
    engine.state.send_mouse_event(
        button as u32,
        x as i32,
        y as i32,
        pressed != jni::sys::JNI_FALSE,
    );
}

/// 更新 TerminalSessionClient
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_updateTerminalSessionClientFromRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    _client: jobject,
) {
    // 目前只需存储引用，实际更新在 Java 侧处理
    if engine_ptr == 0 {
        return;
    }
    // 客户端引用更新已在 createEngineRustWithCallback 中处理
}

/// 设置自动滚动禁用状态
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_setAutoScrollDisabledInRust(
    _env_ptr: *mut jni::sys::JNIEnv,
    _class: jclass,
    engine_ptr: jlong,
    disabled: jni::sys::jboolean,
) {
    if engine_ptr == 0 {
        return;
    }
    let engine_lock = unsafe { &*(engine_ptr as *const std::sync::RwLock<TerminalEngine>) };
    let mut guard = engine_lock.write().unwrap();
    guard.state.auto_scroll_disabled = disabled != jni::sys::JNI_FALSE;
}
