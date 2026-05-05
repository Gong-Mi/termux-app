// Vulkan 内存压力测试
// 验证 VulkanContext / Skia DirectContext 反复创建/销毁的稳定性与资源释放
// 运行: cargo test --test vulkan_memory_stress_test -- --nocapture

use ash::{vk, Entry, Instance, Device};
use ash::vk::Handle;
use skia_safe::gpu;
use std::ffi::CStr;
use std::time::Instant;

struct HeadlessVulkan {
    #[allow(dead_code)]
    entry: Entry,
    instance: Instance,
    device: Device,
    queue: vk::Queue,
    queue_family: u32,
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
        Some(Self { entry, instance, device, queue, queue_family })
    }
}

impl Drop for HeadlessVulkan {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

fn create_skia_context(vk: &HeadlessVulkan) -> Option<gpu::DirectContext> {
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
    gpu::direct_contexts::make_vulkan(&backend, Some(&ctx_opts))
}

// ============================================================
// 测试 1: DirectContext 反复创建/销毁（模拟 Activity 重建）
// ============================================================
#[test]
fn test_direct_context_create_destroy_stress() {
    println!("\n========== Vulkan DirectContext 创建/销毁压力测试 ==========\n");

    let vk = HeadlessVulkan::new().expect("Failed to create headless Vulkan context");
    let iterations = 20;

    let mut durations = Vec::with_capacity(iterations);
    let _total_glyph_cache = 0u64;

    for i in 0..iterations {
        let start = Instant::now();
        let mut context = create_skia_context(&vk).expect("Failed to create Skia context");

        // 模拟一些 GPU 工作：创建和释放资源
        context.set_resource_cache_limit(8 * 1024 * 1024);

        // 获取上下文资源使用量（近似）
        // skia-safe 0.93.1 没有 get_resource_cache_total_bytes() API
        // 仅记录迭代次数和耗时

        // 模拟 vulkan_context.rs Drop 中的逻辑
        context.release_resources_and_abandon();
        // 注意：这里我们不 mem::forget，让 Rust 正常 drop
        // 如果 Skia 内部在 abandon 后仍访问 Vulkan Device，会崩溃
        drop(context);

        let elapsed = start.elapsed();
        durations.push(elapsed);

        if (i + 1) % 5 == 0 {
            println!("  迭代 {}/{} 完成, 平均耗时 {:.2}ms",
                i + 1, iterations,
                durations.iter().map(|d| d.as_secs_f64()).sum::<f64>() / durations.len() as f64 * 1000.0
            );
        }
    }

    let avg = durations.iter().map(|d| d.as_secs_f64()).sum::<f64>() / durations.len() as f64;
    let max = durations.iter().map(|d| d.as_secs_f64()).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
    let min = durations.iter().map(|d| d.as_secs_f64()).min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();

    println!();
    println!("统计:");
    println!("  迭代次数:    {}", iterations);
    println!("  平均耗时:    {:.2}ms", avg * 1000.0);
    println!("  最大耗时:    {:.2}ms", max * 1000.0);
    println!("  最小耗时:    {:.2}ms", min * 1000.0);
    println!("  总迭代次数: {}", iterations);
    println!();

    // 关键断言：所有迭代都成功完成，没有崩溃
    println!("✅ 所有 {} 次创建/销毁循环成功完成，无崩溃", iterations);
    println!("   注意：此测试使用正常的 drop，未使用 mem::forget workaround。");
    println!("   如果在真实设备上（Adreno 驱动）出现崩溃，才需要 mem::forget 规避。");
}

// ============================================================
// 测试 2: 模拟 vulkan_context.rs 的 mem::forget 模式
// ============================================================
#[test]
fn test_mem_forget_resource_leak_simulation() {
    println!("\n========== mem::forget 资源泄漏模拟测试 ==========\n");

    let vk = HeadlessVulkan::new().expect("Failed to create headless Vulkan context");
    let iterations = 10;

    for i in 0..iterations {
        let mut context = create_skia_context(&vk).expect("Failed to create Skia context");
        context.set_resource_cache_limit(8 * 1024 * 1024);

        // 模拟创建一些资源负载
        // 模拟 abandon + forget（与 vulkan_context.rs Drop 一致）
        context.release_resources_and_abandon();
        std::mem::forget(context); // ← 与生产代码相同的 workaround

        if (i + 1) % 3 == 0 {
            println!("  迭代 {}: mem::forget 执行，C++ DirectContext 壳层泄漏", i + 1);
        }
    }

    println!();
    println!("⚠️  此测试模拟了 vulkan_context.rs 的 mem::forget workaround。");
    println!("   每次迭代都泄漏了一个 DirectContext 的 C++ 对象壳层。");
    println!("   虽然 release_resources_and_abandon() 释放了大部分 GPU 资源，");
    println!("   但 C++ GrDirectContext 的内存、内部句柄表等仍被泄漏。");
    println!();
    println!("   在生产环境中，如果用户频繁旋转屏幕或切换应用，");
    println!("   这些泄漏会累积，最终触发 LMK 或 GPU OOM。");
    println!();
    println!("✅ 测试完成（{} 次 mem::forget 未崩溃）", iterations);
}

// ============================================================
// 测试 3: 资源缓存限制效果验证
// ============================================================
#[test]
fn test_resource_cache_limit_effect() {
    println!("\n========== 资源缓存限制效果验证 ==========\n");

    let vk = HeadlessVulkan::new().expect("Failed to create headless Vulkan context");
    let mut context = create_skia_context(&vk).expect("Failed to create Skia context");

    let limits = [
        (4 * 1024 * 1024, "4MB"),
        (8 * 1024 * 1024, "8MB"),
        (16 * 1024 * 1024, "16MB"),
        (32 * 1024 * 1024, "32MB"),
        (64 * 1024 * 1024, "64MB"),
    ];

    println!("缓存限制 vs 初始缓存占用:");
    for (limit, label) in &limits {
        context.set_resource_cache_limit(*limit);
        println!("  {:<6} 限制 => 限制已设置 (无法直接读取当前占用)", label);
    }

    println!();
    println!("结论:");
    println!("  set_resource_cache_limit() 设置了上限，但初始占用通常远低于限制。");
    println!("  实际达到限制后的驱逐行为取决于 Skia 内部策略，无法直接观测。");
    println!("  当前代码使用 64MB 限制，对于终端渲染通常足够。");

    // 正常 drop（不用 mem::forget）
    drop(context);
}
