/// JNI 绑定模块
/// 
/// 包含所有 Java JNI 接口函数：
/// - TerminalView JNI 函数
/// - TerminalEmulator JNI 函数

use jni::JNIEnv;
use jni::objects::{JClass, JString, JObject};
use jni::sys::{jint, jlong, jbyteArray, jboolean, jintArray, jstring, jfloat};
use std::sync::Arc;
use std::os::fd::FromRawFd;
use std::io::Read;

use crate::utils::{android_log, LogPriority};
use crate::engine::{TerminalEngine, TerminalContext, TerminalEvent};
use crate::terminal::colors::TerminalColors;
use crate::terminal::modes::*;
use crate::coordinator::SessionCoordinator;
use crate::render_thread;

/// 设置渲染参数
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeUpdateRenderParams(
    _env: JNIEnv,
    _obj: JObject,
    scale: jfloat,
    scroll_offset: jfloat,
    top_row: jint,
    sel_x1: jint,
    sel_y1: jint,
    sel_x2: jint,
    sel_y2: jint,
    sel_active: jboolean,
) {
    let mut params = render_thread::get_render_params().lock().unwrap();
    params.scale = scale;
    params.scroll_offset = scroll_offset;
    params.top_row = top_row;
    params.sel_x1 = sel_x1;
    params.sel_y1 = sel_y1;
    params.sel_x2 = sel_x2;
    params.sel_y2 = sel_y2;
    params.sel_active = sel_active != 0;
    drop(params);
    render_thread::request_render();
}

/// 设置字体尺寸
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetFontSize(
    _env: JNIEnv,
    _obj: JObject,
    font_size: jfloat,
) {
    let old_size = *render_thread::get_render_font_size().lock().unwrap();
    *render_thread::get_render_font_size().lock().unwrap() = font_size;
    android_log(LogPriority::DEBUG, &format!("nativeSetFontSize: {} -> {}", old_size, font_size));
    render_thread::request_render();
}

/// 设置自定义字体文件路径
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetFontPath(
    mut env: JNIEnv,
    _obj: JObject,
    path: JString,
) {
    if let Ok(path_str) = env.get_string(&path) {
        let path_str: String = path_str.into();
        render_thread::set_render_font_path(&path_str);
        android_log(LogPriority::DEBUG, &format!("nativeSetFontPath: {}", path_str));
        render_thread::request_render();
    }
}

/// 获取字体指标
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeGetFontMetrics(
    env: JNIEnv,
    _obj: JObject,
    metrics_array: jni::sys::jfloatArray,
) {
    let font_size = *render_thread::get_render_font_size().lock().unwrap();
    let font_path = render_thread::get_render_font_path();
    let safe_font_size = if font_size > 0.0 && font_size.is_finite() { font_size } else { 12.0 };

    use skia_safe::{Font, FontMgr, FontStyle, Data};

    let font_mgr = FontMgr::new();

    // Try custom font first
    let custom_typeface = font_path.as_ref().and_then(|path| {
        std::fs::read(path).ok().map(|data| {
            let font_data = Data::new_copy(&data);
            font_mgr.new_from_data(&font_data, 0)
        }).flatten()
    });

    let (font_width, font_height, font_ascent) = match custom_typeface
        .or_else(|| font_mgr.match_family_style("monospace", FontStyle::normal()))
    {
        Some(tf) => {
            let font = Font::new(tf, Some(safe_font_size));
            let metrics = font.metrics();
            let h = (metrics.1.descent - metrics.1.ascent + metrics.1.leading).ceil();
            let (w, _) = font.measure_str("M", None);
            (w, h, metrics.1.ascent)
        }
        None => {
            (safe_font_size * 0.6, safe_font_size * 1.2, -safe_font_size * 0.8)
        }
    };

    let values = [font_width, font_height, font_ascent];
    unsafe {
        let j_array = jni::objects::JFloatArray::from_raw(metrics_array);
        let _ = env.set_float_array_region(&j_array, 0, &values);
    }
}

/// 设置 Surface
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetSurface(
    env: JNIEnv,
    _obj: JObject,
    surface: JObject,
) {
    #[cfg(target_os = "android")]
    {
        if surface.is_null() {
            let start_time = std::time::Instant::now();
            android_log(LogPriority::WARN, "CHECKPOINT: nativeSetSurface(null) ENTERED - Surface being destroyed");
            
            render_thread::get_surface_ready().store(false, std::sync::atomic::Ordering::SeqCst);
            render_thread::get_render_thread_running().store(false, std::sync::atomic::Ordering::SeqCst);
            
            android_log(LogPriority::DEBUG, "CHECKPOINT: Flags cleared, breaking potential driver blocks...");

            // 在 join 之前，通过 Mutex 获取上下文并强制 Abandon。
            // 这一步能解除渲染线程可能在驱动内部（如 queue_present）的阻塞。
            if let Some(mutex) = render_thread::get_vulkan_context().get() {
                if let Ok(mut guard) = mutex.try_lock() {
                    if let Some(ctx) = guard.as_mut() {
                        android_log(LogPriority::WARN, "CHECKPOINT: Force abandoning Skia context from UI thread");
                        unsafe { ctx.context.abandon_context(); }
                    }
                }
            }

            if let Some(handle) = render_thread::get_render_thread_handle().lock().unwrap().take() {
                let join_start = std::time::Instant::now();
                let _ = handle.join();
                android_log(LogPriority::INFO, &format!("CHECKPOINT: Render thread joined in {:?}. Total time so far: {:?}", 
                    join_start.elapsed(), start_time.elapsed()));
            }
 else {
                android_log(LogPriority::WARN, "CHECKPOINT: No active render thread handle found to join");
            }

            if let Some(mutex) = render_thread::get_vulkan_context().get() {
                android_log(LogPriority::DEBUG, "CHECKPOINT: Attempting to clear VULKAN_CONTEXT...");
                let lock_start = std::time::Instant::now();
                match mutex.try_lock() {
                    Ok(mut guard) => {
                        *guard = None;
                        android_log(LogPriority::INFO, &format!("CHECKPOINT: VULKAN_CONTEXT cleared. Lock acquired in {:?}. Total: {:?}", 
                            lock_start.elapsed(), start_time.elapsed()));
                    }
                    Err(_) => {
                        android_log(LogPriority::ERROR, "CRITICAL: VULKAN_CONTEXT lock is held by another thread! Forcing wait...");
                        let mut guard = mutex.lock().unwrap();
                        *guard = None;
                        android_log(LogPriority::WARN, &format!("CHECKPOINT: VULKAN_CONTEXT cleared after FORCED WAIT. Total: {:?}", 
                            start_time.elapsed()));
                    }
                }
            }
            android_log(LogPriority::INFO, "CHECKPOINT: nativeSetSurface(null) EXITING");
            return;
        }

        android_log(LogPriority::DEBUG, "nativeSetSurface: Non-null surface received");

        let window = unsafe {
            ndk_sys::ANativeWindow_fromSurface(env.get_native_interface(), surface.as_raw())
        };

        if window.is_null() {
            android_log(LogPriority::ERROR, "nativeSetSurface: Failed to get ANativeWindow from Surface");
            render_thread::get_surface_ready().store(false, std::sync::atomic::Ordering::SeqCst);
            return;
        }

        android_log(LogPriority::DEBUG, &format!("nativeSetSurface: ANativeWindow acquired: {:p}", window));

        unsafe {
            android_log(LogPriority::DEBUG, "nativeSetSurface: Attempting VulkanContext::new()");
            if let Some(ctx) = crate::vulkan_context::VulkanContext::new(window as _) {
                let mutex = render_thread::get_vulkan_context().get_or_init(|| std::sync::Mutex::new(None));
                let mut guard = mutex.lock().unwrap();
                *guard = Some(ctx);
                android_log(LogPriority::INFO, "nativeSetSurface: Vulkan Context initialized successfully");

                render_thread::get_surface_ready().store(true, std::sync::atomic::Ordering::SeqCst);
                android_log(LogPriority::DEBUG, "nativeSetSurface: SURFACE_READY set to true, calling try_start_render_thread()");
                render_thread::try_start_render_thread();
            } else {
                android_log(LogPriority::ERROR, "nativeSetSurface: FAILED to initialize Vulkan Context");
                render_thread::get_surface_ready().store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        android_log(LogPriority::WARN, "nativeSetSurface: Called on non-Android platform");
    }
}

/// 尺寸变化通知
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeOnSizeChanged(
    _env: JNIEnv,
    _obj: JObject,
    width: jint,
    height: jint,
) {
    android_log(LogPriority::INFO, &format!("nativeOnSizeChanged: {}x{}", width, height));

    *render_thread::get_surface_new_width().lock().unwrap() = width as u32;
    *render_thread::get_surface_new_height().lock().unwrap() = height as u32;
    render_thread::get_surface_size_changed().store(true, std::sync::atomic::Ordering::SeqCst);
    render_thread::request_render();
    android_log(LogPriority::DEBUG, "nativeOnSizeChanged: Set SURFACE_SIZE_CHANGED=true");
}

/// 设置引擎指针
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetEnginePointer(
    _env: JNIEnv,
    _obj: JObject,
    engine_ptr: jlong,
) {
    android_log(LogPriority::INFO, &format!("nativeSetEnginePointer: engine_ptr={}", engine_ptr));

    if engine_ptr == 0 {
        android_log(LogPriority::ERROR, "nativeSetEnginePointer: Received null engine pointer!");
        return;
    }

    *render_thread::get_engine_pointer().lock().unwrap() = engine_ptr;
    render_thread::get_engine_ready().store(true, std::sync::atomic::Ordering::SeqCst);
    android_log(LogPriority::DEBUG, "nativeSetEnginePointer: ENGINE_READY set to true");
    render_thread::try_start_render_thread();
}

/// 渲染方法（已弃用路径）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeRender(
    _env: JNIEnv,
    _obj: JObject,
    engine_ptr: jlong,
    _scale: jfloat,
    _scroll_offset: jfloat,
    _sel_x1: jint,
    _sel_y1: jint,
    _sel_x2: jint,
    _sel_y2: jint,
    _sel_active: jboolean,
) {
    android_log(LogPriority::DEBUG, "nativeRender: Called (deprecated path)");

    if engine_ptr != 0 {
        *render_thread::get_engine_pointer().lock().unwrap() = engine_ptr;
        render_thread::get_engine_ready().store(true, std::sync::atomic::Ordering::SeqCst);
    }

    if !render_thread::get_render_thread_running().load(std::sync::atomic::Ordering::SeqCst) {
        render_thread::try_start_render_thread();
    }
}

// ============================================================================
// TerminalEmulator JNI 接口
// ============================================================================

/// 将事件刷新到 Java 侧
fn flush_events_to_java(env: &mut JNIEnv, callback_obj: &Option<jni::objects::GlobalRef>, events: Vec<TerminalEvent>) {
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
                    let _ = env.call_method(obj, "onCopyTextToClipboard", "(Ljava/lang/String;)V", &[(&j_text).into()]);
                }
            }
            TerminalEvent::TitleChanged(title) => {
                if let Ok(j_title) = env.new_string(title) {
                    let _ = env.call_method(obj, "reportTitleChange", "(Ljava/lang/String;)V", &[(&j_title).into()]);
                }
            }
            TerminalEvent::TerminalResponse(resp) => {
                if let Ok(j_resp) = env.new_string(resp) {
                    let _ = env.call_method(obj, "write", "(Ljava/lang/String;)V", &[(&j_resp).into()]);
                }
            }
            TerminalEvent::SixelImage { rgba_data, width, height, start_x, start_y } => {
                if let Ok(j_data) = env.new_byte_array(rgba_data.len() as i32) {
                    unsafe {
                        let _ = env.set_byte_array_region(&j_data, 0, std::mem::transmute::<&[u8], &[i8]>(&rgba_data));
                    }
                    let _ = env.call_method(obj, "onSixelImage", "([BIIII)V", &[
                        (&j_data).into(),
                        width.into(),
                        height.into(),
                        start_x.into(),
                        start_y.into(),
                    ]);
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

            let mut attached_env = crate::JAVA_VM.get().and_then(|vm| {
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

// ============================================================================
// 其余 TerminalEmulator JNI 接口
// ============================================================================

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

        android_log(LogPriority::DEBUG, &format!("[TRACE_SESSION] Session ID: {}", session_id));

        let pty_res = crate::pty::create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, cols, cw, ch);
        let (pty_fd, pid) = match pty_res {
            Ok(res) => {
                android_log(LogPriority::DEBUG, &format!("[TRACE_SESSION] PTY created (fd={}, pid={})", res.0, res.1));
                res
            },
            Err(_) => {
                android_log(LogPriority::ERROR, "[TRACE_SESSION] PTY creation failed");
                coordinator.unregister_session(session_id);
                return;
            }
        };

        android_log(LogPriority::DEBUG, "[TRACE_SESSION] Creating TerminalEngine");
        let mut engine = TerminalEngine::new(cols, rows, transcript_rows, cw, ch);
        if let Some(ref cb) = callback_ref {
            engine.state.java_callback_obj = Some(cb.clone());
        }

        let context = Arc::new(TerminalContext::new(engine));
        let context_ptr = Arc::into_raw(context.clone());

        android_log(LogPriority::DEBUG, "[TRACE_SESSION] Starting IO thread");
        context.start_io_thread(pty_fd);

        if let Some(ref cb) = callback_ref {
            android_log(LogPriority::DEBUG, "[TRACE_SESSION] Attempting to callback Java");
            if let Some(vm) = crate::JAVA_VM.get() {
                if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                    let _ = env.call_method(
                        cb.as_obj(),
                        "onEngineInitialized",
                        "(JII)V",
                        &[
                            jni::objects::JValue::Long(context_ptr as jlong),
                            jni::objects::JValue::Int(pty_fd),
                            jni::objects::JValue::Int(pid),
                        ],
                    );
                    android_log(LogPriority::DEBUG, "[TRACE_SESSION] Java callback executed");
                }
            }
        }
        android_log(LogPriority::INFO, &format!("Async session creation complete (pid={})", pid));
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

// ============================================================================
// KeyHandler.java - 键盘按键处理
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getKeyCode(
    env: JNIEnv,
    _class: JClass,
    key_code: jint,
    key_mode: jint,
    cursor_app: jboolean,
    keypad: jboolean,
) -> jstring {
    let result = crate::terminal::key_handler::get_code(
        key_code,
        key_mode as u32,
        cursor_app != 0,
        keypad != 0,
    );

    match result {
        Some(s) => env.new_string(s).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getKeyCodeFromTermcap(
    mut env: JNIEnv,
    _class: JClass,
    termcap: JString,
    cursor_app: jboolean,
    keypad: jboolean,
) -> jstring {
    let termcap_str: String = env.get_string(&termcap).unwrap().into();

    let result = crate::terminal::key_handler::get_code_from_termcap(
        &termcap_str,
        cursor_app != 0,
        keypad != 0,
    );

    match result {
        Some(s) => env.new_string(s).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    }
}

// ============================================================================
// JNI_OnLoad
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: std::ffi::c_void) -> jint {
    let result = crate::JAVA_VM.set(vm);
    match result {
        Ok(()) => android_log(LogPriority::INFO, "JNI_OnLoad: Termux- library loaded successfully"),
        Err(_) => android_log(LogPriority::WARN, "JNI_OnLoad: JAVA_VM was already set"),
    }
    jni::sys::JNI_VERSION_1_6
}
