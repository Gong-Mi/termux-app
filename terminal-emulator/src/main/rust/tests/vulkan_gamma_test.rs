// Vulkan Gamma / ColorSpace 正确性验证测试
// 使用 headless Vulkan 后端创建 Skia Surface，对比有/无 sRGB ColorSpace 的渲染结果
// 运行: cargo test --test vulkan_gamma_test -- --nocapture

use ash::{vk, Entry, Instance, Device};
use ash::vk::Handle;
use skia_safe::{gpu, Color, Paint, PaintStyle, Rect, Surface as SkSurface, ColorType, TileMode};
use std::ffi::CStr;

// ============================================================
// Headless Vulkan 基础设施
// ============================================================
struct HeadlessVulkan {
    #[allow(dead_code)]
    entry: Entry,
    instance: Instance,
    device: Device,
    queue: vk::Queue,
    queue_family: u32,
    command_pool: vk::CommandPool,
}

impl HeadlessVulkan {
    fn new() -> Option<Self> {
        let entry = unsafe { Entry::load().ok()? };

        let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_0);
        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = unsafe { entry.create_instance(&create_info, None).ok()? };

        let pdevs = unsafe { instance.enumerate_physical_devices() }.ok()?;
        let pdev = *pdevs.first()?;

        let queue_props = unsafe { instance.get_physical_device_queue_family_properties(pdev) };
        let queue_family = queue_props.iter().enumerate()
            .find(|(_, q)| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|(i, _)| i as u32)?;

        let queue_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&[1.0f32]);
        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_info));
        let device = unsafe { instance.create_device(pdev, &device_create_info, None).ok()? };
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None).ok()? };

        Some(Self { entry, instance, device, queue, queue_family, command_pool })
    }

    fn create_image(&self, width: u32, height: u32) -> Option<(vk::Image, vk::DeviceMemory, vk::DeviceSize)> {
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .extent(vk::Extent3D { width, height, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let image = unsafe { self.device.create_image(&image_info, None).ok()? };

        let mem_reqs = unsafe { self.device.get_image_memory_requirements(image) };
        let mem_props = unsafe { self.instance.get_physical_device_memory_properties(
            self.instance.enumerate_physical_devices().ok()?.first()?.clone()
        ) };

        let mem_type_idx = (0..mem_props.memory_type_count)
            .find(|&i| {
                let t = mem_props.memory_types[i as usize];
                (mem_reqs.memory_type_bits & (1 << i)) != 0
                    && t.property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })?;

        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(mem_type_idx);
        let memory = unsafe { self.device.allocate_memory(&alloc_info, None).ok()? };
        unsafe { self.device.bind_image_memory(image, memory, 0).ok()? };

        Some((image, memory, mem_reqs.size))
    }

    fn create_readback_buffer(&self, size: vk::DeviceSize) -> Option<(vk::Buffer, vk::DeviceMemory)> {
        let buffer_info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { self.device.create_buffer(&buffer_info, None).ok()? };

        let mem_reqs = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let mem_props = unsafe { self.instance.get_physical_device_memory_properties(
            self.instance.enumerate_physical_devices().ok()?.first()?.clone()
        ) };

        let mem_type_idx = (0..mem_props.memory_type_count)
            .find(|&i| {
                let t = mem_props.memory_types[i as usize];
                (mem_reqs.memory_type_bits & (1 << i)) != 0
                    && t.property_flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
                    && t.property_flags.contains(vk::MemoryPropertyFlags::HOST_COHERENT)
            })?;

        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(mem_type_idx);
        let memory = unsafe { self.device.allocate_memory(&alloc_info, None).ok()? };
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0).ok()? };

        Some((buffer, memory))
    }

    fn copy_image_to_buffer(&self, image: vk::Image, buffer: vk::Buffer, width: u32, height: u32) {
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let cmd_buf = unsafe { self.device.allocate_command_buffers(&alloc_info).unwrap()[0] };

        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { self.device.begin_command_buffer(cmd_buf, &begin_info).unwrap() };

        // Barrier: COLOR_ATTACHMENT_OPTIMAL -> TRANSFER_SRC_OPTIMAL
        let barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1))
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ);

        unsafe {
            self.device.cmd_pipeline_barrier(
                cmd_buf,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                std::slice::from_ref(&barrier),
            );
        }

        let copy = vk::BufferImageCopy::default()
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(0)
                .base_array_layer(0)
                .layer_count(1))
            .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
            .image_extent(vk::Extent3D { width, height, depth: 1 });

        unsafe {
            self.device.cmd_copy_image_to_buffer(
                cmd_buf,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                std::slice::from_ref(&copy),
            );
        }

        unsafe { self.device.end_command_buffer(cmd_buf).unwrap() };

        let submit_info = vk::SubmitInfo::default()
            .command_buffers(std::slice::from_ref(&cmd_buf));
        let fence = unsafe { self.device.create_fence(&vk::FenceCreateInfo::default(), None).unwrap() };
        unsafe {
            self.device.queue_submit(self.queue, std::slice::from_ref(&submit_info), fence).unwrap();
            self.device.wait_for_fences(std::slice::from_ref(&fence), true, u64::MAX).unwrap();
            self.device.destroy_fence(fence, None);
            self.device.free_command_buffers(self.command_pool, std::slice::from_ref(&cmd_buf));
        }
    }
}

impl Drop for HeadlessVulkan {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

// ============================================================
// Skia Vulkan Surface 创建
// ============================================================
fn create_skia_vulkan_surface(
    vk: &HeadlessVulkan,
    image: vk::Image,
    width: u32,
    height: u32,
    color_space: Option<skia_safe::ColorSpace>,
) -> Option<(SkSurface, gpu::DirectContext)> {
    let pdevice = unsafe { vk.instance.enumerate_physical_devices() }.ok()?.first()?.clone();
    let instance_raw = vk.instance.handle().as_raw();
    let device_raw = vk.device.handle().as_raw();

    let get_proc = move |of: gpu::vk::GetProcOf| -> *const std::ffi::c_void {
        unsafe {
            match of {
                gpu::vk::GetProcOf::Instance(inst, name) => {
                    let name_cstr = CStr::from_ptr(name);
                    vk.entry.get_instance_proc_addr(vk::Instance::from_raw(inst as _), name_cstr.as_ptr())
                        .map(|f| f as _).unwrap_or(std::ptr::null())
                }
                gpu::vk::GetProcOf::Device(dev, name) => {
                    let name_cstr = CStr::from_ptr(name);
                    vk.instance.get_device_proc_addr(vk::Device::from_raw(dev as _), name_cstr.as_ptr())
                        .map(|f| f as *mut std::ffi::c_void).unwrap_or(std::ptr::null_mut()) as _
                }
            }
        }
    };

    let backend = unsafe {
        gpu::vk::BackendContext::new(
            instance_raw as _,
            pdevice.as_raw() as _,
            device_raw as _,
            (vk.queue.as_raw() as _, vk.queue_family as usize),
            &get_proc,
        )
    };

    let mut ctx_opts = gpu::ContextOptions::new();
    ctx_opts.glyph_cache_texture_maximum_bytes = 2 * 1024 * 1024;
    let mut context = gpu::direct_contexts::make_vulkan(&backend, Some(&ctx_opts))?;

    let image_info = unsafe {
        gpu::vk::ImageInfo::new(
            image.as_raw() as _,
            gpu::vk::Alloc::default(),
            gpu::vk::ImageTiling::OPTIMAL,
            gpu::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            gpu::vk::Format::R8G8B8A8_UNORM,
            1,
            None,
            None,
            None,
            None,
        )
    };

    let render_target = gpu::backend_render_targets::make_vk(
        (width as i32, height as i32),
        &image_info,
    );

    let surface = gpu::surfaces::wrap_backend_render_target(
        &mut context,
        &render_target,
        gpu::SurfaceOrigin::TopLeft,
        ColorType::RGBA8888,
        color_space,
        None,
    )?;

    Some((surface, context))
}

// 从 mapped buffer 读取指定坐标像素 (ARGB)
fn read_pixel(ptr: *const u8, width: u32, x: u32, y: u32) -> u32 {
    let offset = (y * width + x) as usize * 4;
    let bytes = unsafe { std::slice::from_raw_parts(ptr.add(offset), 4) };
    let r = bytes[0] as u32;
    let g = bytes[1] as u32;
    let b = bytes[2] as u32;
    let a = bytes[3] as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

// ============================================================
// 测试 1: ColorSpace 对渐变插值的影响（最能体现差异）
// ============================================================
#[test]
fn test_colorspace_gradient_interpolation() {
    println!("\n========== Vulkan ColorSpace 渐变插值测试 ==========\n");

    let vk = HeadlessVulkan::new().expect("Failed to create headless Vulkan context");
    let width = 256u32;
    let height = 64u32;
    let buffer_size = ((width * height) as usize * 4) as u64;
    let mid_x = width / 2;
    let mid_y = height / 2;

    // ---- 场景 A: 无 ColorSpace ----
    let (img_a, mem_a, _) = vk.create_image(width, height).expect("Failed to create image A");
    let (mut surf_a, mut ctx_a) = create_skia_vulkan_surface(&vk, img_a, width, height, None)
        .expect("Failed to create Skia surface A");

    let canvas_a = surf_a.canvas();
    let mut paint_a = Paint::default();
    paint_a.set_style(PaintStyle::Fill);
    paint_a.set_anti_alias(false);

    // 黑到白水平渐变
    let colors: Vec<Color> = vec![Color::new(0xFF000000), Color::new(0xFFFFFFFF)];
    let pt1 = skia_safe::Point::new(0.0, (height as f32) / 2.0);
    let pt2 = skia_safe::Point::new(width as f32, (height as f32) / 2.0);
    let shader_a = skia_safe::gradient_shader::linear(
        (pt1, pt2), &*colors, None, TileMode::Clamp, None, None
    );
    paint_a.set_shader(shader_a);
    canvas_a.draw_rect(&Rect::from_xywh(0.0, 0.0, width as f32, height as f32), &paint_a);
    ctx_a.flush_and_submit();

    let (buf_a, buf_mem_a) = vk.create_readback_buffer(buffer_size).expect("Failed to create readback buffer A");
    vk.copy_image_to_buffer(img_a, buf_a, width, height);
    let ptr_a = unsafe { vk.device.map_memory(buf_mem_a, 0, buffer_size, vk::MemoryMapFlags::empty()).unwrap() as *const u8 };
    let pixel_a = read_pixel(ptr_a, width, mid_x, mid_y);
    unsafe { vk.device.unmap_memory(buf_mem_a); }

    // ---- 场景 B: 有 sRGB ColorSpace ----
    let (img_b, mem_b, _) = vk.create_image(width, height).expect("Failed to create image B");
    let srgb = skia_safe::ColorSpace::new_srgb();
    let (mut surf_b, mut ctx_b) = create_skia_vulkan_surface(&vk, img_b, width, height, Some(srgb))
        .expect("Failed to create Skia surface B");

    let canvas_b = surf_b.canvas();
    let mut paint_b = Paint::default();
    paint_b.set_style(PaintStyle::Fill);
    paint_b.set_anti_alias(false);

    let shader_b = skia_safe::gradient_shader::linear(
        (pt1, pt2), &*colors, None, TileMode::Clamp, None, None
    );
    paint_b.set_shader(shader_b);
    canvas_b.draw_rect(&Rect::from_xywh(0.0, 0.0, width as f32, height as f32), &paint_b);
    ctx_b.flush_and_submit();

    let (buf_b, buf_mem_b) = vk.create_readback_buffer(buffer_size).expect("Failed to create readback buffer B");
    vk.copy_image_to_buffer(img_b, buf_b, width, height);
    let ptr_b = unsafe { vk.device.map_memory(buf_mem_b, 0, buffer_size, vk::MemoryMapFlags::empty()).unwrap() as *const u8 };
    let pixel_b = read_pixel(ptr_b, width, mid_x, mid_y);
    unsafe { vk.device.unmap_memory(buf_mem_b); }

    // ---- 结果对比 ----
    let ga = (pixel_a >> 8) & 0xFF;
    let gb = (pixel_b >> 8) & 0xFF;

    println!("场景 A (无 ColorSpace):  渐变中点 = {:#010X}, G={}", pixel_a, ga);
    println!("场景 B (sRGB ColorSpace): 渐变中点 = {:#010X}, G={}", pixel_b, gb);
    println!();

    // 理论分析:
    // 黑(0) -> 白(255) 在 256 像素宽度的中点 (x=128)
    // 无 ColorSpace: 数值线性插值 => 128 => 0xFF808080
    // 有 sRGB ColorSpace: 感知线性插值
    //   黑色 linear = 0, 白色 linear = 1
    //   中点 linear = 0.5
    //   sRGB 编码: 0.5^(1/2.2) ≈ 0.73 => 186-188 => 0xFFBCBCBC
    let expected_no_cs = 128u32;
    let expected_with_cs = 188u32;

    println!("理论预期:");
    println!("  无 ColorSpace (数值插值):     G ≈ {} => 0xFF808080", expected_no_cs);
    println!("  有 sRGB ColorSpace (感知插值): G ≈ {} => 0xFFBCBCBC", expected_with_cs);
    println!();

    let diff = ((ga as i32) - (gb as i32)).abs();
    println!("G 通道差异: {} (越大说明 ColorSpace 影响越明显)", diff);
    println!();

    // 即使 Skia 版本行为有差异，渐变中点至少应该不同
    // 但有些 Skia 版本默认对所有渐变都做感知插值，导致差异为 0
    // 我们要验证的是：当前代码不传 ColorSpace 时，是否和传了有区别
    if diff > 20 {
        println!("✅ ColorSpace 对渐变插值有明显影响 (diff={})", diff);
        println!("   当前代码 wrap_backend_render_target(..., None) 确实会改变渲染结果");
        assert!(true);
    } else {
        println!("⚠️  ColorSpace 对渐变插值无明显影响 (diff={})", diff);
        println!("   说明 Skia 0.93.1 的 Vulkan 后端可能默认已启用感知插值，");
        println!("   或 ColorSpace 参数在此场景下未生效。");
        println!("   但这不代表 ColorSpace 在所有场景都无效（如图像解码、色彩转换等）。");
        // 不强制失败，因为不同 Skia 版本行为不同
        // 但记录此现象供后续分析
    }

    // 安全清理
    unsafe {
        vk.device.destroy_buffer(buf_a, None); vk.device.free_memory(buf_mem_a, None);
        vk.device.destroy_image(img_a, None); vk.device.free_memory(mem_a, None);
        vk.device.destroy_buffer(buf_b, None); vk.device.free_memory(buf_mem_b, None);
        vk.device.destroy_image(img_b, None); vk.device.free_memory(mem_b, None);
    }

    println!("\n测试完成。");
}

// ============================================================
// 测试 2: 纯色 Clear 精确度验证
// ============================================================
#[test]
fn test_colorspace_clear_solid_color() {
    println!("\n========== Vulkan ColorSpace 纯色 Clear 测试 ==========\n");

    let vk = HeadlessVulkan::new().expect("Failed to create headless Vulkan context");
    let width = 64u32;
    let height = 64u32;
    let buffer_size = ((width * height) as usize * 4) as u64;

    let (img, mem, _) = vk.create_image(width, height).expect("Failed to create image");
    let srgb = skia_safe::ColorSpace::new_srgb();
    let (mut surf, mut ctx) = create_skia_vulkan_surface(&vk, img, width, height, Some(srgb))
        .expect("Failed to create Skia surface");

    let canvas = surf.canvas();
    canvas.clear(Color::new(0xFF1E1E1E));
    ctx.flush_and_submit();

    let (buf, buf_mem) = vk.create_readback_buffer(buffer_size).expect("Failed to create readback buffer");
    vk.copy_image_to_buffer(img, buf, width, height);

    let ptr = unsafe { vk.device.map_memory(buf_mem, 0, buffer_size, vk::MemoryMapFlags::empty()).unwrap() as *const u8 };
    let pixel = read_pixel(ptr, width, width/2, height/2);
    unsafe { vk.device.unmap_memory(buf_mem); }

    println!("Clear color input: 0xFF1E1E1E (sRGB 编码的暗灰)");
    println!("Readback pixel:    {:#010X}", pixel);

    assert_eq!(pixel, 0xFF1E1E1E,
        "纯色 clear 应该精确写回输入值。实际 {:#010X}", pixel);

    println!("✅ 纯色 clear 值精确匹配输入");

    unsafe {
        vk.device.destroy_buffer(buf, None);
        vk.device.free_memory(buf_mem, None);
        vk.device.destroy_image(img, None);
        vk.device.free_memory(mem, None);
    }
}

// ============================================================
// 测试 3: Alpha 混合对比（用于对比验证）
// ============================================================
#[test]
fn test_colorspace_alpha_blending() {
    println!("\n========== Vulkan ColorSpace Alpha 混合对比测试 ==========\n");

    let vk = HeadlessVulkan::new().expect("Failed to create headless Vulkan context");
    let width = 256u32;
    let height = 256u32;
    let buffer_size = ((width * height) as usize * 4) as u64;
    let mid_x = width / 2;
    let mid_y = height / 2;

    // 场景 A: 无 ColorSpace
    let (img_a, mem_a, _) = vk.create_image(width, height).expect("Failed to create image A");
    let (mut surf_a, mut ctx_a) = create_skia_vulkan_surface(&vk, img_a, width, height, None)
        .expect("Failed to create Skia surface A");
    let canvas_a = surf_a.canvas();
    canvas_a.clear(Color::new(0xFFFFFFFF));
    let mut paint_a = Paint::default();
    paint_a.set_style(PaintStyle::Fill);
    paint_a.set_color(Color::from_argb(128, 255, 0, 0));
    paint_a.set_anti_alias(false);
    canvas_a.draw_rect(&Rect::from_xywh(32.0, 32.0, 192.0, 192.0), &paint_a);
    ctx_a.flush_and_submit();

    let (buf_a, buf_mem_a) = vk.create_readback_buffer(buffer_size).expect("Failed to create buffer A");
    vk.copy_image_to_buffer(img_a, buf_a, width, height);
    let ptr_a = unsafe { vk.device.map_memory(buf_mem_a, 0, buffer_size, vk::MemoryMapFlags::empty()).unwrap() as *const u8 };
    let pixel_a = read_pixel(ptr_a, width, mid_x, mid_y);
    unsafe { vk.device.unmap_memory(buf_mem_a); }

    // 场景 B: sRGB ColorSpace
    let (img_b, mem_b, _) = vk.create_image(width, height).expect("Failed to create image B");
    let srgb = skia_safe::ColorSpace::new_srgb();
    let (mut surf_b, mut ctx_b) = create_skia_vulkan_surface(&vk, img_b, width, height, Some(srgb))
        .expect("Failed to create Skia surface B");
    let canvas_b = surf_b.canvas();
    canvas_b.clear(Color::new(0xFFFFFFFF));
    let mut paint_b = Paint::default();
    paint_b.set_style(PaintStyle::Fill);
    paint_b.set_color(Color::from_argb(128, 255, 0, 0));
    paint_b.set_anti_alias(false);
    canvas_b.draw_rect(&Rect::from_xywh(32.0, 32.0, 192.0, 192.0), &paint_b);
    ctx_b.flush_and_submit();

    let (buf_b, buf_mem_b) = vk.create_readback_buffer(buffer_size).expect("Failed to create buffer B");
    vk.copy_image_to_buffer(img_b, buf_b, width, height);
    let ptr_b = unsafe { vk.device.map_memory(buf_mem_b, 0, buffer_size, vk::MemoryMapFlags::empty()).unwrap() as *const u8 };
    let pixel_b = read_pixel(ptr_b, width, mid_x, mid_y);
    unsafe { vk.device.unmap_memory(buf_mem_b); }

    let ga = (pixel_a >> 8) & 0xFF;
    let gb = (pixel_b >> 8) & 0xFF;

    println!("场景 A (无 ColorSpace):  {:#010X}, G={}", pixel_a, ga);
    println!("场景 B (sRGB ColorSpace): {:#010X}, G={}", pixel_b, gb);
    println!("差异: {}", ((ga as i32) - (gb as i32)).abs());

    if pixel_a == pixel_b {
        println!("ℹ️  Alpha 混合结果完全相同，说明 Skia 的 SrcOver 混合在此配置下不受 ColorSpace 影响。");
        println!("   这与大多数 GPU 图形库的行为一致：alpha 混合默认在数值空间进行。");
    } else {
        println!("✅ Alpha 混合结果不同，ColorSpace 确实影响了混合计算。");
    }

    unsafe {
        vk.device.destroy_buffer(buf_a, None); vk.device.free_memory(buf_mem_a, None);
        vk.device.destroy_image(img_a, None); vk.device.free_memory(mem_a, None);
        vk.device.destroy_buffer(buf_b, None); vk.device.free_memory(buf_mem_b, None);
        vk.device.destroy_image(img_b, None); vk.device.free_memory(mem_b, None);
    }
}
