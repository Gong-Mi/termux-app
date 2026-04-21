#![allow(unused_variables)]
use std::sync::atomic::Ordering;
use jni::JNIEnv;
use jni::objects::{JObject, JString, JClass};
use jni::sys::{jint, jlong, jfloat, jfloatArray, jboolean};

use crate::utils::{android_log, LogPriority};
use crate::vulkan_context::VulkanContext;

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_view_TerminalView_nativeSetSurface(
    env: JNIEnv,
    _class: JClass,
    surface: JObject,
) {
    #[cfg(target_os = "android")]
    {
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
            let window = unsafe { ndk_sys::ANativeWindow_fromSurface(env.get_native_interface(), surface.as_raw()) };
            if !window.is_null() {
                let ctx_cell = crate::render_thread::get_vulkan_context();
                if let Some(mutex) = ctx_cell.get() {
                    if let Ok(mut guard) = mutex.lock() {
                        let ctx_opt: &mut Option<VulkanContext> = &mut *guard;
                        if let Some(ctx) = ctx_opt.as_mut() {
                            unsafe { ctx.recreate_surface(window as _); }
                        } else if let Some(new_ctx) = unsafe { VulkanContext::new(window as _) } {
                            *ctx_opt = Some(new_ctx);
                        }
                    }
                } else {
                    let _ = ctx_cell.get_or_init(|| {
                        let ctx = unsafe { VulkanContext::new(window as _) };
                        std::sync::Mutex::new(ctx)
                    });
                }
                crate::render_thread::get_surface_ready().store(true, Ordering::SeqCst);
            }
        }
    }
    
    #[cfg(not(target_os = "android"))]
    {
        // CI 环境下的空实现
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
    if ptr != 0 {
        crate::render_thread::try_start_render_thread();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_view_TerminalView_nativeUpdateRenderParams(
    _env: JNIEnv,
    _class: JClass,
    scale: jfloat,
    scroll_offset: jfloat,
    top_row: jint,
    sel_x1: jint,
    sel_y1: jint,
    sel_x2: jint,
    sel_y2: jint,
    sel_active: jboolean,
) {
    if let Ok(mut params) = crate::render_thread::get_render_params().lock() {
        params.scale = scale;
        params.scroll_offset = scroll_offset;
        params.top_row = top_row;
        params.sel_x1 = sel_x1;
        params.sel_y1 = sel_y1;
        params.sel_x2 = sel_x2;
        params.sel_y2 = sel_y2;
        params.sel_active = sel_active != 0;
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
    let mut font_width = 0.0f32;
    let mut font_height = 0.0f32;
    let mut font_ascent = 0.0f32;

    // 1. 尝试从已存在的 TerminalRenderer 读取真实字体指标
    if let Some(mutex) = crate::render_thread::get_terminal_renderer().get() {
        if let Ok(guard) = mutex.lock() {
            if let Some(renderer) = guard.as_ref() {
                font_width = renderer.font_width;
                font_height = renderer.font_height;
                font_ascent = renderer.font_ascent();
            }
        }
    }

    // 2. Renderer 尚未创建时，根据当前 font_size 创建临时 FontCache 计算
    if font_width <= 0.0 {
        let font_size = *crate::render_thread::get_render_font_size().lock().unwrap();
        let font_path = crate::render_thread::get_render_font_path();
        let cache = crate::renderer::FontCache::new(font_size, font_path.as_deref());
        font_width = cache.font_width;
        font_height = cache.font_height;
        font_ascent = cache.font_ascent;
    }

    let values = [font_width, font_height, font_ascent];
    if !metrics_array.is_null() {
        let j_array = unsafe { jni::objects::JFloatArray::from_raw(metrics_array) };
        let _ = env.set_float_array_region(&j_array, 0, &values);
    }
}
