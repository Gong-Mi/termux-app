use jni::JNIEnv;
use jni::objects::{JClass, JString, JObject};
use jni::sys::{jint, jlong, jbyteArray, jboolean, jintArray, jstring, jfloat};
use once_cell::sync::OnceCell;
use std::sync::Mutex;
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use libc;

// 声明子模块
pub mod terminal;
pub mod utils;
pub mod engine;
pub mod bootstrap;
pub mod fastpath;
pub mod pty;
pub mod vte_parser;
pub mod coordinator;
pub mod renderer;
pub mod vulkan_context;

pub use crate::engine::{TerminalEngine, TerminalContext, TerminalEvent};
pub use crate::coordinator::{SessionCoordinator, SessionState};
pub use crate::terminal::style::*;
pub use crate::terminal::modes::*;
pub use crate::terminal::colors::*;
pub use crate::terminal::sixel::{SixelDecoder, SixelState, SixelColor};
use crate::utils::{android_log, LogPriority};

pub static JAVA_VM: OnceCell<jni::JavaVM> = OnceCell::new();

static VULKAN_CONTEXT: OnceCell<Mutex<Option<crate::vulkan_context::VulkanContext>>> = OnceCell::new();
static TERMINAL_RENDERER: OnceCell<Mutex<Option<crate::renderer::TerminalRenderer>>> = OnceCell::new();

/// 渲染线程控制
static RENDER_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);
static RENDER_THREAD_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static ENGINE_POINTER: Mutex<jlong> = Mutex::new(0);

/// 状态标志：用于跟踪 Surface 和 Engine 的就绪状态
static SURFACE_READY: AtomicBool = AtomicBool::new(false);
static ENGINE_READY: AtomicBool = AtomicBool::new(false);

/// 渲染参数（由 Java 侧通过 JNI 设置）
static RENDER_SCALE: Mutex<f32> = Mutex::new(1.0);
static RENDER_SCROLL_OFFSET: Mutex<f32> = Mutex::new(0.0);
static RENDER_TOP_ROW: Mutex<jint> = Mutex::new(0);
static RENDER_SEL_X1: Mutex<jint> = Mutex::new(0);
static RENDER_SEL_Y1: Mutex<jint> = Mutex::new(0);
static RENDER_SEL_X2: Mutex<jint> = Mutex::new(0);
static RENDER_SEL_Y2: Mutex<jint> = Mutex::new(0);
static RENDER_SEL_ACTIVE: Mutex<bool> = Mutex::new(false);

/// 字体尺寸（由 Java 侧设置）
static RENDER_FONT_SIZE: Mutex<f32> = Mutex::new(12.0);

/// 通知渲染线程重建 swapchain
static SURFACE_SIZE_CHANGED: AtomicBool = AtomicBool::new(false);
static SURFACE_NEW_WIDTH: Mutex<u32> = Mutex::new(0);
static SURFACE_NEW_HEIGHT: Mutex<u32> = Mutex::new(0);

/// 屏幕脏标记：终端内容变化时设为 true，渲染后清零
/// 使渲染线程只在有变化时工作，避免空转耗电
static SCREEN_DIRTY: AtomicBool = AtomicBool::new(false);

/// 触发重绘并唤醒渲染线程
pub fn request_render() {
    SCREEN_DIRTY.store(true, Ordering::SeqCst);
    if let Ok(guard) = RENDER_THREAD_HANDLE.lock() {
        if let Some(handle) = guard.as_ref() {
            handle.thread().unpark();
        }
    }
}

/// 设置渲染参数（由 Java onDraw 调用）
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
    *RENDER_SCALE.lock().unwrap() = scale;
    *RENDER_SCROLL_OFFSET.lock().unwrap() = scroll_offset;
    *RENDER_TOP_ROW.lock().unwrap() = top_row;
    *RENDER_SEL_X1.lock().unwrap() = sel_x1;
    *RENDER_SEL_Y1.lock().unwrap() = sel_y1;
    *RENDER_SEL_X2.lock().unwrap() = sel_x2;
    *RENDER_SEL_Y2.lock().unwrap() = sel_y2;
    *RENDER_SEL_ACTIVE.lock().unwrap() = sel_active != 0;
    // 渲染参数改变（如滚动）需要触发重绘
    request_render();
}

/// 设置字体尺寸（由 Java setTextSize 调用）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetFontSize(
    _env: JNIEnv,
    _obj: JObject,
    font_size: jfloat,
) {
    let old_size = *RENDER_FONT_SIZE.lock().unwrap();
    *RENDER_FONT_SIZE.lock().unwrap() = font_size;
    android_log(LogPriority::DEBUG, &format!("nativeSetFontSize: {} -> {}", old_size, font_size));
    // 字体大小改变需要触发重绘
    request_render();
}

/// 获取字体指标（供 Java TerminalView 替代 mRenderer 使用）
/// 返回值通过 float[] 参数传出：[fontWidth, fontHeight, fontAscent]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeGetFontMetrics(
    mut env: JNIEnv,
    _obj: JObject,
    metrics_array: jni::sys::jfloatArray,
) {
    let font_size = *RENDER_FONT_SIZE.lock().unwrap();

    // 防御性检查：font_size 无效时使用默认值
    let safe_font_size = if font_size > 0.0 && font_size.is_finite() { font_size } else { 12.0 };

    // 使用 skia 测量字体指标（与 renderer.rs 中 FontCache 一致）
    use skia_safe::{Font, FontMgr, FontStyle};

    let (font_width, font_height, font_ascent) = match FontMgr::new().match_family_style("monospace", FontStyle::normal()) {
        Some(tf) => {
            let mut font = Font::new(tf, Some(safe_font_size));
            let metrics = font.metrics();
            let h = (metrics.1.descent - metrics.1.ascent + metrics.1.leading).ceil();
            let (w, _) = font.measure_str("M", None);
            (w, h, metrics.1.ascent)
        }
        None => {
            // Fallback：基于 font_size 的简单估算
            (safe_font_size * 0.6, safe_font_size * 1.2, -safe_font_size * 0.8)
        }
    };

    let values = [font_width, font_height, font_ascent];
    unsafe {
        let j_array = jni::objects::JFloatArray::from_raw(metrics_array);
        let _ = env.set_float_array_region(&j_array, 0, &values);
    }
}

/// 核心修复：在不持有锁的情况下将事件刷新到 Java，彻底杜绝双向死锁
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

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: std::ffi::c_void) -> jint {
    let result = JAVA_VM.set(vm);
    match result {
        Ok(()) => android_log(LogPriority::INFO, "JNI_OnLoad: Termux-Rust library loaded successfully, JAVA_VM set"),
        Err(_) => android_log(LogPriority::WARN, "JNI_OnLoad: JAVA_VM was already set (library loaded before?)"),
    }
    jni::sys::JNI_VERSION_1_6
}

/// 统一的渲染线程启动检查函数
/// 只有在 Surface 和 Engine 都就绪后才会启动渲染线程
fn try_start_render_thread() {
    let surface_ready = SURFACE_READY.load(Ordering::SeqCst);
    let engine_ready = ENGINE_READY.load(Ordering::SeqCst);

    android_log(LogPriority::DEBUG, &format!(
        "try_start_render_thread: surface_ready={}, engine_ready={}, already_running={}",
        surface_ready, engine_ready, RENDER_THREAD_RUNNING.load(Ordering::SeqCst)
    ));

    if surface_ready && engine_ready {
        let engine_ptr = *ENGINE_POINTER.lock().unwrap();
        if engine_ptr != 0 && !RENDER_THREAD_RUNNING.load(Ordering::SeqCst) {
            android_log(LogPriority::INFO, &format!("try_start_render_thread: Both conditions met, starting render thread with engine={}", engine_ptr));
            spawn_render_thread(engine_ptr);
        } else if engine_ptr == 0 {
            android_log(LogPriority::ERROR, "try_start_render_thread: engine_ptr is 0, cannot start render thread");
        } else {
            android_log(LogPriority::DEBUG, "try_start_render_thread: Render thread already running, skipping");
        }
    } else {
        android_log(LogPriority::DEBUG, "try_start_render_thread: Waiting for both surface and engine to be ready");
    }
}

/// 实际启动渲染线程的内部函数
fn spawn_render_thread(engine_ptr: jlong) {
    RENDER_THREAD_RUNNING.store(true, Ordering::SeqCst);
    android_log(LogPriority::INFO, &format!("spawn_render_thread: Starting Vulkan render thread (engine={})", engine_ptr));

    let handle = std::thread::Builder::new()
        .name("VulkanRender".to_string())
        .spawn(move || {
            android_log(LogPriority::INFO, "Render thread started");

            // 统计变量，用于诊断日志节流
            let mut frame_count: u64 = 0;
            let mut last_log_time = std::time::Instant::now();

            while RENDER_THREAD_RUNNING.load(Ordering::SeqCst) {
                // 1. 首先检查是否需要重建 swapchain (不受 SCREEN_DIRTY 影响)
                if SURFACE_SIZE_CHANGED.load(Ordering::SeqCst) {
                    let new_width = *SURFACE_NEW_WIDTH.lock().unwrap();
                    let new_height = *SURFACE_NEW_HEIGHT.lock().unwrap();

                    if let Some(ctx_mutex) = VULKAN_CONTEXT.get() {
                        if let Ok(mut ctx_guard) = ctx_mutex.try_lock() {
                            if let Some(ctx) = ctx_guard.as_mut() {
                                let ok = ctx.recreate_swapchain(new_width, new_height);
                                android_log(LogPriority::INFO, &format!(
                                    "Render: Swapchain recreated {}x{} success={}", new_width, new_height, ok
                                ));
                                SURFACE_SIZE_CHANGED.store(false, Ordering::SeqCst);
                                // Resize 后应当强制刷新一帧
                                request_render();
                            }
                        }
                    }
                }

                // 2. 事件驱动与轮询结合的节流：
                // 如果没有脏标记，则挂起线程直到被唤醒（如新字符输入、滚动），
                // 或 500ms 超时（用于处理无输入时的光标闪烁刷新）。
                if !SCREEN_DIRTY.swap(false, Ordering::SeqCst) {
                    std::thread::park_timeout(std::time::Duration::from_millis(500));
                    // 无论是因为超时还是被事件唤醒，醒来后都清除可能的脏标记准备重新渲染。
                    SCREEN_DIRTY.store(false, Ordering::SeqCst);
                }

                // 获取 Vulkan 上下文
                let ctx_mutex = match VULKAN_CONTEXT.get() {
                    Some(m) => m,
                    None => {
                        std::thread::sleep(std::time::Duration::from_millis(16));
                        continue;
                    }
                };

                let mut ctx_guard = match ctx_mutex.try_lock() {
                    Ok(g) => g,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(8));
                        continue;
                    }
                };

                let ctx = match ctx_guard.as_mut() {
                    Some(c) => c,
                    None => {
                        std::thread::sleep(std::time::Duration::from_millis(16));
                        continue;
                    }
                };

                // 获取 Engine 实例 - 只在极短时间内持有锁
                let current_engine_ptr = *ENGINE_POINTER.lock().unwrap();
                if current_engine_ptr == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                    continue;
                }

                let term_ctx = unsafe { &*(current_engine_ptr as *const crate::engine::TerminalContext) };
                let top_row = *RENDER_TOP_ROW.lock().unwrap();

                let frame = {
                    let engine = match term_ctx.lock.try_read() {
                        Ok(e) => e,
                        Err(_) => {
                            std::thread::sleep(std::time::Duration::from_millis(2));
                            continue;
                        }
                    };
                    crate::renderer::RenderFrame::from_engine(&engine, engine.state.rows as usize, engine.state.cols as usize, top_row)
                };

                // 1. 获取下一个交换链图像索引
                let image_index = match ctx.acquire_next_image() {
                    Some(idx) => idx,
                    None => {
                        std::thread::sleep(std::time::Duration::from_millis(8));
                        continue;
                    }
                };

                // 2. 获取 Skia Surface
                let mut sk_surface = match ctx.get_sk_surface(image_index) {
                    Some(s) => s,
                    None => {
                        // 只在首次或每 60 帧打印一次
                        if frame_count == 0 || last_log_time.elapsed().as_secs() >= 5 {
                            android_log(LogPriority::WARN, &format!("Render: get_sk_surface returned None (frame {})", frame_count));
                            last_log_time = std::time::Instant::now();
                        }
                        continue;
                    }
                };
                let canvas = sk_surface.canvas();

                // 3. 执行绘制
                let renderer_mutex = TERMINAL_RENDERER.get_or_init(|| Mutex::new(None));
                let mut renderer_guard = match renderer_mutex.try_lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };

                let font_size = *RENDER_FONT_SIZE.lock().unwrap();
                let needs_recreate = renderer_guard.as_ref().map_or(true, |r| (r.font_size - font_size).abs() > 0.1);
                if needs_recreate {
                    *renderer_guard = Some(crate::renderer::TerminalRenderer::new(&[], font_size));
                }

                if let Some(renderer) = renderer_guard.as_mut() {
                    let scale = *RENDER_SCALE.lock().unwrap();
                    let scroll_offset = *RENDER_SCROLL_OFFSET.lock().unwrap();
                    let sel_active = *RENDER_SEL_ACTIVE.lock().unwrap();
                    let sel_x1 = *RENDER_SEL_X1.lock().unwrap();
                    let sel_y1 = *RENDER_SEL_Y1.lock().unwrap();
                    let sel_x2 = *RENDER_SEL_X2.lock().unwrap();
                    let sel_y2 = *RENDER_SEL_Y2.lock().unwrap();

                    if sel_active {
                        renderer.set_selection(sel_x1, sel_y1, sel_x2, sel_y2);
                    } else {
                        renderer.clear_selection();
                    }

                    // 诊断日志：首帧和尺寸变化时打印
                    if frame_count == 0 {
                        android_log(LogPriority::INFO, &format!(
                            "Render: First frame - scale={}, scroll_offset={}, font_size={}, rows={}, cols={}",
                            scale, scroll_offset, font_size, frame.rows, frame.cols
                        ));
                    }

                    renderer.draw_frame(canvas, &frame, scale, scroll_offset);
                }

                ctx.context.flush_and_submit();

                // 4. 呈现图像
                let present_info = ash::vk::PresentInfoKHR {
                    swapchain_count: 1,
                    p_swapchains: &ctx.swapchain,
                    p_image_indices: &image_index,
                    ..Default::default()
                };

                let present_result = unsafe {
                    ctx.swapchain_loader.queue_present(ctx.queue, &present_info)
                };

                // 首帧或偶尔记录呈现结果
                if frame_count == 0 {
                    android_log(LogPriority::INFO, &format!("Render: Present completed (result={:?})", present_result));
                }

                frame_count += 1;
            }
            android_log(LogPriority::INFO, &format!("Render thread stopped after {} frames", frame_count));
        })
        .expect("Failed to spawn render thread");

    *RENDER_THREAD_HANDLE.lock().unwrap() = Some(handle);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetSurface(
    env: JNIEnv,
    _obj: JObject,
    surface: JObject,
) {
    #[cfg(target_os = "android")]
    {
        // Handle null surface: clean up Vulkan context if surface is destroyed
        if surface.is_null() {
            android_log(LogPriority::INFO, "nativeSetSurface: Surface destroyed, stopping render thread");
            SURFACE_READY.store(false, Ordering::SeqCst);

            // 停止渲染线程
            RENDER_THREAD_RUNNING.store(false, Ordering::SeqCst);
            if let Some(handle) = RENDER_THREAD_HANDLE.lock().unwrap().take() {
                let _ = handle.join();
                android_log(LogPriority::INFO, "nativeSetSurface: Render thread stopped");
            }

            // 清理 Vulkan 上下文
            if let Some(mutex) = VULKAN_CONTEXT.get() {
                let mut guard = mutex.lock().unwrap();
                *guard = None;
                android_log(LogPriority::INFO, "nativeSetSurface: Vulkan context destroyed");
            }
            return;
        }

        android_log(LogPriority::DEBUG, "nativeSetSurface: Non-null surface received");

        let window = unsafe {
            ndk_sys::ANativeWindow_fromSurface(env.get_native_interface(), surface.as_raw())
        };

        if window.is_null() {
            android_log(LogPriority::ERROR, "nativeSetSurface: Failed to get ANativeWindow from Surface");
            SURFACE_READY.store(false, Ordering::SeqCst);
            return;
        }

        android_log(LogPriority::DEBUG, &format!("nativeSetSurface: ANativeWindow acquired: {:p}", window));

        unsafe {
            android_log(LogPriority::DEBUG, "nativeSetSurface: Attempting VulkanContext::new()");
            if let Some(ctx) = crate::vulkan_context::VulkanContext::new(window as _) {
                let mutex = VULKAN_CONTEXT.get_or_init(|| Mutex::new(None));
                let mut guard = mutex.lock().unwrap();
                *guard = Some(ctx);
                android_log(LogPriority::INFO, "nativeSetSurface: Vulkan Context initialized successfully");

                // 标记 Surface 就绪，并尝试启动渲染线程
                SURFACE_READY.store(true, Ordering::SeqCst);
                android_log(LogPriority::DEBUG, "nativeSetSurface: SURFACE_READY set to true, calling try_start_render_thread()");
                try_start_render_thread();
            } else {
                android_log(LogPriority::ERROR, "nativeSetSurface: FAILED to initialize Vulkan Context - Vulkan not available or unsupported?");
                SURFACE_READY.store(false, Ordering::SeqCst);
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        android_log(LogPriority::WARN, "nativeSetSurface: Called on non-Android platform");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeOnSizeChanged(
    _env: JNIEnv,
    _obj: JObject,
    width: jint,
    height: jint,
) {
    android_log(LogPriority::INFO, &format!("nativeOnSizeChanged: {}x{}", width, height));

    // 设置标志通知渲染线程重建 swapchain
    *SURFACE_NEW_WIDTH.lock().unwrap() = width as u32;
    *SURFACE_NEW_HEIGHT.lock().unwrap() = height as u32;
    SURFACE_SIZE_CHANGED.store(true, Ordering::SeqCst);
    // 尺寸变化需要触发重绘以刷新渲染目标
    request_render();
    android_log(LogPriority::DEBUG, "nativeOnSizeChanged: Set SURFACE_SIZE_CHANGED=true, render thread will handle swapchain recreation");
}

/// 设置引擎指针（emulator 初始化完成后调用）
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

    *ENGINE_POINTER.lock().unwrap() = engine_ptr;

    // 标记 Engine 就绪，并尝试启动渲染线程
    ENGINE_READY.store(true, Ordering::SeqCst);
    android_log(LogPriority::DEBUG, "nativeSetEnginePointer: ENGINE_READY set to true, calling try_start_render_thread()");
    try_start_render_thread();
}

/// nativeRender 不再用于每帧渲染（现在由独立渲染线程处理）
/// 保留此方法以兼容旧代码调用
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
    android_log(LogPriority::DEBUG, "nativeRender: Called (deprecated path, should not be used for per-frame rendering)");

    // 更新引擎指针（渲染线程会使用）
    if engine_ptr != 0 {
        *ENGINE_POINTER.lock().unwrap() = engine_ptr;
        ENGINE_READY.store(true, Ordering::SeqCst);
    }

    // 如果渲染线程还没启动，尝试启动它
    if !RENDER_THREAD_RUNNING.load(Ordering::SeqCst) {
        try_start_render_thread();
    }
}

// ============================================================================
// TerminalEmulator JNI 接口
// ============================================================================

use std::sync::Arc;

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
    if !callback.is_null() {
        if let Ok(global_ref) = env.new_global_ref(callback) {
            engine.state.java_callback_obj = Some(global_ref);
        }
    }
    let context = Arc::new(TerminalContext::new(engine));
    Arc::into_raw(context) as jlong
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
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            let j_array = unsafe { jni::objects::JByteArray::from_raw(data) };
            if let Ok(bytes) = env.convert_byte_array(&j_array) {
                engine.process_bytes(&bytes);
            }
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        flush_events_to_java(&mut env, &cb, events);
        request_render();
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_processBatchRust(
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
        request_render();
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeResize(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cols: jint,
    rows: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.resize(cols, rows);
        engine.events.push(TerminalEvent::ScreenUpdated);
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_processCodePointRust(
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
        request_render();
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resizeEngineRustFull(
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
    request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_destroyEngineRust(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
        context.running.store(false, Ordering::SeqCst);
        // Arc will be dropped here as we don't call Arc::into_raw(context)
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeStartIoThread(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    fd: jint,
) {
    if ptr == 0 { return; }
    
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let context_thread = Arc::clone(&context);
    
    // 克隆 PTY FD 以便在独立线程中使用
    let pty_fd = unsafe { libc::dup(fd) };
    if pty_fd < 0 { 
        let _ = Arc::into_raw(context);
        return; 
    }

    std::thread::Builder::new()
        .name("RustEngine".to_string())
        .spawn(move || {
            let context = context_thread;
            // 设置线程名称（Android 限制 16 字符，包括 null 终止符）
            let thread_name = std::ffi::CString::new("RustEngine").unwrap();
            unsafe {
                // 方法 1: prctl (Linux 标准)
                libc::prctl(libc::PR_SET_NAME, thread_name.as_ptr(), 0, 0, 0);
                // 方法 2: pthread_setname_np (Android 更可靠)
                libc::pthread_setname_np(libc::pthread_self(), thread_name.as_ptr());
            }

            android_log(LogPriority::INFO, "Rust IO Thread started (RustEngine)");
            
            // 必须附加当前线程到 JVM，否则无法通过 JNI 调用 Java 方法
            let mut attached_env = crate::JAVA_VM.get().and_then(|vm| {
                vm.attach_current_thread_as_daemon().ok()
            });

            let mut file = unsafe { std::fs::File::from_raw_fd(pty_fd) };
            let mut buffer = [0u8; 8192];
            
            // 获取 context 的原始指针进行长时间持有
            while context.running.load(Ordering::SeqCst) {
                match file.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let (events, cb) = {
                            let mut engine = context.lock.write().unwrap();
                            engine.process_bytes(&buffer[..n]);
                            (engine.take_events(), engine.state.java_callback_obj.clone())
                        };
                        request_render();
                        if let Some(ref mut env) = attached_env {
                            flush_events_to_java(env, &cb, events);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            android_log(LogPriority::INFO, "Rust IO Thread stopped");
        })
        .expect("Failed to spawn Rust IO thread");
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTitleFromRust(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorRowFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.y as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorColFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.x as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getCursorStyleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.style as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorStyleFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong, cursor_style: jint) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.style = cursor_style as i32;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_doDecSetOrResetFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong, setting: jboolean, mode: jint) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (events, cb) = {
            let mut engine = context.lock.write().unwrap();
            engine.state.do_decset_or_reset(setting != 0, mode as u32);
            (engine.take_events(), engine.state.java_callback_obj.clone())
        };
        request_render();
        flush_events_to_java(&mut env, &cb, events);
    }));
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_shouldCursorBeVisibleFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorEnabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isReverseVideoFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAlternateBufferActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isCursorKeysApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isKeypadApplicationModeFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isMouseTrackingActiveFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isInsertModeActiveFromRust(_env: JNIEnv, _class: JClass, _ptr: jlong) -> jboolean { 0 }

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getScrollCounterFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cols as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_readRowFromRust(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getSelectedTextFromRust(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getWordAtLocationFromRust(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getTranscriptTextFromRust(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_clearScrollCounterFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong) {
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_isAutoScrollDisabledFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jboolean {
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
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_toggleAutoScrollDisabledFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong) {
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendMouseEventFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong, button: jint, col: jint, row: jint, pressed: jboolean) {
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_sendKeyCodeFromRust(
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
        // send_key_event 现在返回生成的序列，由 Java 写入 PTY
        engine.send_key_event(key_code, Some(rust_str), meta_state)
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_pasteTextFromRust(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getActiveTranscriptRowsFromRust(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.get_current_screen().active_transcript_rows as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getColorsFromRust(env: JNIEnv, _class: JClass, ptr: jlong) -> jintArray {
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(mut env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };

    // 修复：在锁外回调，避免死锁
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.colors.reset();
        // 不再调用 report_colors_changed()，而是手动添加事件
        // 这样事件会在锁释放后通过 flush_events_to_java 处理
        let mut events = engine.take_events();
        events.push(crate::engine::TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    }; // 锁在此处释放

    // 在锁外安全回调 Java
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_updateColorsFromProperties(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    properties_obj: JObject,
) {
    if ptr == 0 || properties_obj.is_null() { return; }

    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };

    // 从 Java Properties 对象读取键值对
    let props_map = {
        let mut map = std::collections::HashMap::new();

        // 调用 Properties.entrySet() 获取所有键值对
        if let Ok(entry_set) = env.call_method(&properties_obj, "entrySet", "()Ljava/util/Set;", &[]) {
            if let Ok(entry_set_obj) = entry_set.l() {
                // 调用 Set.iterator()
                if let Ok(iterator) = env.call_method(&entry_set_obj, "iterator", "()Ljava/util/Iterator;", &[]) {
                    if let Ok(iter_obj) = iterator.l() {
                        // 遍历所有 entry
                        loop {
                            // 检查 hasNext()
                            if let Ok(has_next) = env.call_method(&iter_obj, "hasNext", "()Z", &[]) {
                                if let Ok(has_next_val) = has_next.z() {
                                    if !has_next_val { break; }
                                } else { break; }
                            } else { break; }

                            // 调用 next()
                            if let Ok(entry) = env.call_method(&iter_obj, "next", "()Ljava/lang/Object;", &[]) {
                                if let Ok(entry_obj) = entry.l() {
                                    // 调用 Map.Entry.getKey()
                                    if let Ok(key) = env.call_method(&entry_obj, "getKey", "()Ljava/lang/Object;", &[]) {
                                        if let Ok(key_obj) = key.l() {
                                            // 调用 Map.Entry.getValue()
                                            if let Ok(value) = env.call_method(&entry_obj, "getValue", "()Ljava/lang/Object;", &[]) {
                                                if let Ok(value_obj) = value.l() {
                                                    // 转换为 Rust String
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

        // 调用 Rust 的 update_with_properties
        if let Err(e) = engine.state.colors.update_with_properties(&props_map) {
            android_log(crate::utils::LogPriority::WARN, &format!("Failed to update colors: {}", e));
        }

        let mut events = engine.take_events();
        events.push(crate::engine::TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };

    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorColorForBackgroundFromRust(
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
        events.push(crate::engine::TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    };
    
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getPerceivedBrightnessOfColor(
    _env: JNIEnv,
    _class: JClass,
    color: jint,
) -> jint {
    // 将 Java 的 int (0xAARRGGBB) 转换为 u32
    let color_u32 = color as u32;
    TerminalColors::get_perceived_brightness(color_u32) as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_updateTerminalSessionClientFromRust(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkStateInRust(mut env: JNIEnv, _class: JClass, ptr: jlong, state: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.blink_state = state != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_setCursorBlinkingEnabledInRust(mut env: JNIEnv, _class: JClass, ptr: jlong, enabled: jboolean) {
    if ptr == 0 { return; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.cursor.blinking_enabled = enabled != 0;
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    request_render();
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(_env: JNIEnv, _class: JClass, ptr: jlong) {
    if ptr != 0 { 
        let _context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
        // Arc will be dropped here
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorCol(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.x as jint
    };
    let _ = Arc::into_raw(context);
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorRow(_env: JNIEnv, _class: JClass, ptr: jlong) -> jint {
    if ptr == 0 { return 0; }
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    let result = {
        let engine = context.lock.read().unwrap();
        engine.state.cursor.y as jint
    };
    let _ = Arc::into_raw(context);
    result
}

// ============================================================================
// TerminalEmulator.java - 调试辅助方法
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_getDebugInfoFromRust(
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
        // 注册 session 到协调器
        let coordinator = SessionCoordinator::get();
        let session_id = coordinator.register_session();
        
        android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.1. Background thread started in Rust");
        android_log(LogPriority::INFO, &format!("[TRACE_SESSION] Session ID: {}", session_id));

        // 1. 创建子进程
        let pty_res = crate::pty::create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, cols, cw, ch);
        let (pty_fd, pid) = match pty_res {
            Ok(res) => {
                android_log(LogPriority::DEBUG, &format!("[TRACE_SESSION] 5.2. PTY created (fd={}, pid={})", res.0, res.1));
                res
            },
            Err(_) => {
                android_log(LogPriority::ERROR, "[TRACE_SESSION] 5.2. ERROR: PTY creation failed");
                coordinator.unregister_session(session_id);
                return;
            }
        };

        // 2. 创建 Engine
        android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.3. Creating TerminalEngine");
        let mut engine = TerminalEngine::new(cols, rows, transcript_rows, cw, ch);
        if let Some(ref cb) = callback_ref {
            engine.state.java_callback_obj = Some(cb.clone());
        }
        
        let context = Arc::new(TerminalContext::new(engine));
        let context_ptr = Arc::into_raw(context.clone());

        // 3. 启动 IO 线程
        android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.4. Starting IO thread");
        context.start_io_thread(pty_fd);

        // 4. 回调 Java
        if let Some(ref cb) = callback_ref {
            android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.5. Attempting to callback Java");
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
                    android_log(LogPriority::DEBUG, "[TRACE_SESSION] 5.6. Java callback executed");
                }
            }
        }
        android_log(LogPriority::INFO, &format!("Async session creation complete (pid={}, engine={:p})", pid, context_ptr));
    });
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
