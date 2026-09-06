use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jbyteArray, jint, jintArray, jlong, jstring};
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::Arc;

use crate::coordinator::SessionCoordinator;
use crate::engine::{TerminalContext, TerminalEngine, TerminalEvent};
use crate::render_thread;
use crate::terminal::colors::TerminalColors;
use crate::terminal::modes::*;
use crate::utils::{LogPriority, android_log};

/// Rolls back registry ownership if async delivery fails or unwinds. Reader
/// cancellation/join remains a separate IO contract; revocation is not a join.
struct PendingEngineDelivery {
    handle: jlong,
    delivered: bool,
}

impl Drop for PendingEngineDelivery {
    fn drop(&mut self) {
        if !self.delivered {
            crate::engine::destroy_engine(self.handle);
        }
    }
}

/// 将事件刷新到 Java 侧
pub fn flush_events_to_java(
    env: &mut JNIEnv,
    callback_obj: &Option<jni::objects::GlobalRef>,
    events: Vec<TerminalEvent>,
) {
    if events.is_empty() {
        return;
    }
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
                    let _ = env.call_method(
                        obj,
                        "onCopyTextToClipboard",
                        "(Ljava/lang/String;)V",
                        &[val],
                    );
                }
            }
            TerminalEvent::TitleChanged(title) => {
                if let Ok(j_title) = env.new_string(title) {
                    let val = JValue::from(&j_title);
                    let _ =
                        env.call_method(obj, "reportTitleChange", "(Ljava/lang/String;)V", &[val]);
                }
            }
            TerminalEvent::TerminalResponse(resp) => {
                if let Ok(j_resp) = env.new_string(resp) {
                    let val = JValue::from(&j_resp);
                    let _ = env.call_method(obj, "write", "(Ljava/lang/String;)V", &[val]);
                }
            }
            TerminalEvent::SixelImage {
                rgba_data,
                width,
                height,
                start_x,
                start_y,
            } => {
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
    android_log(
        LogPriority::DEBUG,
        &format!("JNI: createEngine ({}x{})", cols, rows),
    );
    let mut engine = TerminalEngine::new(0, cols, rows, total_rows, cw, ch);
    if !callback.is_null() {
        if let Ok(global_ref) = env.new_global_ref(callback) {
            engine.state.java_callback_obj = Some(global_ref);
        }
    }
    let context = Arc::new(TerminalContext::new(engine));
    crate::engine::ENGINE_HANDLES.insert(context).unwrap_or(0)
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
    if ptr == 0 || batch.is_null() {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = crate::safe_write!(context.lock);
            let j_array = unsafe { jni::objects::JByteArray::from_raw(batch) };
            if let Ok(bytes) = env.convert_byte_array(&j_array) {
                let len = length as usize;
                let actual_len = std::cmp::min(len, bytes.len());
                engine.process_bytes(&bytes[..actual_len]);

                // 记录吞吐量性能指标
                crate::utils::METRICS.record_bytes(actual_len as u64);
                crate::utils::METRICS.try_report();
            }
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
        render_thread::request_render();
    }));
}

/// Enqueue a complete input slice. 0=accepted, -1=closed, -2=full, -3=invalid.
/// Acceptance is not delivery: explicit cancellation can discard pending bytes.
fn enqueue_input(
    env: &JNIEnv,
    context: &TerminalContext,
    data: jbyteArray,
    offset: jint,
    count: jint,
) -> jint {
    if data.is_null() || offset < 0 || count < 0 {
        return -3;
    }
    let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
    let Ok(bytes) = env.convert_byte_array(&j_array) else {
        return -3;
    };
    let Some(end) = (offset as usize).checked_add(count as usize) else {
        return -3;
    };
    let Some(slice) = bytes.get(offset as usize..end) else {
        return -3;
    };
    match context.submit_input(slice) {
        Ok(()) => 0,
        Err(crate::engine::io_runtime::SubmitError::Closed) => -1,
        Err(crate::engine::io_runtime::SubmitError::Full) => -2,
    }
}

/// Legacy JVM signature retained; rejection is logged, never a blocking write.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_processInput(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
    offset: jint,
    count: jint,
) {
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let status = enqueue_input(&env, &context, data, offset, count);
    if status != 0 {
        android_log(
            LogPriority::WARN,
            &format!("PTY input rejected: status={status}"),
        );
    }
}

/// Status-returning API for callers that must report bounded-queue rejection.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_tryProcessInput(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
    offset: jint,
    count: jint,
) -> jint {
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return -1;
    };
    enqueue_input(&env, &context, data, offset, count)
}

/// 启动 IO 线程
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_startIoThread(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    pty_fd: jint,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    if pty_fd < 0 {
        return;
    }
    // Preserve the legacy startIoThread ownership-transfer contract.
    let owned = unsafe { OwnedFd::from_raw_fd(pty_fd) };
    if let Err(error) = TerminalContext::start_io_owned(Arc::clone(&context), owned) {
        android_log(
            LogPriority::ERROR,
            &format!("startIoThread failed: {error}"),
        );
    }
}

/// 销毁引擎实例
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_destroyEngine(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 {
        return;
    }
    crate::engine::destroy_engine(ptr);
}

/// 处理 Unicode 码点
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_processCodePoint(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    code_point: jint,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = crate::safe_write!(context.lock);
            engine.process_code_point(code_point as u32);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
        render_thread::request_render();
    }));
}

/// 设置历史记录行数
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setTranscriptRows(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    rows: jint,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.main_screen.resize_transcript(rows as usize);
    }
}

/// 处理尺寸调整
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_resize(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cols: jint,
    rows: jint,
    cw: jint,
    ch: jint,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    context.resize_pty(rows, cols, cw, ch);
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.resize(cols, rows);
        engine.events.push(TerminalEvent::ScreenUpdated);
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
}

/// 获取标题
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getTitle(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jstring {
    if ptr == 0 {
        return std::ptr::null_mut();
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return std::ptr::null_mut();
    };
    let title = {
        let engine = crate::safe_read!(context.lock);
        engine.state.title.clone().unwrap_or_default()
    };
    let result = if let Ok(j_str) = env.new_string(title) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    };
    drop(context);
    result
}

/// 获取光标行
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCursorRow(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        engine.state.cursor.y as jint
    };
    drop(context);
    result
}

/// 获取光标列
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCursorCol(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        engine.state.cursor.x as jint
    };
    drop(context);
    result
}

/// 获取光标样式
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCursorStyle(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        engine.state.cursor.style as jint
    };
    drop(context);
    result
}

/// 设置光标样式
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorStyle(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cursor_style: jint,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.cursor.style = cursor_style as i32;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
}

/// DECSET/DECRST
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_doDecSetOrReset(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    setting: jboolean,
    mode: jint,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = crate::safe_write!(context.lock);
            engine.state.do_decset_or_reset(setting != 0, mode as u32);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        render_thread::request_render();
        flush_events_to_java(&mut env, &cb, events);
    }));
}

/// 光标可见性检查
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_shouldCursorBeVisible(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine
            .state
            .cursor
            .should_be_visible(engine.state.cursor_enabled)
        {
            1
        } else {
            0
        }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isCursorEnabled(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine.state.cursor_enabled { 1 } else { 0 }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isReverseVideo(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO) {
            1
        } else {
            0
        }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isAlternateBufferActive(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine.state.use_alternate_buffer {
            1
        } else {
            0
        }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isCursorKeysApplicationMode(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine.state.application_cursor_keys {
            1
        } else {
            0
        }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isKeypadApplicationMode(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine.state.modes.is_enabled(DECSET_BIT_APPLICATION_KEYPAD) {
            1
        } else {
            0
        }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isMouseTrackingActive(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine.state.mouse_tracking { 1 } else { 0 }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isInsertModeActive(
    _env: JNIEnv,
    _class: JClass,
    _ptr: jlong,
) -> jboolean {
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getScrollCounter(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        engine.state.scroll_counter as jint
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getRows(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        engine.state.rows as jint
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCols(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        engine.state.cols as jint
    };
    drop(context);
    result
}

/// 读取行数据
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_readRow(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    row: jint,
    text: jni::sys::jintArray,
    styles: jni::sys::jlongArray,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (text_buf, style_buf) = {
        let engine = crate::safe_read!(context.lock);
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
}

/// 获取选中文本
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getSelectedText(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    x1: jint,
    y1: jint,
    x2: jint,
    y2: jint,
) -> jstring {
    if ptr == 0 {
        return std::ptr::null_mut();
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return std::ptr::null_mut();
    };

    // 规范化坐标：确保 (x1, y1) 在 (x2, y2) 之前
    let (real_x1, real_y1, real_x2, real_y2) = if y1 < y2 || (y1 == y2 && x1 <= x2) {
        (x1, y1, x2, y2)
    } else {
        (x2, y2, x1, y1)
    };

    let text = {
        let engine = crate::safe_read!(context.lock);
        engine
            .state
            .get_current_screen()
            .get_selected_text(real_x1, real_y1, real_x2, real_y2)
    };
    let result = if let Ok(j_str) = env.new_string(text) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    };
    drop(context);
    result
}

/// 获取单词
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getWordAtLocation(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    x: jint,
    y: jint,
) -> jstring {
    if ptr == 0 {
        return std::ptr::null_mut();
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return std::ptr::null_mut();
    };
    let text = {
        let engine = crate::safe_read!(context.lock);
        engine
            .state
            .get_current_screen()
            .get_row(y)
            .get_word_at(x as usize)
    };
    let result = if let Ok(j_str) = env.new_string(text) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    };
    drop(context);
    result
}

/// 获取历史记录文本
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getTranscriptText(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jstring {
    if ptr == 0 {
        return std::ptr::null_mut();
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return std::ptr::null_mut();
    };
    let text = {
        let engine = crate::safe_read!(context.lock);
        engine.state.get_current_screen().get_transcript_text()
    };
    let result = if let Ok(j_str) = env.new_string(text) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    };
    drop(context);
    result
}

/// 清除滚动计数器
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_clearScrollCounter(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.scroll_counter = 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
}

/// 自动滚动设置
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_isAutoScrollDisabled(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jboolean {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        if engine.state.auto_scroll_disabled {
            1
        } else {
            0
        }
    };
    drop(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_toggleAutoScrollDisabled(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.auto_scroll_disabled = !engine.state.auto_scroll_disabled;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
}

/// 鼠标事件
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_sendMouseEvent(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    button: jint,
    col: jint,
    row: jint,
    pressed: jboolean,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine
            .state
            .send_mouse_event(button as u32, col, row, pressed != 0);
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    flush_events_to_java(&mut env, &cb, events);
}

/// 按键码处理
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_sendKeyCode(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    key_code: jint,
    char_str: jstring,
    meta_state: jint,
) -> jstring {
    if ptr == 0 {
        return std::ptr::null_mut();
    }
    let rust_str = if !char_str.is_null() {
        let j_str = unsafe { JString::from_raw(char_str) };
        env.get_string(&j_str)
            .ok()
            .map(|s| String::from(s))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return std::ptr::null_mut();
    };

    let seq = {
        let mut engine = crate::safe_write!(context.lock);
        engine
            .state
            .send_key_event(key_code, Some(rust_str), meta_state)
    };

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
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    text: jstring,
) {
    if ptr == 0 {
        return;
    }
    let rust_str = if !text.is_null() {
        let j_str = unsafe { JString::from_raw(text) };
        env.get_string(&j_str).ok().map(|s| String::from(s))
    } else {
        None
    };

    if let Some(s) = rust_str {
        let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
            return;
        };
        let (events, cb) = {
            let mut engine = crate::safe_write!(context.lock);
            engine.state.paste(&s);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
    }
}

/// 获取活动历史记录行数
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getActiveTranscriptRows(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    if ptr == 0 {
        return 0;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return 0;
    };
    let result = {
        let engine = crate::safe_read!(context.lock);
        engine.state.get_current_screen().active_transcript_rows as jint
    };
    drop(context);
    result
}

/// 获取颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getColors(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jintArray {
    if ptr == 0 {
        return std::ptr::null_mut();
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return std::ptr::null_mut();
    };
    let colors = {
        let engine = crate::safe_read!(context.lock);
        engine.state.colors.current_colors
    };

    let result = if let Ok(j_array) = env.new_int_array(colors.len() as jint) {
        unsafe {
            let _ = env.set_int_array_region(
                &j_array,
                0,
                std::mem::transmute::<&[u32], &[i32]>(&colors),
            );
        }
        j_array.into_raw()
    } else {
        std::ptr::null_mut()
    };
    drop(context);
    result
}

/// 重置颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_resetColors(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };

    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.colors.reset();
        let mut events = engine.take_events();
        events.push(TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };

    flush_events_to_java(&mut env, &cb, events);
}

/// 更新颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_updateColors(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    properties_obj: JObject,
) {
    if ptr == 0 || properties_obj.is_null() {
        return;
    }

    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };

    let props_map = {
        let mut map = std::collections::HashMap::new();

        if let Ok(entry_set) =
            env.call_method(&properties_obj, "entrySet", "()Ljava/util/Set;", &[])
        {
            if let Ok(entry_set_obj) = entry_set.l() {
                if let Ok(iterator) =
                    env.call_method(&entry_set_obj, "iterator", "()Ljava/util/Iterator;", &[])
                {
                    if let Ok(iter_obj) = iterator.l() {
                        loop {
                            if let Ok(has_next) = env.call_method(&iter_obj, "hasNext", "()Z", &[])
                            {
                                if let Ok(has_next_val) = has_next.z() {
                                    if !has_next_val {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }

                            if let Ok(entry) =
                                env.call_method(&iter_obj, "next", "()Ljava/lang/Object;", &[])
                            {
                                if let Ok(entry_obj) = entry.l() {
                                    if let Ok(key) = env.call_method(
                                        &entry_obj,
                                        "getKey",
                                        "()Ljava/lang/Object;",
                                        &[],
                                    ) {
                                        if let Ok(key_obj) = key.l() {
                                            if let Ok(value) = env.call_method(
                                                &entry_obj,
                                                "getValue",
                                                "()Ljava/lang/Object;",
                                                &[],
                                            ) {
                                                if let Ok(value_obj) = value.l() {
                                                    let key_jstring =
                                                        jni::objects::JString::from(key_obj);
                                                    let value_jstring =
                                                        jni::objects::JString::from(value_obj);

                                                    if let (Ok(key_rust), Ok(value_rust)) = (
                                                        env.get_string(&key_jstring),
                                                        env.get_string(&value_jstring),
                                                    ) {
                                                        map.insert(
                                                            key_rust.to_string_lossy().to_string(),
                                                            value_rust
                                                                .to_string_lossy()
                                                                .to_string(),
                                                        );
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
        let mut engine = crate::safe_write!(context.lock);

        if let Err(e) = engine.state.colors.update_with_properties(&props_map) {
            android_log(
                LogPriority::WARN,
                &format!("Failed to update colors: {}", e),
            );
        }

        let mut events = engine.take_events();
        events.push(TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };

    flush_events_to_java(&mut env, &cb, events);
}

/// 设置光标颜色
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorColorForBackground(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 {
        return;
    }

    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };

    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.colors.set_cursor_color_for_background();

        let mut events = engine.take_events();
        events.push(TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };

    flush_events_to_java(&mut env, &cb, events);
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
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
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
}

/// 设置光标闪烁状态
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorBlinkState(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    state: jboolean,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.cursor.blink_state = state != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_setCursorBlinkingEnabled(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    enabled: jboolean,
) {
    if ptr == 0 {
        return;
    }
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return;
    };
    let (events, cb) = {
        let mut engine = crate::safe_write!(context.lock);
        engine.state.cursor.blinking_enabled = enabled != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    render_thread::request_render();
    flush_events_to_java(&mut env, &cb, events);
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
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        let empty = env.new_string("TerminalEmulator[destroyed]").ok();
        return empty.map_or(std::ptr::null_mut(), |s| s.into_raw());
    };
    let debug_info = {
        let engine = crate::safe_read!(context.lock);
        engine.state.get_debug_info()
    };
    let result = if let Ok(j_str) = env.new_string(debug_info) {
        j_str.into_raw()
    } else {
        std::ptr::null_mut()
    };
    drop(context);
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
    session_id: jint,
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

    // Null callback selects the legacy polling delivery API. A non-null
    // callback selects push delivery; never publish the same owner twice.
    let poll_delivery = callback.is_null();
    let callback_ref = if !poll_delivery {
        env.new_global_ref(callback).ok()
    } else {
        None
    };

    if !poll_delivery && callback_ref.is_none() {
        android_log(
            LogPriority::ERROR,
            "createSessionAsync: callback global reference failed",
        );
        return;
    }

    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(move || {
            crate::utils::android_log(
                crate::utils::LogPriority::INFO,
                "[TRACE_SESSION] 5.1. Background thread started in Rust",
            );
            let coordinator = SessionCoordinator::get();
            let session_id = session_id as usize;
            if !coordinator.has_session(session_id) {
                return;
            }

            let pty_res = crate::pty::create_subprocess_with_data(
                cmd_str, cwd_str, argv, envp, rows, cols, cw, ch,
            );
            let (pty_fd, pid) = match pty_res {
                Ok(res) => {
                    crate::utils::android_log(
                        crate::utils::LogPriority::DEBUG,
                        &format!(
                            "[TRACE_SESSION] 5.2. PTY created (fd={}, pid={})",
                            res.0, res.1
                        ),
                    );
                    res
                }
                Err(_) => {
                    crate::utils::android_log(
                        crate::utils::LogPriority::ERROR,
                        "[TRACE_SESSION] 5.2. FAILED to create PTY",
                    );
                    return;
                }
            };

            let owned = unsafe { OwnedFd::from_raw_fd(pty_fd) };
            let process = match coordinator.bind_pty_child(session_id, pid) {
                Ok(process) => process,
                Err(error) => {
                    android_log(
                        LogPriority::ERROR,
                        &format!("async child bind rejected: {error}"),
                    );
                    // bind_pty_child owns rejection cleanup. Re-claiming this
                    // numeric PID here after cleanup could target a reused PID.
                    return;
                }
            };

            crate::utils::android_log(
                crate::utils::LogPriority::DEBUG,
                "[TRACE_SESSION] 5.3. Creating TerminalEngine",
            );
            let mut engine =
                TerminalEngine::new(session_id as i32, cols, rows, transcript_rows, cw, ch);
            if let Some(ref cb) = callback_ref {
                engine.state.java_callback_obj = Some(cb.clone());
            }

            let context = Arc::new(TerminalContext::with_process(engine, process));
            let Some(context_handle) = crate::engine::ENGINE_HANDLES.insert(Arc::clone(&context))
            else {
                // The unpublished OwnedFd is closed by its destructor.
                return;
            };
            let mut pending = PendingEngineDelivery {
                handle: context_handle,
                delivered: false,
            };

            // Transfer the original fd, not a duplicate. Java receives metadata
            // only; production resize and input route through the live handle.
            if let Err(error) = TerminalContext::start_io_owned(Arc::clone(&context), owned) {
                android_log(
                    LogPriority::ERROR,
                    &format!("createSessionAsync IO start failed: {error}"),
                );
                return;
            }

            if poll_delivery {
                coordinator.set_engine_data(
                    session_id,
                    crate::coordinator::SessionEngineData {
                        ptr: context_handle,
                        pty_fd: pty_fd as i32,
                        pid: pid as i32,
                    },
                );
                // The coordinator owns this unclaimed delivery until poll or
                // unregister. Callback delivery does not also populate the map.
                pending.delivered = true;
                return;
            }
            crate::utils::android_log(
                crate::utils::LogPriority::INFO,
                &format!(
                    "[TRACE_SESSION] 5.5. Engine data registered for session {}. SUCCESS.",
                    session_id
                ),
            );

            // 主动回调 Java 通知初始化完成
            if let Some(ref cb) = callback_ref {
                if let Some(vm) = crate::JAVA_VM.get() {
                    if let Ok(mut env) = vm.attach_current_thread() {
                        let delivered = env.call_method(
                            cb.as_obj(),
                            "onEngineInitialized",
                            "(JII)V",
                            &[
                                jni::objects::JValue::Long(context_handle),
                                jni::objects::JValue::Int(pty_fd as i32),
                                jni::objects::JValue::Int(pid as i32),
                            ],
                        );
                        pending.delivered = delivered.is_ok();
                        if pending.delivered {
                            crate::utils::android_log(
                                crate::utils::LogPriority::INFO,
                                "[TRACE_SESSION] 5.6. Java onEngineInitialized callback executed.",
                            );
                        } else {
                            android_log(
                                LogPriority::ERROR,
                                "createSessionAsync: callback rejected",
                            );
                        }
                    }
                }
            }
        });

        if let Err(e) = result {
            crate::utils::android_log(
                crate::utils::LogPriority::ERROR,
                &format!("CRITICAL: Rust background thread PANICKED: {:?}", e),
            );
        }
    });
}

/// 创建子进程 (JNI.java 同步调用版)
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_createSubprocess(
    mut env: JNIEnv,
    _class: JClass,
    cmd: jstring,
    cwd: jstring,
    args: jni::sys::jobjectArray,
    env_vars: jni::sys::jobjectArray,
    process_id_array: jni::sys::jintArray,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
) -> jint {
    let cmd_str = if !cmd.is_null() {
        env.get_string(&unsafe { JString::from_raw(cmd) })
            .map(|s| s.into())
            .unwrap_or_default()
    } else {
        String::new()
    };

    let cwd_str = if !cwd.is_null() {
        env.get_string(&unsafe { JString::from_raw(cwd) })
            .map(|s| s.into())
            .unwrap_or_default()
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

    let pty_res =
        crate::pty::create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, cols, cw, ch);
    match pty_res {
        Ok((pty_fd, pid)) => {
            let p_pid = [pid as jint];
            let array = unsafe { jni::objects::JIntArray::from_raw(process_id_array) };
            let _ = env.set_int_array_region(&array, 0, &p_pid);
            pty_fd as jint
        }
        Err(_) => -1,
    }
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
/// 注意：Android 10+ 引入了 fdsan，如果 FD 被 Java 侧的 ParcelFileDescriptor 拥有，
/// 直接在此处 libc::close 会导致进程崩溃 (SIGABRT)。
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_JNI_close(
    _env: JNIEnv,
    _class: JClass,
    fd: jint,
) {
    if fd < 0 {
        return;
    }
    // 暂时禁用 Rust 侧的主动关闭，交由 Java 侧的 ParcelFileDescriptor 处理生命周期，
    // 或者使用更安全的 dup/detach 策略。这是解决 [fdsan_error] 崩溃的关键。
    crate::utils::android_log(
        crate::utils::LogPriority::DEBUG,
        &format!(
            "JNI: close(fd={}) called from Java. (fdsan bypass: skipping direct libc::close)",
            fd
        ),
    );
    // unsafe { libc::close(fd); }
}

/// Observe process and IO terminal facts without changing their ownership.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_RustTerminal_getCompletionStatus(
    env: JNIEnv,
    _class: JClass,
    ptr: jni::sys::jlong,
) -> jni::sys::jintArray {
    let Some(context) = crate::engine::ENGINE_HANDLES.acquire(ptr) else {
        return std::ptr::null_mut();
    };
    let status = context.completion_status();
    let Ok(array) = env.new_int_array(4) else {
        return std::ptr::null_mut();
    };
    if env.set_int_array_region(&array, 0, &status).is_err() {
        return std::ptr::null_mut();
    }
    array.into_raw()
}
