use std::sync::atomic::Ordering;
use jni::JNIEnv;
use jni::objects::{JObject, JString, JClass};
use jni::sys::{jint, jlong, jfloat, jfloatArray};

use crate::utils::{android_log, LogPriority};
use crate::vulkan_context::VulkanContext;
use crate::render_thread;

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetSurfaceScale(
    _env: JNIEnv,
    _class: JClass,
    _ptr: jlong,
    scale: jfloat,
    scroll_offset: jfloat,
) {
    if let Ok(mut params) = crate::render_thread::get_render_params().lock() {
        params.scale = scale;
        params.scroll_offset = scroll_offset;
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetFontSize(
    _env: JNIEnv,
    _class: JClass,
    font_size: jfloat,
) {
    if let Ok(mut size) = crate::render_thread::get_render_font_size().lock() {
        *size = font_size;
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetFont(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    if let Ok(path_str) = env.get_string(&path) {
        let path_str: String = path_str.into();
        android_log(LogPriority::DEBUG, &format!("nativeSetFont: {}", path_str));
        // TODO: Implement font cache update if needed via a dirty flag
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeGetCursorCoords(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jfloatArray {
    if ptr == 0 { return std::ptr::null_mut(); }
    let context = unsafe { std::sync::Arc::from_raw(ptr as *const crate::engine::TerminalContext) };
    
    let (cx, cy) = {
        let engine = context.lock.read().unwrap();
        (engine.state.cursor.x as f32, engine.state.cursor.y as f32)
    };
    
    let _ = std::sync::Arc::into_raw(context);
    
    let values = [cx, cy];
    if let Ok(j_array) = env.new_float_array(2) {
        let _ = env.set_float_array_region(&j_array, 0, &values);
        j_array.into_raw()
    } else {
        std::ptr::null_mut()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_view_TerminalView_nativeSetSurface(
    mut env: JNIEnv,
    _class: JClass,
    surface: JObject,
) {
    if surface.as_raw().is_null() {
        android_log(LogPriority::WARN, "CHECKPOINT: nativeSetSurface(null) ENTERED - Surface being destroyed");
        if let Some(mut guard) = crate::render_thread::get_vulkan_context().get().and_then(|m| m.lock().ok()) {
            if let Some(ctx) = guard.as_mut() {
                ctx.abandon_surface();
                android_log(LogPriority::WARN, "VulkanContext: Abandoning Surface/Swapchain only");
            }
        }
        crate::render_thread::get_surface_ready().store(false, Ordering::SeqCst);
    } else {
        android_log(LogPriority::DEBUG, "nativeSetSurface: Non-null surface received");
        let window = ndk_sys::ANativeWindow_fromSurface(env.get_native_interface(), surface.as_raw());
        if !window.is_null() {
            let ctx_cell = crate::render_thread::get_vulkan_context();
            if let Some(mutex) = ctx_cell.get() {
                if let Ok(mut guard) = mutex.lock() {
                    if let Some(ctx) = guard.as_mut() {
                        android_log(LogPriority::INFO, "nativeSetSurface: Re-initializing existing context");
                        ctx.recreate_surface(window as _);
                    } else {
                        android_log(LogPriority::INFO, "nativeSetSurface: Initializing VULKAN_CONTEXT placeholder");
                        if let Some(new_ctx) = VulkanContext::new(window as _) {
                            *guard = Some(new_ctx);
                        }
                    }
                }
            } else {
                android_log(LogPriority::INFO, "nativeSetSurface: Initializing VULKAN_CONTEXT OnceCell");
                let _ = ctx_cell.get_or_init(|| {
                    let ctx = VulkanContext::new(window as _);
                    std::sync::Mutex::new(ctx)
                });
            }
            crate::render_thread::get_surface_ready().store(true, Ordering::SeqCst);
            android_log(LogPriority::INFO, "nativeSetSurface: Surface marked as READY");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeOnSizeChanged(
    _env: JNIEnv,
    _class: JClass,
    w: jint,
    h: jint,
) {
    android_log(LogPriority::INFO, &format!("nativeOnSizeChanged: {}x{}", w, h));
    if let Ok(mut new_w) = crate::render_thread::get_surface_new_width().lock() { *new_w = w as u32; }
    if let Ok(mut new_h) = crate::render_thread::get_surface_new_height().lock() { *new_h = h as u32; }
    crate::render_thread::get_surface_size_changed().store(true, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeUpdatePosition(
    _env: JNIEnv,
    _class: JClass,
    _scale: jfloat,
    _scroll_offset: jfloat,
) {
    // 渲染参数更新逻辑
}
