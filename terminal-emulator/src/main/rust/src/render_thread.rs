/// 渲染线程管理
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use jni::sys::jlong;

use crate::utils::{android_log, LogPriority};
use crate::engine::TerminalContext;
use crate::vulkan_context::VulkanContext;
use crate::renderer::{TerminalRenderer, RenderFrame};
use once_cell::sync::OnceCell;

static VULKAN_CONTEXT: OnceCell<Mutex<Option<VulkanContext>>> = OnceCell::new();
static TERMINAL_RENDERER: OnceCell<Mutex<Option<TerminalRenderer>>> = OnceCell::new();

/// 渲染线程控制
static RENDER_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);
static RENDER_THREAD_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static ENGINE_POINTER: Mutex<jlong> = Mutex::new(0);

/// 状态标志
static SURFACE_READY: AtomicBool = AtomicBool::new(false);
static ENGINE_READY: AtomicBool = AtomicBool::new(false);

/// 渲染参数 - 合并为单个结构体，一次锁定获取所有值
#[derive(Clone, Copy)]
#[repr(C)]
pub struct RenderParams {
    pub scale: f32,
    pub scroll_offset: f32,
    pub top_row: i32,
    pub sel_x1: i32,
    pub sel_y1: i32,
    pub sel_x2: i32,
    pub sel_y2: i32,
    pub sel_active: bool,
}

static RENDER_PARAMS: Mutex<RenderParams> = Mutex::new(RenderParams {
    scale: 1.0,
    scroll_offset: 0.0,
    top_row: 0,
    sel_x1: 0,
    sel_y1: 0,
    sel_x2: 0,
    sel_y2: 0,
    sel_active: false,
});

/// 字体尺寸
static RENDER_FONT_SIZE: Mutex<f32> = Mutex::new(12.0);

/// 自定义字体文件路径（由 Java/Kotlin 侧设置）
static RENDER_FONT_PATH: Mutex<Option<String>> = Mutex::new(None);

pub fn get_render_font_path() -> Option<String> {
    RENDER_FONT_PATH.lock().unwrap().clone()
}

pub fn set_render_font_path(path: &str) {
    *RENDER_FONT_PATH.lock().unwrap() = Some(path.to_string());
}

/// 通知渲染线程重建 swapchain
static SURFACE_SIZE_CHANGED: AtomicBool = AtomicBool::new(false);
static SURFACE_NEW_WIDTH: Mutex<u32> = Mutex::new(0);
static SURFACE_NEW_HEIGHT: Mutex<u32> = Mutex::new(0);

/// 屏幕脏标记
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

/// 统一的渲染线程启动检查函数
pub fn try_start_render_thread() {
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

            let mut frame_count: u64 = 0;
            let mut last_log_time = std::time::Instant::now();

            while RENDER_THREAD_RUNNING.load(Ordering::SeqCst) {
                // 1. 检查是否需要重建 swapchain
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
                                request_render();
                            }
                        }
                    }
                }

                // 2. 事件驱动与轮询结合的节流
                if !SCREEN_DIRTY.swap(false, Ordering::SeqCst) {
                    std::thread::park_timeout(std::time::Duration::from_millis(500));
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

                // 获取 Engine 实例
                let current_engine_ptr = *ENGINE_POINTER.lock().unwrap();
                if current_engine_ptr == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                    continue;
                }

                let term_ctx = unsafe { &*(current_engine_ptr as *const TerminalContext) };

                // 一次锁定获取所有渲染参数
                let params = *RENDER_PARAMS.lock().unwrap();
                let top_row = params.top_row;

                let frame = {
                    let engine = match term_ctx.lock.try_read() {
                        Ok(e) => e,
                        Err(_) => {
                            std::thread::sleep(std::time::Duration::from_millis(2));
                            continue;
                        }
                    };
                    RenderFrame::from_engine(&engine, engine.state.rows as usize, engine.state.cols as usize, top_row)
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
                let font_path = crate::render_thread::get_render_font_path();
                let needs_recreate = renderer_guard.as_ref().map_or(true, |r| {
                    (r.font_size - font_size).abs() > 0.1 || r.font_path != font_path
                });
                if needs_recreate {
                    *renderer_guard = Some(TerminalRenderer::new(&[], font_size, font_path.as_deref()));
                }

                if let Some(renderer) = renderer_guard.as_mut() {
                    if params.sel_active {
                        renderer.set_selection(params.sel_x1, params.sel_y1, params.sel_x2, params.sel_y2);
                    } else {
                        renderer.clear_selection();
                    }

                    if frame_count == 0 {
                        android_log(LogPriority::INFO, &format!(
                            "Render: First frame - scale={}, scroll_offset={}, font_size={}, rows={}, cols={}",
                            params.scale, params.scroll_offset, font_size, frame.rows, frame.cols
                        ));
                    }

                    renderer.draw_frame(canvas, &frame, params.scale, params.scroll_offset);
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

// Getters for static state (used by JNI functions)
pub fn get_vulkan_context() -> &'static OnceCell<Mutex<Option<VulkanContext>>> {
    &VULKAN_CONTEXT
}

pub fn get_terminal_renderer() -> &'static OnceCell<Mutex<Option<TerminalRenderer>>> {
    &TERMINAL_RENDERER
}

pub fn get_surface_ready() -> &'static AtomicBool {
    &SURFACE_READY
}

pub fn get_engine_ready() -> &'static AtomicBool {
    &ENGINE_READY
}

pub fn get_engine_pointer() -> &'static Mutex<jlong> {
    &ENGINE_POINTER
}

pub fn get_render_params() -> &'static Mutex<RenderParams> {
    &RENDER_PARAMS
}

pub fn get_render_font_size() -> &'static Mutex<f32> {
    &RENDER_FONT_SIZE
}

pub fn get_surface_size_changed() -> &'static AtomicBool {
    &SURFACE_SIZE_CHANGED
}

pub fn get_surface_new_width() -> &'static Mutex<u32> {
    &SURFACE_NEW_WIDTH
}

pub fn get_surface_new_height() -> &'static Mutex<u32> {
    &SURFACE_NEW_HEIGHT
}

pub fn get_screen_dirty() -> &'static AtomicBool {
    &SCREEN_DIRTY
}

pub fn get_render_thread_running() -> &'static AtomicBool {
    &RENDER_THREAD_RUNNING
}

pub fn get_render_thread_handle() -> &'static Mutex<Option<std::thread::JoinHandle<()>>> {
    &RENDER_THREAD_HANDLE
}
