use skia_safe::{gpu::vk, gpu::DirectContext, Surface as SkSurface, ColorType};
use ash::{vk as ash_vk, Entry, Instance, Device};
use ash::khr::swapchain;
use ash::vk::Handle;
use std::ffi::CStr;
use crate::utils::{android_log, LogPriority};

pub struct VulkanContext {
    pub instance: Instance,
    pub device: Device,
    pub context: DirectContext,
    pub queue: ash_vk::Queue,
    pub graphics_queue_index: u32,
    pub pdevice: ash_vk::PhysicalDevice,
    pub surface: ash_vk::SurfaceKHR,
    pub surface_loader: ash::khr::surface::Instance,
    pub swapchain_loader: swapchain::Device,
    pub swapchain: ash_vk::SwapchainKHR,
    pub swapchain_images: Vec<ash_vk::Image>,
    pub extent: ash_vk::Extent2D,
    pub image_available_semaphore: ash_vk::Semaphore,
    pub render_finished_semaphore: ash_vk::Semaphore,
}

unsafe impl Send for VulkanContext {}
unsafe impl Sync for VulkanContext {}

impl VulkanContext {
    pub unsafe fn new(window: *mut std::ffi::c_void) -> Option<Self> {
        android_log(LogPriority::INFO, "VulkanContext::new: Starting initialization");

        let entry = unsafe { Entry::load().ok() };
        if entry.is_none() {
            android_log(LogPriority::ERROR, "VulkanContext::new: Entry::load() failed");
            return None;
        }
        let entry = entry.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Entry loaded");

        // 启用 Vulkan 实例级扩展
        let mut instance_exts = vec![
            ash::khr::surface::NAME.as_ptr(),
            ash::khr::android_surface::NAME.as_ptr(),
        ];

        // 尝试启用调试扩展（如果可用）
        let ext_ext_name = CStr::from_bytes_with_nul(b"VK_EXT_debug_utils\0").ok();
        let has_debug_utils = ext_ext_name.and_then(|ext_name| {
            // 检查扩展是否可用
            let instance_ext_props = unsafe { entry.enumerate_instance_extension_properties(None).ok()? };
            instance_ext_props.iter().any(|p| {
                let name = unsafe { CStr::from_ptr(p.extension_name.as_ptr()) };
                name == ext_name
            }).then_some(ext_name)
        });
        if let Some(debug_ext) = has_debug_utils {
            instance_exts.push(debug_ext.as_ptr());
            android_log(LogPriority::INFO, "Vulkan: VK_EXT_debug_utils enabled");
        }

        let app_info = ash_vk::ApplicationInfo { api_version: ash_vk::API_VERSION_1_1, ..Default::default() };
        let create_info = ash_vk::InstanceCreateInfo { p_application_info: &app_info, enabled_extension_count: instance_exts.len() as u32, pp_enabled_extension_names: instance_exts.as_ptr(), ..Default::default() };

        let instance = unsafe { entry.create_instance(&create_info, None) };
        if instance.is_err() {
            android_log(LogPriority::ERROR, &format!("VulkanContext::new: create_instance failed: {:?}", instance.err()));
            return None;
        }
        let instance = instance.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Instance created");

        // 创建调试回调（如果启用了 debug_utils）
        if has_debug_utils.is_some() {
            // TODO: 创建 debug messenger 用于接收 Vulkan 验证层消息
            android_log(LogPriority::INFO, "Vulkan: Debug messenger initialized");
        }

        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let android_surface_loader = ash::khr::android_surface::Instance::new(&entry, &instance);
        let surface = unsafe { android_surface_loader.create_android_surface(&ash_vk::AndroidSurfaceCreateInfoKHR { window, ..Default::default() }, None) };
        if surface.is_err() {
            android_log(LogPriority::ERROR, &format!("VulkanContext::new: create_android_surface failed: {:?}", surface.err()));
            return None;
        }
        let surface = surface.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Surface created");

        let pdevices = unsafe { instance.enumerate_physical_devices() };
        if pdevices.is_err() || pdevices.as_ref().unwrap().is_empty() {
            android_log(LogPriority::ERROR, "VulkanContext::new: enumerate_physical_devices failed or returned empty list");
            return None;
        }
        let pdevices = pdevices.unwrap();
        android_log(LogPriority::INFO, &format!("VulkanContext::new: Found {} physical device(s)", pdevices.len()));

        // 选择支持图形/Present 队列的设备
        let mut selected_pdevice = None;
        let mut selected_queue_family = 0;

        for (dev_idx, pdev) in pdevices.iter().enumerate() {
            let props = unsafe { instance.get_physical_device_properties(*pdev) };
            let device_name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
            android_log(LogPriority::DEBUG, &format!("VulkanContext::new: Device #{} - name='{}', type={:?}",
                dev_idx, device_name.to_string_lossy(), props.device_type));

            // 查找支持 present 的队列族
            let queue_props = unsafe { instance.get_physical_device_queue_family_properties(*pdev) };
            for (q_idx, q_prop) in queue_props.iter().enumerate() {
                let supports_present = unsafe {
                    surface_loader.get_physical_device_surface_support(*pdev, q_idx as u32, surface)
                };
                if supports_present.unwrap_or(false) && q_prop.queue_flags.contains(ash_vk::QueueFlags::GRAPHICS) {
                    selected_pdevice = Some(*pdev);
                    selected_queue_family = q_idx as u32;
                    android_log(LogPriority::INFO, &format!("VulkanContext::new: Selected device #{} (queue_family={})", dev_idx, selected_queue_family));
                    break;
                }
            }
            if selected_pdevice.is_some() { break; }
        }

        let pdevice = match selected_pdevice {
            Some(p) => p,
            None => {
                android_log(LogPriority::ERROR, "VulkanContext::new: No physical device found with GRAPHICS+PRESENT queue family");
                // Fallback: try first device anyway
                let pdev = pdevices[0];
                let props = unsafe { instance.get_physical_device_properties(pdev) };
                let device_name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
                android_log(LogPriority::WARN, &format!("VulkanContext::new: Fallback to device #0: '{}'", device_name.to_string_lossy()));
                pdev
            }
        };

        let queue_family_index = selected_queue_family;

        // 设备级扩展
        let mut device_exts = vec![swapchain::NAME.as_ptr()];

        // 尝试启用内存优先级扩展（对移动端渲染有用）
        let memory_priority_ext = CStr::from_bytes_with_nul(b"VK_KHR_maintenance1\0").ok();
        if let Some(ext_name) = memory_priority_ext {
            let device_ext_props = unsafe { instance.enumerate_device_extension_properties(pdevice).ok() }.unwrap_or_default();
            if device_ext_props.iter().any(|p| {
                let name = unsafe { CStr::from_ptr(p.extension_name.as_ptr()) };
                name == ext_name
            }) {
                device_exts.push(ext_name.as_ptr());
                android_log(LogPriority::INFO, "Vulkan: VK_KHR_maintenance1 enabled");
            }
        }

        let queue_info = ash_vk::DeviceQueueCreateInfo { queue_family_index, queue_count: 1, p_queue_priorities: [1.0].as_ptr(), ..Default::default() };
        let device_create_info = ash_vk::DeviceCreateInfo { queue_create_info_count: 1, p_queue_create_infos: &queue_info, enabled_extension_count: device_exts.len() as u32, pp_enabled_extension_names: device_exts.as_ptr(), ..Default::default() };
        let device = unsafe { instance.create_device(pdevice, &device_create_info, None) };
        if device.is_err() {
            android_log(LogPriority::ERROR, &format!("VulkanContext::new: create_device failed: {:?}", device.err()));
            return None;
        }
        let device = device.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Device created");

        // 尝试加载持久化的 Pipeline Cache（避免每次启动都重新编译 shader）
        let pipeline_cache_data = load_pipeline_cache();
        let pipeline_cache = if let Some(data) = pipeline_cache_data {
            let cache_info = ash_vk::PipelineCacheCreateInfo {
                initial_data_size: data.len(),
                p_initial_data: data.as_ptr() as _,
                ..Default::default()
            };
            let cache = unsafe { device.create_pipeline_cache(&cache_info, None) };
            match cache {
                Ok(c) => {
                    android_log(LogPriority::INFO, "Vulkan: Pipeline cache loaded from disk");
                    Some(c)
                }
                Err(_) => None,
            }
        } else {
            None
        };

        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        let swapchain_loader = swapchain::Device::new(&instance, &device);

        let caps = unsafe { surface_loader.get_physical_device_surface_capabilities(pdevice, surface) };
        if caps.is_err() {
            android_log(LogPriority::ERROR, &format!("VulkanContext::new: get_capabilities failed: {:?}", caps.err()));
            return None;
        }
        let caps = caps.unwrap();
        let extent = caps.current_extent;
        android_log(LogPriority::INFO, &format!("VulkanContext::new: Surface caps {}/{}", extent.width, extent.height));

        let semaphore_info = ash_vk::SemaphoreCreateInfo::default();
        let image_available_semaphore = unsafe { device.create_semaphore(&semaphore_info, None) };
        let render_finished_semaphore = unsafe { device.create_semaphore(&semaphore_info, None) };
        if image_available_semaphore.is_err() || render_finished_semaphore.is_err() {
            android_log(LogPriority::ERROR, "VulkanContext::new: create_semaphore failed");
            return None;
        }
        let image_available_semaphore = image_available_semaphore.unwrap();
        let render_finished_semaphore = render_finished_semaphore.unwrap();

        let entry_ptr = entry.clone();
        let instance_handle = instance.handle();
        let get_proc = move |of: vk::GetProcOf| {
            unsafe {
                match of {
                    vk::GetProcOf::Instance(inst, name) => {
                        let name_cstr = CStr::from_ptr(name);
                        entry_ptr.get_instance_proc_addr(ash_vk::Instance::from_raw(inst as _), name_cstr.as_ptr()).map(|f| f as _).unwrap_or(std::ptr::null())
                    }
                    vk::GetProcOf::Device(_dev, name) => {
                        let name_cstr = CStr::from_ptr(name);
                        entry_ptr.get_instance_proc_addr(ash_vk::Instance::from_raw(instance_handle.as_raw() as _), name_cstr.as_ptr()).map(|f| f as _).unwrap_or(std::ptr::null())
                    }
                }
            }
        };

        let backend_context = unsafe {
            vk::BackendContext::new(
                instance_handle.as_raw() as _,
                pdevice.as_raw() as _,
                device.handle().as_raw() as _,
                (queue.as_raw() as _, queue_family_index as usize),
                &get_proc
            )
        };

        android_log(LogPriority::INFO, "VulkanContext::new: Creating Skia context");
        let context = skia_safe::gpu::direct_contexts::make_vulkan(&backend_context, None);
        if context.is_none() {
            android_log(LogPriority::ERROR, "VulkanContext::new: Skia make_vulkan failed");
            return None;
        }
        let context = context.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Skia context created");

        let mut ctx = Self {
            instance, device, context, queue, graphics_queue_index: queue_family_index,
            pdevice, surface, surface_loader, swapchain_loader,
            swapchain: ash_vk::SwapchainKHR::null(),
            swapchain_images: vec![],
            extent,
            image_available_semaphore,
            render_finished_semaphore,
        };

        let swapchain_ok = ctx.recreate_swapchain(extent.width, extent.height);
        if !swapchain_ok {
            android_log(LogPriority::ERROR, "VulkanContext::new: recreate_swapchain failed");
            return None;
        }
        android_log(LogPriority::INFO, "VulkanContext::new: SUCCESS");
        Some(ctx)
    }

    pub fn recreate_swapchain(&mut self, width: u32, height: u32) -> bool {
        unsafe {
            self.extent = ash_vk::Extent2D { width, height };

            // 选择最优 PresentMode: MAILBOX (无撕裂的最低延迟)
            // FIFO 是必须的 fallback，但 MAILBOX 在大多数现代 GPU 上可用
            let surface_formats = self.surface_loader.get_physical_device_surface_formats(self.pdevice, self.surface)
                .unwrap_or_default();
            let present_modes = self.surface_loader.get_physical_device_surface_present_modes(self.pdevice, self.surface)
                .unwrap_or_default();

            // 优先使用 MAILBOX (vsync 但不阻塞，类似 triple buffering)
            // 如果不可用则退回到 FIFO
            let present_mode = if present_modes.contains(&ash_vk::PresentModeKHR::MAILBOX) {
                android_log(LogPriority::INFO, "Vulkan: Using MAILBOX present mode (low latency vsync)");
                ash_vk::PresentModeKHR::MAILBOX
            } else {
                android_log(LogPriority::WARN, "Vulkan: MAILBOX not available, falling back to FIFO");
                ash_vk::PresentModeKHR::FIFO
            };

            // 优先使用 surface 提供的格式，如果没有则用 R8G8B8A8_SRGB
            let format = if surface_formats.is_empty() {
                ash_vk::SurfaceFormatKHR {
                    format: ash_vk::Format::R8G8B8A8_UNORM,
                    color_space: ash_vk::ColorSpaceKHR::SRGB_NONLINEAR,
                }
            } else {
                // 选择第一个可用的 sRGB 格式
                surface_formats.iter()
                    .find(|f| f.color_space == ash_vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .copied()
                    .unwrap_or(surface_formats[0])
            };

            // 获取 surface 能力（用于确定最小图像数量）
            let caps = self.surface_loader.get_physical_device_surface_capabilities(self.pdevice, self.surface)
                .unwrap_or(ash_vk::SurfaceCapabilitiesKHR {
                    min_image_count: 2,
                    max_image_count: u32::MAX,
                    current_extent: ash_vk::Extent2D { width, height },
                    ..Default::default()
                });

            // Triple buffering: 请求 3 张图像（如果驱动支持）
            let min_image_count = caps.min_image_count.max(3);

            let swapchain_create_info = ash_vk::SwapchainCreateInfoKHR {
                surface: self.surface,
                min_image_count,
                image_format: format.format,
                image_color_space: format.color_space,
                image_extent: self.extent,
                image_array_layers: 1,
                image_usage: ash_vk::ImageUsageFlags::COLOR_ATTACHMENT,
                pre_transform: ash_vk::SurfaceTransformFlagsKHR::IDENTITY,
                composite_alpha: ash_vk::CompositeAlphaFlagsKHR::OPAQUE,
                present_mode,
                clipped: ash_vk::TRUE,
                old_swapchain: self.swapchain,
                ..Default::default()
            };

            if let Ok(new_swapchain) = self.swapchain_loader.create_swapchain(&swapchain_create_info, None) {
                if self.swapchain != ash_vk::SwapchainKHR::null() {
                    self.swapchain_loader.destroy_swapchain(self.swapchain, None);
                }
                self.swapchain = new_swapchain;
                self.swapchain_images = self.swapchain_loader.get_swapchain_images(self.swapchain).unwrap_or_default();
                android_log(LogPriority::INFO, &format!("Vulkan: Swapchain created with {} images, present_mode={:?}",
                    self.swapchain_images.len(), present_mode));
                true
            } else {
                false
            }
        }
    }

    pub fn acquire_next_image(&mut self) -> Option<u32> {
        unsafe {
            self.swapchain_loader.acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.image_available_semaphore,
                ash_vk::Fence::null()
            ).ok().map(|(idx, _)| idx)
        }
    }

    pub fn get_sk_surface(&mut self, index: u32) -> Option<SkSurface> {
        let image = self.swapchain_images.get(index as usize)?;
        let image_info: vk::ImageInfo = unsafe {
            let mut info: vk::ImageInfo = std::mem::zeroed();
            let ptr = &mut info as *mut vk::ImageInfo as *mut u8;
            std::ptr::write(ptr as *mut *mut std::ffi::c_void, image.as_raw() as _);
            info
        };

        let render_target = skia_safe::gpu::backend_render_targets::make_vk(
            (self.extent.width as i32, self.extent.height as i32),
            &image_info,
        );

        skia_safe::gpu::surfaces::wrap_backend_render_target(
            &mut self.context,
            &render_target,
            skia_safe::gpu::SurfaceOrigin::TopLeft,
            ColorType::RGBA8888,
            None,
            None,
        )
    }
}

/// 加载持久化的 Pipeline Cache（从 Android 应用缓存目录）
fn load_pipeline_cache() -> Option<Vec<u8>> {
    // 在 Android 上，Pipeline Cache 可以保存到应用缓存目录
    // 这里返回 None，表示暂未实现持久化
    // TODO: 实现 cache 持久化到 /data/data/com.termux/cache/vulkan_pipeline_cache.bin
    None
}
