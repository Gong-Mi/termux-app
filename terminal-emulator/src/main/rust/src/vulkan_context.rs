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
    pub pipeline_cache: ash_vk::PipelineCache,
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

        // 尝试使用 1.1，如果失败则回退到 1.0 (增强 Adreno 兼容性)
        let mut instance = None;
        for api_version in [ash_vk::API_VERSION_1_1, ash_vk::API_VERSION_1_0] {
            let app_info = ash_vk::ApplicationInfo { 
                p_application_name: std::ptr::null(),
                application_version: 0,
                p_engine_name: std::ptr::null(),
                engine_version: 0,
                api_version,
                ..Default::default() 
            };
            let create_info = ash_vk::InstanceCreateInfo { 
                p_application_info: &app_info, 
                enabled_extension_count: instance_exts.len() as u32, 
                pp_enabled_extension_names: instance_exts.as_ptr(), 
                ..Default::default() 
            };

            match unsafe { entry.create_instance(&create_info, None) } {
                Ok(inst) => {
                    instance = Some(inst);
                    let ver_str = if api_version == ash_vk::API_VERSION_1_1 { "1.1" } else { "1.0" };
                    android_log(LogPriority::INFO, &format!("VulkanContext::new: Instance created with API {}", ver_str));
                    break;
                }
                Err(e) => {
                    android_log(LogPriority::WARN, &format!("VulkanContext::new: Failed to create instance with API: {:?}", e));
                }
            }
        }

        let instance = if let Some(inst) = instance {
            inst
        } else {
            android_log(LogPriority::ERROR, "VulkanContext::new: All API versions failed");
            return None;
        };

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

        // 尝试启用内存优先级扩展
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

        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        let swapchain_loader = swapchain::Device::new(&instance, &device);

        // 创建 Pipeline Cache
        let pipeline_cache = match load_pipeline_cache() {
            Some(data) => {
                let create_info = ash_vk::PipelineCacheCreateInfo {
                    initial_data_size: data.len(),
                    p_initial_data: data.as_ptr() as *const _,
                    ..Default::default()
                };
                unsafe { device.create_pipeline_cache(&create_info, None) }.unwrap_or_else(|_| {
                    android_log(LogPriority::WARN, "Vulkan: Failed to create pipeline cache from loaded data, creating empty one");
                    unsafe { device.create_pipeline_cache(&ash_vk::PipelineCacheCreateInfo::default(), None).unwrap() }
                })
            }
            None => {
                android_log(LogPriority::INFO, "Vulkan: No pipeline cache found, creating empty one");
                unsafe { device.create_pipeline_cache(&ash_vk::PipelineCacheCreateInfo::default(), None).unwrap() }
            }
        };

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
        let instance_ptr = instance.clone();
        let instance_raw = instance.handle().as_raw();
        let device_raw = device.handle().as_raw();

        let get_proc = move |of: vk::GetProcOf| {
            unsafe {
                match of {
                    vk::GetProcOf::Instance(inst, name) => {
                        let name_cstr = CStr::from_ptr(name);
                        entry_ptr.get_instance_proc_addr(ash_vk::Instance::from_raw(inst as _), name_cstr.as_ptr()).map(|f| f as _).unwrap_or(std::ptr::null())
                    }
                    vk::GetProcOf::Device(dev, name) => {
                        let name_cstr = CStr::from_ptr(name);
                        instance_ptr.get_device_proc_addr(ash_vk::Device::from_raw(dev as _), name_cstr.as_ptr()).map(|f| f as _).unwrap_or(std::ptr::null())
                    }
                }
            }
        };

        let backend_context = unsafe {
            vk::BackendContext::new(
                instance_raw as _,
                pdevice.as_raw() as _,
                device_raw as _,
                (queue.as_raw() as _, queue_family_index as usize),
                &get_proc
            )
        };

        android_log(LogPriority::INFO, "VulkanContext::new: Creating Skia context with optimized options");
        let mut context_options = skia_safe::gpu::ContextOptions::new();
        // 增加缓存的程序数量，减少重新编译
        context_options.runtime_program_cache_size = 256;
        // 减少着色器变体，降低编译开销
        context_options.reduced_shader_variations = true;
        
        let context = skia_safe::gpu::direct_contexts::make_vulkan(&backend_context, Some(&context_options));
        if context.is_none() {
            android_log(LogPriority::ERROR, "VulkanContext::new: Skia make_vulkan failed");
            return None;
        }
        let mut context = context.unwrap();
        
        // 设置更大的资源缓存限制 (512MB)
        context.set_resource_cache_limit(512 * 1024 * 1024);

        android_log(LogPriority::INFO, "VulkanContext::new: Skia context created and optimized");

        let mut ctx = Self {
            instance, device, context, queue, graphics_queue_index: queue_family_index,
            pdevice, surface, surface_loader, swapchain_loader,
            swapchain: ash_vk::SwapchainKHR::null(),
            swapchain_images: vec![],
            extent,
            image_available_semaphore,
            render_finished_semaphore,
            pipeline_cache,
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

            let surface_formats = self.surface_loader.get_physical_device_surface_formats(self.pdevice, self.surface)
                .unwrap_or_default();
            let present_modes = self.surface_loader.get_physical_device_surface_present_modes(self.pdevice, self.surface)
                .unwrap_or_default();

            let present_mode = if present_modes.contains(&ash_vk::PresentModeKHR::MAILBOX) {
                ash_vk::PresentModeKHR::MAILBOX
            } else {
                ash_vk::PresentModeKHR::FIFO
            };

            let format = if surface_formats.is_empty() {
                ash_vk::SurfaceFormatKHR {
                    format: ash_vk::Format::R8G8B8A8_UNORM,
                    color_space: ash_vk::ColorSpaceKHR::SRGB_NONLINEAR,
                }
            } else {
                surface_formats.iter()
                    .find(|f| f.color_space == ash_vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .copied()
                    .unwrap_or(surface_formats[0])
            };

            let caps = self.surface_loader.get_physical_device_surface_capabilities(self.pdevice, self.surface)
                .unwrap_or(ash_vk::SurfaceCapabilitiesKHR {
                    min_image_count: 2,
                    max_image_count: u32::MAX,
                    current_extent: ash_vk::Extent2D { width, height },
                    ..Default::default()
                });

            // Triple buffering with max count validation
            let mut min_image_count = caps.min_image_count.max(3);
            if caps.max_image_count > 0 && min_image_count > caps.max_image_count {
                min_image_count = caps.max_image_count;
            }

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
                android_log(LogPriority::INFO, &format!("Vulkan: Swapchain created with {} images", self.swapchain_images.len()));
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

        let vk_image_info = unsafe {
            skia_safe::gpu::vk::ImageInfo::new(
                image.as_raw() as _,
                skia_safe::gpu::vk::Alloc::default(),
                skia_safe::gpu::vk::ImageTiling::OPTIMAL,
                skia_safe::gpu::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                skia_safe::gpu::vk::Format::R8G8B8A8_UNORM,
                1,
                None,
                None,
                None,
                None,
            )
        };

        let render_target = skia_safe::gpu::backend_render_targets::make_vk(
            (self.extent.width as i32, self.extent.height as i32),
            &vk_image_info,
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

    /// 仅销毁 Surface 和 Swapchain，保留 Device/Instance 以维持后台进程优先级
    pub fn abandon_surface(&mut self) {
        android_log(LogPriority::WARN, "VulkanContext: Abandoning Surface/Swapchain only");
        unsafe {
            let _ = self.device.device_wait_idle();
            if self.swapchain != ash_vk::SwapchainKHR::null() {
                self.swapchain_loader.destroy_swapchain(self.swapchain, None);
                self.swapchain = ash_vk::SwapchainKHR::null();
            }
            self.surface_loader.destroy_surface(self.surface, None);
            self.surface = ash_vk::SurfaceKHR::null();
        }
    }

    /// 为现有的上下文重新关联新 Surface
    pub unsafe fn recreate_surface(&mut self, entry: &Entry, window: *mut std::ffi::c_void) -> bool {
        android_log(LogPriority::INFO, "VulkanContext: Reattaching to new window");
        
        let android_surface_loader = ash::khr::android_surface::Instance::new(entry, &self.instance);
        let surface = unsafe { 
            android_surface_loader.create_android_surface(
                &ash_vk::AndroidSurfaceCreateInfoKHR { window, ..Default::default() }, 
                None
            ) 
        };

        match surface {
            Ok(s) => {
                self.surface = s;
                let caps = unsafe { self.surface_loader.get_physical_device_surface_capabilities(self.pdevice, self.surface).ok() };
                if let Some(c) = caps {
                    self.extent = c.current_extent;
                    self.recreate_swapchain(self.extent.width, self.extent.height)
                } else {
                    false
                }
            }
            Err(e) => {
                android_log(LogPriority::ERROR, &format!("VulkanContext: Failed to recreate android surface: {:?}", e));
                false
            }
        }
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        android_log(LogPriority::WARN, "CHECKPOINT: VulkanContext::drop ENTERED");
        
        unsafe {
            // 1. 第一时间放弃 Skia 上下文。
            // 这会防止 Skia 在 drop 时尝试调用任何 Vulkan 函数。
            android_log(LogPriority::DEBUG, "VulkanContext::drop: Abandoning Skia context...");
            self.context.abandon();
            
            // 2. 强制等待 GPU 彻底空闲。
            // 必须在销毁任何底层句柄前完成。
            android_log(LogPriority::DEBUG, "VulkanContext::drop: Waiting for device idle...");
            let wait_start = std::time::Instant::now();
            match self.device.device_wait_idle() {
                Ok(_) => android_log(LogPriority::INFO, &format!("VulkanContext::drop: device_wait_idle success in {:?}", wait_start.elapsed())),
                Err(e) => android_log(LogPriority::ERROR, &format!("VulkanContext::drop: device_wait_idle FAILED (expected if surface lost): {:?}", e)),
            }

            android_log(LogPriority::DEBUG, "VulkanContext::drop: Cleaning up Vulkan objects...");
            save_pipeline_cache(&self.device, self.pipeline_cache);
            
            self.device.destroy_pipeline_cache(self.pipeline_cache, None);
            self.device.destroy_semaphore(self.image_available_semaphore, None);
            self.device.destroy_semaphore(self.render_finished_semaphore, None);
            
            // 3. 销毁交换链。
            // 在 Adreno 驱动中，重置命令池可能与交换链状态有关。
            if self.swapchain != ash_vk::SwapchainKHR::null() {
                android_log(LogPriority::DEBUG, "VulkanContext::drop: Destroying swapchain");
                self.swapchain_loader.destroy_swapchain(self.swapchain, None);
                self.swapchain = ash_vk::SwapchainKHR::null();
            }
            
            // 4. 销毁 Surface。
            android_log(LogPriority::DEBUG, "VulkanContext::drop: Destroying surface");
            self.surface_loader.destroy_surface(self.surface, None);

            // 5. 最后销毁核心驱动对象。
            // 顺序极其重要：Device -> Instance。
            android_log(LogPriority::WARN, "VulkanContext::drop: Destroying device...");
            self.device.destroy_device(None);
            
            android_log(LogPriority::WARN, "VulkanContext::drop: Destroying instance...");
            self.instance.destroy_instance(None);
        }
        android_log(LogPriority::WARN, "CHECKPOINT: VulkanContext::drop EXITING - Mutex issues should be avoided");
    }
}

fn get_cache_path() -> std::path::PathBuf {
    std::path::PathBuf::from("/data/data/com.termux/files/home/.termux/vulkan_pipeline_cache.bin")
}

fn load_pipeline_cache() -> Option<Vec<u8>> {
    let path = get_cache_path();
    if path.exists() {
        match std::fs::read(&path) {
            Ok(data) => {
                android_log(LogPriority::INFO, &format!("Vulkan: Loaded pipeline cache ({} bytes)", data.len()));
                Some(data)
            }
            Err(e) => {
                android_log(LogPriority::WARN, &format!("Vulkan: Failed to read pipeline cache file: {:?}", e));
                None
            }
        }
    } else {
        None
    }
}

fn save_pipeline_cache(device: &Device, cache: ash_vk::PipelineCache) {
    let path = get_cache_path();
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    match unsafe { device.get_pipeline_cache_data(cache) } {
        Ok(data) => {
            if !data.is_empty() {
                match std::fs::write(&path, data) {
                    Ok(_) => android_log(LogPriority::INFO, "Vulkan: Successfully saved pipeline cache"),
                    Err(e) => android_log(LogPriority::WARN, &format!("Vulkan: Failed to write pipeline cache file: {:?}", e)),
                }
            }
        }
        Err(e) => {
            android_log(LogPriority::WARN, &format!("Vulkan: Failed to get pipeline cache data from device: {:?}", e));
        }
    }
}
