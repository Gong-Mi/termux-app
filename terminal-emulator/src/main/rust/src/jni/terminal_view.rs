use std::sync::atomic::Ordering;
use jni::JNIEnv;
use jni::objects::{JObject, JString, JClass, JFloatArray};
use jni::sys::{jint, jlong, jfloat, jfloatArray, jboolean};

use crate::utils::{android_log, LogPriority};
use crate::vulkan_context::VulkanContext;

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_view_TerminalView_nativeSetSurface(
    mut env: JNIEnv,
    _class: JClass,
    surface: JObject,
) {
    if surface.as_raw().is_null() {
        android_log(LogPriority::WARN, "CHECKPOINT: nativeSetSurface(null) - Detaching surface");
        if let Some(mutex) = crate::render_thread::get_vulkan_context().get() {
            if let Ok(mut guard) = mutex.lock() {
                let ctx_opt: &mut Option<VulkanContext> = &mut *guard;
                if let Some(ctx) = ctx_opt.as_mut() {
                    ctx.abandon_surface();
                }
            }
        }
        crate::render_thread::get_surface_ready().store(false, Ordering::SeqCst);
    } else {
        android_log(LogPriority::DEBUG, "nativeSetSurface: Attaching new surface");
        let window = ndk_sys::ANativeWindow_fromSurface(env.get_native_interface(), surface.as_raw());
        if !window.is_null() {
            let ctx_cell = crate::render_thread::get_vulkan_context();
            if let Some(mutex) = ctx_cell.get() {
                if let Ok(mut guard) = mutex.lock() {
                    let ctx_opt: &mut Option<VulkanContext> = &mut *guard;
                    if let Some(ctx) = ctx_opt.as_mut() {
                        ctx.recreate_surface(window as _);
                    } else if let Some(new_ctx) = VulkanContext::new(window as _) {
                        *ctx_opt = Some(new_ctx);
                    }
                }
            } else {
                let _ = ctx_cell.get_or_init(|| {
                    let ctx = VulkanContext::new(window as _);
                    std::sync::Mutex::new(ctx)
                });
            }
            crate::render_thread::get_surface_ready().store(true, Ordering::SeqCst);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetEnginePointer(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if let Ok(mut engine_ptr) = crate::render_thread::get_engine_pointer().lock() {
        *engine_ptr = ptr;
    }
    crate::render_thread::get_engine_ready().store(ptr != 0, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeUpdateRenderParams(
    _env: JNIEnv,
    _class: JClass,
    scale: jfloat,
    scroll_offset: jfloat,
    _top_row: jint,
    _sel_x1: jint,
    _sel_y1: jint,
    _sel_x2: jint,
    _sel_y2: jint,
    _sel_active: jboolean,
) {
    if let Ok(mut params) = crate::render_thread::get_render_params().lock() {
        params.scale = scale;
        params.scroll_offset = scroll_offset;
    }
    crate::render_thread::get_screen_dirty().store(true, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeOnSizeChanged(
    _env: JNIEnv,
    _class: JClass,
    w: jint,
    h: jint,
) {
    if let Ok(mut nw) = crate::render_thread::get_surface_new_width().lock() { *nw = w as u32; }
    if let Ok(mut nh) = crate::render_thread::get_surface_new_height().lock() { *nh = h as u32; }
    crate::render_thread::get_surface_size_changed().store(true, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetFontSize(
    _env: JNIEnv,
    _class: JClass,
    size: jfloat,
) {
    if let Ok(mut font_size) = crate::render_thread::get_render_font_size().lock() {
        *font_size = size;
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeSetFontPath(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    if let Ok(path_str) = env.get_string(&path) {
        let _ = String::from(path_str);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeGetFontMetrics(
    env: JNIEnv,
    _class: JClass,
    metrics_array: jfloatArray,
) {
    let values = [-18.0, 4.0, 0.0];
    if !metrics_array.is_null() {
        let j_array = unsafe { jni::objects::JFloatArray::from_raw(metrics_array) };
        let _ = env.set_float_array_region(&j_array, 0, &values);
    }
}
