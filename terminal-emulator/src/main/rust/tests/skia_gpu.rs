use khronos_egl as egl;
use skia_safe::{gpu, Color, Surface};
use std::ptr;

#[test]
fn test_skia_gpu_context() {
    // 1. 加载系统 EGL 驱动
    let egl_lib = unsafe { egl::DynamicInstance::load() }.expect("Failed to load libEGL.so");
    let display = egl_lib.get_display(egl::DEFAULT_DISPLAY).expect("Failed to get EGL display");
    
    let (major, minor) = egl_lib.initialize(display).expect("Failed to initialize EGL");
    println!("EGL version: {}.{}", major, minor);

    // 2. 选择 EGL 配置并创建上下文
    let attributes = [
        egl::RENDERABLE_TYPE, egl::OPENGL_ES2_BIT,
        egl::SURFACE_TYPE, egl::PBUFFER_BIT,
        egl::RED_SIZE, 8,
        egl::GREEN_SIZE, 8,
        egl::BLUE_SIZE, 8,
        egl::ALPHA_SIZE, 8,
        egl::NONE,
    ];

    let config = egl_lib.choose_first_config(display, &attributes)
        .expect("Failed to choose EGL config")
        .expect("No EGL config found");

    let context_attributes = [
        egl::CONTEXT_CLIENT_VERSION, 2,
        egl::NONE,
    ];

    let context = egl_lib.create_context(display, config, egl::NO_CONTEXT, &context_attributes)
        .expect("Failed to create EGL context");

    // 3. 创建一个 1x1 的 PBuffer Surface 用于初始化上下文
    let pbuffer_attributes = [
        egl::WIDTH, 1,
        egl::HEIGHT, 1,
        egl::NONE,
    ];
    let surface = egl_lib.create_pbuffer_surface(display, config, &pbuffer_attributes)
        .expect("Failed to create PBuffer surface");

    egl_lib.make_current(display, Some(surface), Some(surface), Some(context))
        .expect("Failed to make EGL context current");

    println!("EGL Context is current!");

    // 4. 初始化 Skia 的 GPU 直接上下文
    let mut context_options = gpu::ContextOptions::default();
    let direct_context = gpu::DirectContext::make_gl(None, &context_options)
        .expect("Failed to create Skia GPU DirectContext! (This usually means Skia can't talk to GLES)");

    println!("Skia GPU DirectContext created successfully!");

    // 5. 尝试在 GPU 上创建一个 Surface
    let image_info = skia_safe::ImageInfo::new_n32_premul((100, 100), None);
    let mut surface = Surface::new_render_target(
        &mut direct_context.into(),
        skia_safe::Budgeted::Yes,
        &image_info,
        None,
        gpu::SurfaceOrigin::BottomLeft,
        None,
        false,
        None,
    ).expect("Failed to create Skia GPU Surface!");

    let canvas = surface.canvas();
    canvas.clear(Color::BLUE);
    
    println!("Successfully rendered to Skia GPU Surface!");

    // 清理
    egl_lib.terminate(display).expect("Failed to terminate EGL");
}
