// Vulkan Format 探测测试
// 验证物理设备支持的 surface format，揭露硬编码 R8G8B8A8_UNORM 的匹配风险
// 运行: cargo test --test vulkan_format_probe -- --nocapture

use ash::{vk, Entry, Instance};
use std::ffi::CStr;

fn find_physical_device(instance: &Instance) -> Option<vk::PhysicalDevice> {
    let devices = unsafe { instance.enumerate_physical_devices() }.ok()?;
    devices.into_iter().next()
}

#[test]
fn test_physical_device_format_support() {
    println!("\n========== Vulkan Format 探测测试 ==========\n");

    let entry = unsafe { Entry::load().expect("Failed to load Vulkan entry") };

    let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_0);
    let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
    let instance = unsafe { entry.create_instance(&create_info, None).expect("Failed to create Vulkan instance") };

    let pdev = find_physical_device(&instance).expect("No Vulkan physical device found");
    let props = unsafe { instance.get_physical_device_properties(pdev) };
    let device_name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) };
    println!("设备: {}", device_name.to_string_lossy());
    println!("驱动版本: {}.{}.{}",
        vk::api_version_major(props.driver_version),
        vk::api_version_minor(props.driver_version),
        vk::api_version_patch(props.driver_version)
    );
    println!();

    // 查询物理设备所有支持的 OPTIMAL image format
    let test_formats = [
        (vk::Format::R8G8B8A8_UNORM, "R8G8B8A8_UNORM"),
        (vk::Format::B8G8R8A8_UNORM, "B8G8R8A8_UNORM"),
        (vk::Format::A8B8G8R8_UNORM_PACK32, "A8B8G8R8_UNORM_PACK32"),
        (vk::Format::R8G8B8A8_SRGB, "R8G8B8A8_SRGB"),
        (vk::Format::B8G8R8A8_SRGB, "B8G8R8A8_SRGB"),
        (vk::Format::R5G6B5_UNORM_PACK16, "R5G6B5_UNORM_PACK16"),
        (vk::Format::A1R5G5B5_UNORM_PACK16, "A1R5G5B5_UNORM_PACK16"),
        (vk::Format::R16G16B16A16_SFLOAT, "R16G16B16A16_SFLOAT"),
    ];

    println!("支持的 COLOR_ATTACHMENT 格式 (OPTIMAL tiling):");
    let mut supports_rgba = false;
    let mut supports_bgra = false;
    let mut first_supported = None;

    for (fmt, name) in &test_formats {
        let props2 = vk::FormatProperties2::default();
        let mut fmt_props = vk::FormatProperties2::default();
        unsafe {
            instance.get_physical_device_format_properties2(pdev, *fmt, &mut fmt_props);
        }
        let optimal = fmt_props.format_properties.optimal_tiling_features;
        let supported = optimal.contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT);
        let flag_str = if supported { "✅ 支持" } else { "❌ 不支持" };
        println!("  {:<30} {}", name, flag_str);

        if supported {
            if first_supported.is_none() {
                first_supported = Some(*name);
            }
            if *fmt == vk::Format::R8G8B8A8_UNORM {
                supports_rgba = true;
            }
            if *fmt == vk::Format::B8G8R8A8_UNORM {
                supports_bgra = true;
            }
        }
    }

    println!();

    // 关键断言：如果设备优先支持 BGRA 但代码硬编码 RGBA，就有风险
    if supports_bgra && !supports_rgba {
        println!("⚠️  警告: 此设备支持 BGRA 但不支持 RGBA！");
        println!("   当前代码硬编码 Format::R8G8B8A8_UNORM，会导致红蓝通道互换！");
    }

    if !supports_rgba {
        println!("⚠️  严重: 设备不支持 R8G8B8A8_UNORM！");
        println!("   当前 create_sk_surface() 硬编码此格式，必然创建失败。");
    }

    // 无论什么结果都打印 first supported format
    println!("设备首选支持的格式: {:?}", first_supported);
    println!();

    // 不强制 panic，只做信息输出（因为不同设备支持不同）
    // 但如果在 Android 设备上 BGRA 是唯一支持的 UNORM 格式，需要记录
    println!("结论:");
    println!("  - 当前 vulkan_context.rs:605 硬编码 skia_safe::gpu::vk::Format::R8G8B8A8_UNORM");
    println!("  - 而 recreate_swapchain() 查询的 surface_formats 可能返回 B8G8R8A8_UNORM");
    println!("  - 两者不匹配时，Skia ImageInfo 与实际 Vulkan Image 格式不一致");
    println!("  - 后果: 色彩通道错乱、Validation Error、或驱动崩溃");
    println!();

    unsafe { instance.destroy_instance(None); };
}

#[test]
fn test_surfaceless_query_colorspace() {
    // 如果 VK_GOOGLE_surfaceless_query 可用，查询设备默认的 surface format
    println!("\n========== Surfaceless ColorSpace 查询 ==========\n");

    let entry = unsafe { Entry::load().expect("Failed to load Vulkan entry") };

    let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_0);
    let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
    let instance = unsafe { entry.create_instance(&create_info, None).expect("Failed to create Vulkan instance") };

    let pdev = find_physical_device(&instance).expect("No Vulkan physical device found");

    // 查询物理设备属性，看是否有 VK_GOOGLE_surfaceless_query
    let ext_props = unsafe { instance.enumerate_device_extension_properties(pdev).unwrap_or_default() };
    let has_surfaceless = ext_props.iter().any(|p| {
        let name = unsafe { CStr::from_ptr(p.extension_name.as_ptr()) };
        name.to_str().unwrap_or("") == "VK_GOOGLE_surfaceless_query"
    });

    println!("VK_GOOGLE_surfaceless_query 支持: {}", if has_surfaceless { "是" } else { "否" });

    // 查询设备级别的 color space 支持
    let mut props2 = vk::PhysicalDeviceProperties2::default();
    unsafe { instance.get_physical_device_properties2(pdev, &mut props2) };
    println!("API version: {}.{}.{}",
        vk::api_version_major(props2.properties.api_version),
        vk::api_version_minor(props2.properties.api_version),
        vk::api_version_patch(props2.properties.api_version)
    );

    unsafe { instance.destroy_instance(None); };
}
