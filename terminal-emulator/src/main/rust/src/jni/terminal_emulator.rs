use jni::JNIEnv;
use jni::objects::{JClass, JString, JObject, JValue};
use jni::sys::{jint, jlong, jbyteArray, jboolean, jintArray, jstring};
use std::sync::Arc;
use std::os::fd::FromRawFd;
use std::io::Read;

use crate::utils::{android_log, LogPriority};
use crate::engine::{TerminalEngine, TerminalContext, TerminalEvent};
use crate::terminal::colors::TerminalColors;
use crate::terminal::modes::*;
use crate::coordinator::SessionCoordinator;
use crate::render_thread;
use crate::JavaVM;

/// 将事件刷新到 Java 侧
pub fn flush_events_to_java(env: &mut JNIEnv, callback_obj: &Option<jni::objects::GlobalRef>, events: Vec<TerminalEvent>) {
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
            TerminalEvent::CopytoClipboard(text) => {
                if let Ok(j_text) = env.new_string(text) {
                    let val = JValue::from(&j_text);
                    let _ = env.call_method(obj, "onCopyTextToClipboard", "(Ljava/lang/String;)V", &[val]);
                }
            }
            TerminalEvent::TitleChanged(title) => {
                if let Ok(j_title) = env.new_string(title) {
                    let val = JValue::from(&j_title);
                    let _ = env.call_method(obj, "reportTitleChange", "(Ljava/lang/String;)V", &[val]);
                }
            }
            TerminalEvent::TerminalResponse(resp) => {
                if let Ok(j_resp) = env.new_string(resp) {
                    let val = JValue::from(&j_resp);
                    let _ = env.call_method(obj, "write", "(Ljava/lang/String;)V", &[val]);
                }
            }
            TerminalEvent::SixelImage { rgba_data, width, height, start_x, start_y } => {
                if let Ok(j_data) = env.new_byte_array(rgba_data.len() as i32) {
                    let bytes: Vec<i8> = rgba_data.iter().map(|&b| b as i8).collect();
                    let _ = env.set_byte_array_region(&j_data, 0, &bytes);
                    let args = [
                        JValue::from(&j_data),
                        JValue::from(width),
                        JValue::from(height),
                        JValue::from(start_x),
                        JValue::from(start_y),
                    ];
                    let _ = env.call_method(obj, "onSixelImage", "([BIIII)V", &args);
                }
            }
        }
    }
}

/// 创建引擎实例
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_createEngine(
    env: JNIEnv,
    _class: JClass,
    cols: jint,
    rows: jint,
    cw: jint,
    ch: jint,
    total_rows: jint,
    callback: JObject,
) -> jlong {
    android_log(LogPriority::DEBUG, &format!("JNI: createEngine ({}x{})", cols, rows));
    let mut engine = TerminalEngine::new(cols, rows, total_rows, cw, ch);
    if !callback.is_null() {
        if let Ok(global_ref) = env.new_global_ref(callback) {
            engine.state.java_callback_obj = Some(global_ref);
        }
    }
    let context = Arc::new(TerminalContext::new(engine));
    Arc::into_raw(context) as jlong
}

/// 批量处理
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_processBatch(
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
        render_thread::request_render();
    }));
    let _ = Arc::into_raw(context);
}

/// 处理 Unicode 码点
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_processCodePoint(
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
        render_thread::request_render();
    }));
    let _ = Arc::into_raw(context);
}

/// 销毁引擎
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_destroyEngine(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
        context.running.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// 启动 IO 线程
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_startIoThread(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    fd: jint,
) {
    if ptr == 0 { return; }

    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let context_thread = Arc::clone(&context);

    let pty_fd = unsafe { libc::dup(fd) };
    if pty_fd < 0 {
        let _ = Arc::into_raw(context);
        return;
    }

    std::thread::Builder::new()
        .name("Engine".to_string())
        .spawn(move || {
            let context = context_thread;
            let thread_name = std::ffi::CString::new("Engine").unwrap();
            unsafe {
                libc::prctl(libc::PR_SET_NAME, thread_name.as_ptr(), 0, 0, 0);
                libc::pthread_setname_np(libc::pthread_self(), thread_name.as_ptr());
            }

            android_log(LogPriority::INFO, " IO Thread started");

            let mut attached_env: Option<JNIEnv> = crate::JAVA_VM.get().and_then(|vm: &JavaVM| {
                vm.attach_current_thread_as_daemon().ok()
            });

            let mut file = unsafe { std::fs::File::from_raw_fd(pty_fd) };
            let mut buffer = [0u8; 8192];

            while context.running.load(std::sync::atomic::Ordering::SeqCst) {
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let (events, cb) = {
                            let mut engine = context.lock.write().unwrap();
                            engine.process_bytes(&buffer[..n]);
                            (engine.take_events(), engine.state.java_callback_obj.clone())
                        };
                        render_thread::request_render();
                        if let Some(ref mut env) = attached_env {
                            flush_events_to_java(env, &cb, events);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            android_log(LogPriority::INFO, " IO Thread stopped");
        })
        .expect("Failed to spawn  IO thread");
    let _ = Arc::into_raw(context);
}

/// 完整调整大小
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_resize(
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
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

/// 获取标题
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getTitle(
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

/// 获取光标行
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCursorRow(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.y as jint
    };
    let _ = Arc::into_raw(context);
    result
}

/// 获取光标列
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCursorCol(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.x as jint
    };
    let _ = Arc::into_raw(context);
    result
}

/// 获取光标样式
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCursorStyle(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.style as jint
    };
    let _ = Arc::into_raw(context);
    result
}

/// 设置光标样式
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorStyle(mut env: JNIEnv, _class: JClass, ptr: jlong, cursor_style: jint) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.style = cursor_style as i32;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

/// DECSET/DECRST
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_doDecSetOrReset(mut env: JNIEnv, _class: JClass, ptr: jlong, setting: jboolean, mode: jint) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            engine.state.do_decset_or_reset(setting != 0, mode as u32);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        render_thread::request_render();
        flush_events_to_java(&mut env, &cb, events);
    }));
    let _ = Arc::into_raw(context);
}

/// 光标可见性检查
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_shouldCursorBeVisible(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isCursorEnabled(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isReverseVideo(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isAlternateBufferActive(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isCursorKeysApplicationMode(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isKeypadApplicationMode(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isMouseTrackingActive(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isInsertModeActive(_env: JNIEnv, _class: JClass, _ptr: jlong) -> jboolean { 0 }

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getScrollCounter(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getRows(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCols(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cols as jint
    };
    let _ = Arc::into_raw(context);
    result
}

/// 读取行数据
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_readRow(
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

/// 获取选中文本
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getSelectedText(
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

/// 获取单词
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getWordAtLocation(
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

/// 获取历史记录文本
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getTranscriptText(
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

/// 清除滚动计数器
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_clearScrollCounter(mut env: JNIEnv, _class: JClass, ptr: jlong) {
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

/// 自动滚动设置
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isAutoScrollDisabled(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_RustTerminal_toggleAutoScrollDisabled(mut env: JNIEnv, _class: JClass, ptr: jlong) {
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

/// 鼠标事件
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_sendMouseEvent(mut env: JNIEnv, _class: JClass, ptr: jlong, button: jint, col: jint, row: jint, pressed: jboolean) {
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

/// 按键码处理
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_sendKeyCode(
    mut env: JNIEnv, _class: JClass, ptr: jlong, key_code: jint, char_str: jstring, meta_state: jint,
) -> jstring {
    if ptr == 0 { return std::ptr::null_mut(); }
    let rust_str = if !char_str.is_null() {
        let j_str = unsafe { JString::from_raw(char_str) };
        env.get_string(&j_str).ok().map(|s| String::from(s)).unwrap_or_default()
    } else { String::new() };
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };

    let seq = {
        let mut engine = context.lock.write().unwrap();
        engine.state.send_key_event(key_code, Some(rust_str), meta_state)
    };
    let _ = Arc::into_raw(context);

    match seq {
        Some(s) => match env.new_string(s) {
            Ok(j_str) => j_str.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

/// 粘贴文本
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_pasteText(
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

/// 获取活动历史记录行数
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getActiveTranscriptRows(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.get_current_screen().active_transcript_rows as jint
    };
    let _ = Arc::into_raw(context);
    result
}

/// 获取颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getColors(env: JNIEnv, _class: JClass, ptr: jlong) -> jintArray {
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

/// 重置颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_resetColors(mut env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };

    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.colors.reset();
        let mut events = engine.take_events();
        events.push(TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };

    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

/// 更新颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_updateColors(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    properties_obj: JObject,
) {
    if ptr == 0 || properties_obj.is_null() { return; }

    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };

    let props_map = {
        let mut map = std::collections::HashMap::new();

        if let Ok(entry_set) = env.call_method(&properties_obj, "entrySet", "()Ljava/util/Set;", &[]) {
            if let Ok(entry_set_obj) = entry_set.l() {
                if let Ok(iterator) = env.call_method(&entry_set_obj, "iterator", "()Ljava/util/Iterator;", &[]) {
                    if let Ok(iter_obj) = iterator.l() {
                        loop {
                            if let Ok(has_next) = env.call_method(&iter_obj, "hasNext", "()Z", &[]) {
                                if let Ok(has_next_val) = has_next.z() {
                                    if !has_next_val { break; }
                                } else { break; }
                            } else { break; }

                            if let Ok(entry) = env.call_method(&iter_obj, "next", "()Ljava/lang/Object;", &[]) {
                                if let Ok(entry_obj) = entry.l() {
                                    if let Ok(key) = env.call_method(&entry_obj, "getKey", "()Ljava/lang/Object;", &[]) {
                                        if let Ok(key_obj) = key.l() {
                                            if let Ok(value) = env.call_method(&entry_obj, "getValue", "()Ljava/lang/Object;", &[]) {
                                                if let Ok(value_obj) = value.l() {
                                                    let key_jstring = jni::objects::JString::from(key_obj);
                                                    let value_jstring = jni::objects::JString::from(value_obj);

                                                    if let (Ok(key_rust), Ok(value_rust)) = (
                                                        env.get_string(&key_jstring),
                                                        env.get_string(&value_jstring)
                                                    ) {
                                                        map.insert(key_rust.to_string_lossy().to_string(), value_rust.to_string_lossy().to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        map
    };

    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();

        if let Err(e) = engine.state.colors.update_with_properties(&props_map) {
            android_log(LogPriority::WARN, &format!("Failed to update colors: {}", e));
        }

        let mut events = engine.take_events();
        events.push(TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };

    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

/// 设置光标颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorColorForBackground(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 { return; }

    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };

    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.colors.set_cursor_color_for_background();

        let mut events = engine.take_events();
        events.push(TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };

    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

/// 获取感知亮度
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getPerceivedBrightnessOfColor(
    _env: JNIEnv,
    _class: JClass,
    color: jint,
) -> jint {
    let color_u32 = color as u32;
    TerminalColors::get_perceived_brightness(color_u32) as jint
}

/// 更新终端会话客户端
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_updateTerminalSessionClient(
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

/// 设置光标闪烁状态
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorBlinkState(mut env: JNIEnv, _class: JClass, ptr: jlong, state: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.blink_state = state != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorBlinkingEnabled(mut env: JNIEnv, _class: JClass, ptr: jlong, enabled: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.blinking_enabled = enabled != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

/// 获取调试信息
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getDebugInfo(
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
// PTY 处理 (JNI.java)
// ============================================================================

/// 设置 PTY 窗口大小
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

/// 创建异步会话
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
        let coordinator = SessionCoordinator::get();
        let session_id = coordinator.register_session();

        let pty_res = crate::pty::create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, cols, cw, ch);
        let (pty_fd, pid) = match pty_res {
            Ok(res) => res,
            Err(_) => {
                coordinator.unregister_session(session_id);
                return;
            }
        };

        let mut engine = TerminalEngine::new(cols, rows, transcript_rows, cw, ch);
        if let Some(ref cb) = callback_ref {
            engine.state.java_callback_obj = Some(cb.clone());
        }

        let context = Arc::new(TerminalContext::new(engine));
        let context_ptr = Arc::into_raw(context.clone());

        context.start_io_thread(pty_fd);

        if let Some(ref cb) = callback_ref {
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let mut env: JNIEnv = env;
                    let _ = env.call_method(
                        cb.as_obj(),
                        "onEngineInitialized",
                        "(JII)V",
                        &[
                            JValue::from(context_ptr as jlong),
                            JValue::from(pty_fd),
                            JValue::from(pid),
                        ],
                    );
                }
            }
        }
    });
}

/// 等待进程
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_waitFor(
    _env: JNIEnv,
    _class: JClass,
    pid: jint,
) -> jint {
    crate::pty::wait_for(pid)
}

/// 关闭 FD
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_close(
    _env: JNIEnv,
    _class: JClass,
    fd: jint,
) {
    unsafe { libc::close(fd); }
}

/// 创建子进程
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_createSubprocess(
    mut env: JNIEnv,
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
