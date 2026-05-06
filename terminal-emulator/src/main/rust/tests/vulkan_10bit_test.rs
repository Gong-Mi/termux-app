// Vulkan 10-bit 验证测试 (精简版)
// 运行: cargo test --test vulkan_10bit_test -- --nocapture

use ash::vk::Handle;
use ash::{Device, Entry, Instance, vk};
use skia_safe::{ColorType, Surface as SkSurface, gpu};
use std::ffi::CStr;

struct HeadlessVulkan {
    #[allow(dead_code)]
    entry: Entry,
    instance: Instance,
    device: Device,
    queue: vk::Queue,
    queue_family: u32,
    pdev: vk::PhysicalDevice,
}

impl HeadlessVulkan {
    fn new() -> Option<Self> {
        let entry = unsafe { Entry::load().ok()? };
        let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = unsafe { entry.create_instance(&create_info, None).ok()? };

        let pdevs = unsafe { instance.enumerate_physical_devices() }.ok()?;
        let pdev = *pdevs.first()?;

        let queue_props = unsafe { instance.get_physical_device_queue_family_properties(pdev) };
        let queue_family = queue_props
            .iter()
            .enumerate()
            .find(|(_, q)| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|(i, _)| i as u32)?;

        let queue_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&[1.0f32]);
        let device_create_info =
            vk::DeviceCreateInfo::default().queue_create_infos(std::slice::from_ref(&queue_info));
        let device = unsafe {
            instance
                .create_device(pdev, &device_create_info, None)
                .ok()?
        };
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        Some(Self {
            entry,
            instance,
            device,
            queue,
            queue_family,
            pdev,
        })
    }

    fn create_image(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
    ) -> Option<(vk::Image, vk::DeviceMemory)> {
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let image = unsafe { self.device.create_image(&image_info, None).ok()? };
        let mem_reqs = unsafe { self.device.get_image_memory_requirements(image) };
        let mem_props = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.pdev)
        };

        let mem_type_idx = (0..mem_props.memory_type_count).find(|&i| {
            let t = mem_props.memory_types[i as usize];
            (mem_reqs.memory_type_bits & (1 << i)) != 0
                && t.property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })?;

        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(mem_type_idx);
        let memory = unsafe { self.device.allocate_memory(&alloc_info, None).ok()? };
        unsafe { self.device.bind_image_memory(image, memory, 0).ok()? };

        Some((image, memory))
    }
}

fn create_skia_surface(
    vk: &HeadlessVulkan,
    image: vk::Image,
    width: u32,
    height: u32,
    vk_format: vk::Format,
    color_type: ColorType,
    color_space: Option<skia_safe::ColorSpace>,
) -> Option<(SkSurface, gpu::DirectContext)> {
    let get_proc = move |of: gpu::vk::GetProcOf| -> *const std::ffi::c_void {
        unsafe {
            match of {
                gpu::vk::GetProcOf::Instance(inst, name) => {
                    let name_cstr = CStr::from_ptr(name);
                    vk.entry
                        .get_instance_proc_addr(
                            vk::Instance::from_raw(inst as _),
                            name_cstr.as_ptr(),
                        )
                        .map(|f| f as _)
                        .unwrap_or(std::ptr::null())
                }
                gpu::vk::GetProcOf::Device(dev, name) => {
                    let name_cstr = CStr::from_ptr(name);
                    vk.instance
                        .get_device_proc_addr(vk::Device::from_raw(dev as _), name_cstr.as_ptr())
                        .map(|f| f as *mut std::ffi::c_void)
                        .unwrap_or(std::ptr::null_mut()) as _
                }
            }
        }
    };

    let backend = unsafe {
        gpu::vk::BackendContext::new(
            vk.instance.handle().as_raw() as _,
            vk.pdev.as_raw() as _,
            vk.device.handle().as_raw() as _,
            (vk.queue.as_raw() as _, vk.queue_family as usize),
            &get_proc,
        )
    };

    let mut context = gpu::direct_contexts::make_vulkan(&backend, None)?;
    let image_info = unsafe {
        gpu::vk::ImageInfo::new(
            image.as_raw() as _,
            gpu::vk::Alloc::default(),
            gpu::vk::ImageTiling::OPTIMAL,
            gpu::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            std::mem::transmute(vk_format),
            1,
            None,
            None,
            None,
            None,
        )
    };

    let render_target =
        gpu::backend_render_targets::make_vk((width as i32, height as i32), &image_info);
    let surface = gpu::surfaces::wrap_backend_render_target(
        &mut context,
        &render_target,
        gpu::SurfaceOrigin::TopLeft,
        color_type,
        color_space,
        None,
    )?;

    Some((surface, context))
}

#[test]
fn test_10bit_creation() {
    println!("\n========== Vulkan 10-bit Surface 创建测试 ==========\n");

    let vk = HeadlessVulkan::new().expect("Failed to create Vulkan context");

    // 检查硬件是否支持 A2B10G10R10_UNORM_PACK32
    let mut fmt_props = vk::FormatProperties2::default();
    unsafe {
        vk.instance.get_physical_device_format_properties2(
            vk.pdev,
            vk::Format::A2B10G10R10_UNORM_PACK32,
            &mut fmt_props,
        );
    }
    let supported = fmt_props
        .format_properties
        .optimal_tiling_features
        .contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT);

    println!(
        "硬件支持 A2B10G10R10_UNORM_PACK32: {}",
        if supported { "✅ 是" } else { "❌ 否" }
    );
    if !supported {
        return;
    }

    let width = 100u32;
    let height = 100u32;

    // 测试 1: 创建 10-bit Surface (无 ColorSpace)
    let (img1, _) = vk
        .create_image(width, height, vk::Format::A2B10G10R10_UNORM_PACK32)
        .unwrap();
    let res1 = create_skia_surface(
        &vk,
        img1,
        width,
        height,
        vk::Format::A2B10G10R10_UNORM_PACK32,
        ColorType::RGBA1010102,
        None,
    );
    println!(
        "创建 10-bit Surface (无 ColorSpace): {}",
        if res1.is_some() {
            "✅ 成功"
        } else {
            "❌ 失败"
        }
    );

    // 测试 2: 创建 10-bit Surface (sRGB ColorSpace)
    let (img2, _) = vk
        .create_image(width, height, vk::Format::A2B10G10R10_UNORM_PACK32)
        .unwrap();
    let srgb = skia_safe::ColorSpace::new_srgb();
    let res2 = create_skia_surface(
        &vk,
        img2,
        width,
        height,
        vk::Format::A2B10G10R10_UNORM_PACK32,
        ColorType::RGBA1010102,
        Some(srgb),
    );
    println!(
        "创建 10-bit Surface (sRGB ColorSpace): {}",
        if res2.is_some() {
            "✅ 成功"
        } else {
            "❌ 失败"
        }
    );

    // 测试 3: 创建 10-bit Surface (sRGB Linear ColorSpace)
    let (img3, _) = vk
        .create_image(width, height, vk::Format::A2B10G10R10_UNORM_PACK32)
        .unwrap();
    let srgb_linear = skia_safe::ColorSpace::new_srgb_linear();
    let res3 = create_skia_surface(
        &vk,
        img3,
        width,
        height,
        vk::Format::A2B10G10R10_UNORM_PACK32,
        ColorType::RGBA1010102,
        Some(srgb_linear),
    );
    println!(
        "创建 10-bit Surface (sRGB Linear ColorSpace): {}",
        if res3.is_some() {
            "✅ 成功"
        } else {
            "❌ 失败"
        }
    );
}
