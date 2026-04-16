use jni::JNIEnv;
use jni::objects::{JString, JObject};
use jni::sys::{jint, jlong, jboolean, jfloat};
use std::sync::Mutex;

use crate::utils::{android_log, LogPriority};
use crate::render_thread;
use crate::vulkan_context::VulkanContext;

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
    let mut size_guard = render_thread::get_render_font_size().lock().unwrap();
    let old_size = *size_guard;
    *size_guard = font_size;
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
    mut env: JNIEnv,
    _obj: JObject,
    surface: JObject,
) {
    #[cfg(target_os = "android")]
    {
        // 关键修复：确保在访问 surface 之前检查其是否为 null (底层的 jobject 指针)
        if surface.as_raw().is_null() {
            let start_time = std::time::Instant::now();
            android_log(LogPriority::WARN, "CHECKPOINT: nativeSetSurface(null) ENTERED - Surface being destroyed");
            
            // 1. 立即标记 Surface 为不可用，拦截所有后续渲染尝试
            render_thread::get_surface_ready().store(false, std::sync::atomic::Ordering::SeqCst);
            
            // 2. 标记线程退出并唤醒它
            render_thread::get_render_thread_running().store(false, std::sync::atomic::Ordering::SeqCst);
            render_thread::request_render(); 

            // 3. 在 join 之前，通过 Mutex 获取上下文并强制释放 GPU 资源。
            if let Some(mutex) = render_thread::get_vulkan_context().get() {
                if let Ok(mut guard) = mutex.try_lock() {
                    if let Some(ctx) = guard.as_mut() {
                        android_log(LogPriority::WARN, "CHECKPOINT: Abandoning Vulkan context");
                        ctx.context.abandon();
                    }
                    *guard = None; // 彻底清除上下文
                }
            }

            // 4. 等待渲染线程结束
            if let Some(handle) = render_thread::get_render_thread_handle().lock().unwrap().take() {
                let _ = handle.join();
                android_log(LogPriority::INFO, "CHECKPOINT: Render thread joined successfully");
            }

            android_log(LogPriority::INFO, &format!("CHECKPOINT: nativeSetSurface(null) EXITING - Total time: {:?}", start_time.elapsed()));
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

        // 初始化或更新 Vulkan 上下文
        if let Some(mutex) = render_thread::get_vulkan_context().get() {
            let mut guard = mutex.lock().unwrap();
            android_log(LogPriority::INFO, "nativeSetSurface: Recreating Vulkan context for new window");
            *guard = unsafe { VulkanContext::new(window as _) };
        } else {
            android_log(LogPriority::INFO, "nativeSetSurface: Initializing VULKAN_CONTEXT OnceCell");
            let ctx = unsafe { VulkanContext::new(window as _) };
            let mutex = Mutex::new(ctx);
            let _ = render_thread::get_vulkan_context().set(mutex);
        }

        render_thread::get_surface_ready().store(true, std::sync::atomic::Ordering::SeqCst);
        render_thread::try_start_render_thread();
        android_log(LogPriority::INFO, "nativeSetSurface: Surface marked as READY");
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
