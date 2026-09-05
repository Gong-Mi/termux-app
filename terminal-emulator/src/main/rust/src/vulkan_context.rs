use crate::utils::{LogPriority, android_log};
use ash::khr::swapchain;
use ash::vk::Handle;
use ash::{Device, Entry, Instance, vk as ash_vk};
use skia_safe::{ColorType, Surface as SkSurface, gpu::DirectContext, gpu::vk};
use std::ffi::CStr;

fn skia_max_api_version(created: u32) -> u32 {
    #[cfg(all(feature = "skia-api-experiment", target_os = "android"))]
    {
        // Android PROP_VALUE_MAX is 92, including the terminating NUL.
        unsafe extern "C" {
            fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> i32;
        }
        let mut value = [0 as std::ffi::c_char; 92];
        let length = unsafe {
            __system_property_get(c"debug.termux.skia_api_cap".as_ptr(), value.as_mut_ptr())
        };
        let property = if length > 0 {
            unsafe { CStr::from_ptr(value.as_ptr()) }.to_str().ok()
        } else {
            None
        };
        crate::skia_api_contract::max_api_version(created, true, property)
    }
    #[cfg(not(all(feature = "skia-api-experiment", target_os = "android")))]
    crate::skia_api_contract::max_api_version(created, false, None)
}

pub struct VulkanContext {
    pub entry: Entry,
    // 依赖对象最先声明，以便最先销毁
    pub context: Option<DirectContext>,

    // 渲染资源
    pub pipeline_cache: ash_vk::PipelineCache,
    pub image_available_semaphore: ash_vk::Semaphore,
    pub render_finished_semaphore: ash_vk::Semaphore,
    pub in_flight_fence: ash_vk::Fence,
    pub swapchain: ash_vk::SwapchainKHR,
    pub swapchain_images: Vec<ash_vk::Image>,
    pub surface: ash_vk::SurfaceKHR,

    // 加载器
    pub swapchain_loader: swapchain::Device,
    pub surface_loader: ash::khr::surface::Instance,

    // 核心驱动对象最后声明，以便最后销毁
    pub device: Device,
    pub instance: Instance,

    // 其他状态
    pub pdevice: ash_vk::PhysicalDevice,
    pub graphics_queue_index: u32,
    pub queue: ash_vk::Queue,
    pub extent: ash_vk::Extent2D,
}

unsafe impl Send for VulkanContext {}
unsafe impl Sync for VulkanContext {}

impl VulkanContext {
    pub unsafe fn new(window: *mut std::ffi::c_void) -> Option<Self> {
        android_log(
            LogPriority::INFO,
            "VulkanContext::new: Starting initialization",
        );

        let entry = unsafe { Entry::load().ok() };
        if entry.is_none() {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: Entry::load() failed",
            );
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
            let instance_ext_props =
                unsafe { entry.enumerate_instance_extension_properties(None).ok()? };
            instance_ext_props
                .iter()
                .any(|p| {
                    let name = unsafe { CStr::from_ptr(p.extension_name.as_ptr()) };
                    name == ext_name
                })
                .then_some(ext_name)
        });
        if let Some(debug_ext) = has_debug_utils {
            instance_exts.push(debug_ext.as_ptr());
            android_log(LogPriority::INFO, "Vulkan: VK_EXT_debug_utils enabled");
        }

        // Retain the successful ApplicationInfo version, not the loader/device maximum.
        // Skia m145 requires 1.1; the legacy 1.0 fallback is rejected before Skia.
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
                    instance = Some((inst, api_version));
                    let ver_str = if api_version == ash_vk::API_VERSION_1_1 {
                        "1.1"
                    } else {
                        "1.0"
                    };
                    android_log(
                        LogPriority::INFO,
                        &format!("VulkanContext::new: Instance created with API {}", ver_str),
                    );
                    break;
                }
                Err(e) => {
                    android_log(
                        LogPriority::WARN,
                        &format!(
                            "VulkanContext::new: Failed to create instance with API: {:?}",
                            e
                        ),
                    );
                }
            }
        }

        let (instance, created_api_version) = if let Some(inst) = instance {
            inst
        } else {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: All API versions failed",
            );
            return None;
        };

        let loader_api_version = match unsafe { entry.try_enumerate_instance_version() } {
            Ok(version) => version.unwrap_or(ash_vk::API_VERSION_1_0),
            Err(error) => {
                android_log(LogPriority::ERROR, &format!("SKIA_API_REJECT: loader query failed: {error:?}"));
                unsafe { instance.destroy_instance(None) };
                return None;
            }
        };
        if !crate::skia_api_contract::supported(created_api_version, loader_api_version, ash_vk::API_VERSION_1_1) {
            android_log(LogPriority::ERROR, "SKIA_API_REJECT: instance/loader requires Vulkan 1.1");
            unsafe { instance.destroy_instance(None) };
            return None;
        }

        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let android_surface_loader = ash::khr::android_surface::Instance::new(&entry, &instance);
        let surface = unsafe {
            android_surface_loader.create_android_surface(
                &ash_vk::AndroidSurfaceCreateInfoKHR {
                    window,
                    ..Default::default()
                },
                None,
            )
        };
        if surface.is_err() {
            android_log(
                LogPriority::ERROR,
                &format!(
                    "VulkanContext::new: create_android_surface failed: {:?}",
                    surface.err()
                ),
            );
            return None;
        }
        let surface = surface.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Surface created");

        let pdevices = unsafe { instance.enumerate_physical_devices() };
        if pdevices.is_err() || pdevices.as_ref().unwrap().is_empty() {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: enumerate_physical_devices failed or returned empty list",
            );
            return None;
        }
        let pdevices = pdevices.unwrap();
        android_log(
            LogPriority::INFO,
            &format!(
                "VulkanContext::new: Found {} physical device(s)",
                pdevices.len()
            ),
        );

        // 选择支持图形/Present 队列的设备
        let mut selected_pdevice = None;
        let mut selected_queue_family = 0;

        for (dev_idx, pdev) in pdevices.iter().enumerate() {
            let props = unsafe { instance.get_physical_device_properties(*pdev) };
            let device_name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
            android_log(
                LogPriority::DEBUG,
                &format!(
                    "VulkanContext::new: Device #{} - name='{}', type={:?}",
                    dev_idx,
                    device_name.to_string_lossy(),
                    props.device_type
                ),
            );

            // 查找支持 present 的队列族
            let queue_props =
                unsafe { instance.get_physical_device_queue_family_properties(*pdev) };
            for (q_idx, q_prop) in queue_props.iter().enumerate() {
                let supports_present = unsafe {
                    surface_loader.get_physical_device_surface_support(*pdev, q_idx as u32, surface)
                };
                if supports_present.unwrap_or(false)
                    && q_prop.queue_flags.contains(ash_vk::QueueFlags::GRAPHICS)
                {
                    selected_pdevice = Some(*pdev);
                    selected_queue_family = q_idx as u32;
                    android_log(
                        LogPriority::INFO,
                        &format!(
                            "VulkanContext::new: Selected device #{} (queue_family={})",
                            dev_idx, selected_queue_family
                        ),
                    );
                    break;
                }
            }
            if selected_pdevice.is_some() {
                break;
            }
        }

        let pdevice = match selected_pdevice {
            Some(p) => p,
            None => {
                android_log(
                    LogPriority::ERROR,
                    "VulkanContext::new: No physical device found with GRAPHICS+PRESENT queue family",
                );
                let pdev = pdevices[0];
                let props = unsafe { instance.get_physical_device_properties(pdev) };
                let device_name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
                android_log(
                    LogPriority::WARN,
                    &format!(
                        "VulkanContext::new: Fallback to device #0: '{}'",
                        device_name.to_string_lossy()
                    ),
                );
                pdev
            }
        };

        let physical_api_version = unsafe { instance.get_physical_device_properties(pdevice) }.api_version;
        android_log(LogPriority::INFO, &format!("SKIA_API_VERSIONS: loader={loader_api_version} physical={physical_api_version} created={created_api_version}"));
        if !crate::skia_api_contract::supported(created_api_version, loader_api_version, physical_api_version) {
            android_log(LogPriority::ERROR, "SKIA_API_REJECT: physical device requires Vulkan 1.1");
            unsafe {
                surface_loader.destroy_surface(surface, None);
                instance.destroy_instance(None);
            }
            return None;
        }
        let queue_family_index = selected_queue_family;

        // 设备级扩展
        let mut device_exts = vec![swapchain::NAME.as_ptr()];

        // 尝试启用内存优先级扩展
        let memory_priority_ext = CStr::from_bytes_with_nul(b"VK_KHR_maintenance1\0").ok();
        if let Some(ext_name) = memory_priority_ext {
            let device_ext_props =
                unsafe { instance.enumerate_device_extension_properties(pdevice).ok() }
                    .unwrap_or_default();
            if device_ext_props.iter().any(|p| {
                let name = unsafe { CStr::from_ptr(p.extension_name.as_ptr()) };
                name == ext_name
            }) {
                device_exts.push(ext_name.as_ptr());
                android_log(LogPriority::INFO, "Vulkan: VK_KHR_maintenance1 enabled");
            }
        }

        let queue_info = ash_vk::DeviceQueueCreateInfo {
            queue_family_index,
            queue_count: 1,
            p_queue_priorities: [1.0].as_ptr(),
            ..Default::default()
        };
        let device_create_info = ash_vk::DeviceCreateInfo {
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_info,
            enabled_extension_count: device_exts.len() as u32,
            pp_enabled_extension_names: device_exts.as_ptr(),
            ..Default::default()
        };
        let device = unsafe { instance.create_device(pdevice, &device_create_info, None) };
        if device.is_err() {
            android_log(
                LogPriority::ERROR,
                &format!(
                    "VulkanContext::new: create_device failed: {:?}",
                    device.err()
                ),
            );
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
                android_log(
                    LogPriority::INFO,
                    "Vulkan: No pipeline cache found, creating empty one",
                );
                unsafe {
                    device
                        .create_pipeline_cache(&ash_vk::PipelineCacheCreateInfo::default(), None)
                        .unwrap()
                }
            }
        };

        let caps =
            unsafe { surface_loader.get_physical_device_surface_capabilities(pdevice, surface) };
        if caps.is_err() {
            android_log(
                LogPriority::ERROR,
                &format!(
                    "VulkanContext::new: get_capabilities failed: {:?}",
                    caps.err()
                ),
            );
            return None;
        }
        let caps = caps.unwrap();
        let extent = caps.current_extent;
        android_log(
            LogPriority::INFO,
            &format!(
                "VulkanContext::new: Surface caps {}/{}",
                extent.width, extent.height
            ),
        );

        let semaphore_info = ash_vk::SemaphoreCreateInfo::default();
        let image_available_semaphore = unsafe { device.create_semaphore(&semaphore_info, None) };
        let render_finished_semaphore = unsafe { device.create_semaphore(&semaphore_info, None) };
        if image_available_semaphore.is_err() || render_finished_semaphore.is_err() {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: create_semaphore failed",
            );
            return None;
        }
        let image_available_semaphore = image_available_semaphore.unwrap();
        let render_finished_semaphore = render_finished_semaphore.unwrap();

        let fence_info = ash_vk::FenceCreateInfo {
            flags: ash_vk::FenceCreateFlags::SIGNALED,
            ..Default::default()
        };
        let in_flight_fence = unsafe { device.create_fence(&fence_info, None) };
        if in_flight_fence.is_err() {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: create_fence failed",
            );
            return None;
        }
        let in_flight_fence = in_flight_fence.unwrap();

        let entry_ptr = entry.clone();
        let instance_ptr = instance.clone();
        let instance_raw = instance.handle().as_raw();
        let device_raw = device.handle().as_raw();

        let get_proc = move |of: vk::GetProcOf| unsafe {
            let (scope, name, proc) = match of {
                vk::GetProcOf::Instance(inst, name) => (
                    "instance", name,
                    entry_ptr.get_instance_proc_addr(ash_vk::Instance::from_raw(inst as _), name),
                ),
                vk::GetProcOf::Device(dev, name) => (
                    "device", name,
                    instance_ptr.get_device_proc_addr(ash_vk::Device::from_raw(dev as _), name),
                ),
            };
            if proc.is_none() {
                android_log(LogPriority::WARN, &format!(
                    "SKIA_NULL_PROC: scope={scope} name={}", CStr::from_ptr(name).to_string_lossy()
                ));
            }
            proc.map(|f| f as _).unwrap_or(std::ptr::null())
        };

        let mut backend_context = unsafe {
            vk::BackendContext::new(
                instance_raw as _,
                pdevice.as_raw() as _,
                device_raw as _,
                (queue.as_raw() as _, queue_family_index as usize),
                &get_proc,
            )
        };

        let max_api_version = skia_max_api_version(created_api_version);
        backend_context.set_max_api_version(max_api_version);
        android_log(LogPriority::INFO, &format!("SKIA_API_CONTRACT: created={created_api_version} max={max_api_version}"));
        android_log(
            LogPriority::INFO,
            "VulkanContext::new: Creating Skia context with optimized options",
        );
        let mut context_options = skia_safe::gpu::ContextOptions::new();

        // 性能优化：扩大内存中的管线缓存，减少 Android 上的着色器编译卡顿
        context_options.runtime_program_cache_size = 512;
        context_options.reduced_shader_variations = true;

        let context =
            skia_safe::gpu::direct_contexts::make_vulkan(&backend_context, Some(&context_options));
        if context.is_none() {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: Skia make_vulkan failed",
            );
            return None;
        }
        let mut context = context.unwrap();

        // 设置更大的资源缓存限制 (512MB) 以提高多字体/大数据量下的渲染稳定性
        context.set_resource_cache_limit(512 * 1024 * 1024);

        android_log(
            LogPriority::INFO,
            "VulkanContext::new: Skia context created and optimized",
        );

        let mut ctx = Self {
            entry,
            context: Some(context),
            pipeline_cache,
            image_available_semaphore,
            render_finished_semaphore,
            in_flight_fence,
            swapchain: ash_vk::SwapchainKHR::null(),
            swapchain_images: vec![],
            surface,
            swapchain_loader,
            surface_loader,
            device,
            instance,
            pdevice,
            graphics_queue_index: queue_family_index,
            queue,
            extent,
        };

        // The probe runs on this real Vulkan DirectContext, before any swapchain drawing.
        // Build feature, not debug_assertions: debug APKs also use cargo --release.
        #[cfg(feature = "skia-api-experiment")]
        if let Err(reason) = crate::skia_backend_probe::draw_and_readback(ctx.context.as_mut().unwrap()) {
            android_log(LogPriority::ERROR, &format!("SKIA_BACKEND_READBACK: FAIL {reason}"));
            return None;
        }
        #[cfg(feature = "skia-api-experiment")]
        android_log(LogPriority::INFO, "SKIA_BACKEND_READBACK: PASS");

        let swapchain_ok = ctx.recreate_swapchain(extent.width, extent.height);
        if !swapchain_ok {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: recreate_swapchain failed",
            );
            return None;
        }
        android_log(LogPriority::INFO, "VulkanContext::new: SUCCESS");
        Some(ctx)
    }

    pub fn recreate_swapchain(&mut self, width: u32, height: u32) -> bool {
        unsafe {
            let surface_formats = self
                .surface_loader
                .get_physical_device_surface_formats(self.pdevice, self.surface)
                .unwrap_or_default();
            let present_modes = self
                .surface_loader
                .get_physical_device_surface_present_modes(self.pdevice, self.surface)
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
                surface_formats
                    .iter()
                    .find(|f| f.color_space == ash_vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .copied()
                    .unwrap_or(surface_formats[0])
            };

            let caps = self
                .surface_loader
                .get_physical_device_surface_capabilities(self.pdevice, self.surface)
                .unwrap_or(ash_vk::SurfaceCapabilitiesKHR {
                    min_image_count: 2,
                    max_image_count: u32::MAX,
                    current_extent: ash_vk::Extent2D { width, height },
                    min_image_extent: ash_vk::Extent2D {
                        width: 1,
                        height: 1,
                    },
                    max_image_extent: ash_vk::Extent2D { width, height },
                    ..Default::default()
                });

            // 关键修复：根据 Vulkan 规范，如果 currentExtent 是具体值（非 0xFFFFFFFF），
            // 则 swapchain 的 imageExtent 必须与之匹配，否则会导致画布大小与显示大小不一致。
            let final_width = if caps.current_extent.width != u32::MAX {
                caps.current_extent.width
            } else {
                width.clamp(caps.min_image_extent.width, caps.max_image_extent.width)
            };
            let final_height = if caps.current_extent.height != u32::MAX {
                caps.current_extent.height
            } else {
                height.clamp(caps.min_image_extent.height, caps.max_image_extent.height)
            };

            self.extent = ash_vk::Extent2D {
                width: final_width,
                height: final_height,
            };

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

            if let Ok(new_swapchain) = self
                .swapchain_loader
                .create_swapchain(&swapchain_create_info, None)
            {
                if self.swapchain != ash_vk::SwapchainKHR::null() {
                    self.swapchain_loader
                        .destroy_swapchain(self.swapchain, None);
                }
                self.swapchain = new_swapchain;
                self.swapchain_images = self
                    .swapchain_loader
                    .get_swapchain_images(self.swapchain)
                    .unwrap_or_default();
                android_log(
                    LogPriority::INFO,
                    &format!(
                        "Vulkan: Swapchain created {}x{} with {} images",
                        final_width,
                        final_height,
                        self.swapchain_images.len()
                    ),
                );
                true
            } else {
                android_log(
                    LogPriority::ERROR,
                    &format!(
                        "Vulkan: create_swapchain FAILED for {}x{}",
                        final_width, final_height
                    ),
                );
                false
            }
        }
    }

    pub fn acquire_next_image(&mut self) -> Option<u32> {
        if self.swapchain == ash_vk::SwapchainKHR::null() {
            return None;
        }

        unsafe {
            // 等待上一帧完成，防止覆盖正在渲染的帧
            let _ = self
                .device
                .wait_for_fences(&[self.in_flight_fence], true, 1_000_000_000);
            let _ = self.device.reset_fences(&[self.in_flight_fence]);

            // 严禁使用 u64::MAX。在系统进入后台或 Surface 失效时，
            // MAX 会导致线程永久挂起。改为 100ms 超时。
            match self.swapchain_loader.acquire_next_image(
                self.swapchain,
                100_000_000, // 100ms (单位纳秒)
                self.image_available_semaphore,
                ash_vk::Fence::null(),
            ) {
                Ok((idx, _)) => Some(idx),
                Err(e) => {
                    // 如果是因为超时或 Surface 丢失导致的失败，返回 None
                    if e != ash_vk::Result::NOT_READY && e != ash_vk::Result::TIMEOUT {
                        android_log(
                            LogPriority::WARN,
                            &format!("Vulkan: acquire_next_image critical error: {:?}", e),
                        );
                    }
                    None
                }
            }
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
            self.context.as_mut().expect("Skia context missing"),
            &render_target,
            skia_safe::gpu::SurfaceOrigin::TopLeft,
            ColorType::RGBA8888,
            None,
            None,
        )
    }

    /// 仅销毁 Surface 和 Swapchain，保留 Device/Instance 以维持后台进程优先级
    pub fn abandon_surface(&mut self) {
        android_log(
            LogPriority::WARN,
            "VulkanContext: Abandoning Surface/Swapchain only",
        );
        if let Some(ctx) = self.context.as_mut() {
            ctx.flush_and_submit();
        }

        unsafe {
            let _ = self.device.device_wait_idle();
            if self.swapchain != ash_vk::SwapchainKHR::null() {
                self.swapchain_loader
                    .destroy_swapchain(self.swapchain, None);
                self.swapchain = ash_vk::SwapchainKHR::null();
            }
            self.swapchain_images.clear();
            self.surface_loader.destroy_surface(self.surface, None);
            self.surface = ash_vk::SurfaceKHR::null();
        }
    }

    /// 为现有的上下文重新关联新 Surface
    pub unsafe fn recreate_surface(&mut self, window: *mut std::ffi::c_void) -> bool {
        android_log(
            LogPriority::INFO,
            "VulkanContext: Reattaching to new window",
        );

        // 1. 彻底清理旧的 Surface 资源
        self.abandon_surface();

        // 2. 创建新 Surface
        let android_surface_loader =
            ash::khr::android_surface::Instance::new(&self.entry, &self.instance);
        let surface_result = unsafe {
            android_surface_loader.create_android_surface(
                &ash_vk::AndroidSurfaceCreateInfoKHR {
                    window,
                    ..Default::default()
                },
                None,
            )
        };

        match surface_result {
            Ok(s) => {
                self.surface = s;
                let caps = unsafe {
                    self.surface_loader
                        .get_physical_device_surface_capabilities(self.pdevice, self.surface)
                        .ok()
                };
                if let Some(c) = caps {
                    self.extent = c.current_extent;
                    android_log(
                        LogPriority::INFO,
                        &format!(
                            "VulkanContext: Re-associated surface size {}x{}",
                            self.extent.width, self.extent.height
                        ),
                    );
                    self.recreate_swapchain(self.extent.width, self.extent.height)
                } else {
                    android_log(
                        LogPriority::ERROR,
                        "VulkanContext: Failed to get new surface capabilities",
                    );
                    false
                }
            }
            Err(e) => {
                android_log(
                    LogPriority::ERROR,
                    &format!("VulkanContext: Failed to recreate android surface: {:?}", e),
                );
                false
            }
        }
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        android_log(LogPriority::WARN, "CHECKPOINT: VulkanContext::drop ENTERED");

        unsafe {
            // 1. 第一时间放弃并销毁 Skia 上下文。
            // 这会释放 Skia 所有的资源，并确保它不再引用 Vulkan 设备。
            if let Some(mut sk_ctx) = self.context.take() {
                android_log(
                    LogPriority::DEBUG,
                    "VulkanContext::drop: Abandoning Skia context...",
                );
                sk_ctx.abandon();
                drop(sk_ctx); // 显式显式释放
            }

            // 2. 强制等待 GPU 彻底空闲。
            // 必须在销毁任何底层句柄前完成。
            android_log(
                LogPriority::DEBUG,
                "VulkanContext::drop: Waiting for device idle...",
            );
            let wait_start = std::time::Instant::now();
            match self.device.device_wait_idle() {
                Ok(_) => android_log(
                    LogPriority::INFO,
                    &format!(
                        "VulkanContext::drop: device_wait_idle success in {:?}",
                        wait_start.elapsed()
                    ),
                ),
                Err(e) => android_log(
                    LogPriority::ERROR,
                    &format!("VulkanContext::drop: device_wait_idle FAILED: {:?}", e),
                ),
            }

            android_log(
                LogPriority::DEBUG,
                "VulkanContext::drop: Cleaning up Vulkan objects...",
            );
            save_pipeline_cache(&self.device, self.pipeline_cache);

            self.device
                .destroy_pipeline_cache(self.pipeline_cache, None);
            self.device
                .destroy_semaphore(self.image_available_semaphore, None);
            self.device
                .destroy_semaphore(self.render_finished_semaphore, None);
            self.device.destroy_fence(self.in_flight_fence, None);

            // 3. 销毁交换链。
            if self.swapchain != ash_vk::SwapchainKHR::null() {
                android_log(
                    LogPriority::DEBUG,
                    "VulkanContext::drop: Destroying swapchain",
                );
                self.swapchain_loader
                    .destroy_swapchain(self.swapchain, None);
                self.swapchain = ash_vk::SwapchainKHR::null();
            }

            // 4. 销毁 Surface。
            if self.surface != ash_vk::SurfaceKHR::null() {
                android_log(
                    LogPriority::DEBUG,
                    "VulkanContext::drop: Destroying surface",
                );
                self.surface_loader.destroy_surface(self.surface, None);
                self.surface = ash_vk::SurfaceKHR::null();
            }

            // 5. 最后销毁核心驱动对象。
            // 顺序极其重要：Device -> Instance。
            android_log(
                LogPriority::WARN,
                "VulkanContext::drop: Destroying device...",
            );
            self.device.destroy_device(None);

            android_log(
                LogPriority::WARN,
                "VulkanContext::drop: Destroying instance...",
            );
            self.instance.destroy_instance(None);
        }
        android_log(LogPriority::WARN, "CHECKPOINT: VulkanContext::drop EXITING");
    }
}

fn get_cache_path() -> std::path::PathBuf {
    std::path::PathBuf::from("/data/data/com.termux/cache/vulkan_pipeline_cache.bin")
}

fn load_pipeline_cache() -> Option<Vec<u8>> {
    let path = get_cache_path();
    if path.exists() {
        match std::fs::read(&path) {
            Ok(data) => {
                android_log(
                    LogPriority::INFO,
                    &format!("Vulkan: Loaded pipeline cache ({} bytes)", data.len()),
                );
                Some(data)
            }
            Err(e) => {
                android_log(
                    LogPriority::WARN,
                    &format!("Vulkan: Failed to read pipeline cache file: {:?}", e),
                );
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
                    Ok(_) => android_log(
                        LogPriority::INFO,
                        "Vulkan: Successfully saved pipeline cache",
                    ),
                    Err(e) => android_log(
                        LogPriority::WARN,
                        &format!("Vulkan: Failed to write pipeline cache file: {:?}", e),
                    ),
                }
            }
        }
        Err(e) => {
            android_log(
                LogPriority::WARN,
                &format!(
                    "Vulkan: Failed to get pipeline cache data from device: {:?}",
                    e
                ),
            );
        }
    }
}
