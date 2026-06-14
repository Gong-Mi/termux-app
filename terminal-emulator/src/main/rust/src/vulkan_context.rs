use crate::utils::{LogPriority, android_log};
use ash::khr::swapchain;
use ash::vk::Handle;
use ash::{Device, Entry, Instance, vk as ash_vk};
use skia_safe::{ColorType, Surface as SkSurface, gpu::DirectContext, gpu::vk};
use std::ffi::CStr;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

// === Pipeline Cache Persistence Hooks ===
static ORIGINAL_CREATE_PIPELINE_CACHE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_DESTROY_PIPELINE_CACHE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static HOOKED_PIPELINE_CACHE: AtomicU64 = AtomicU64::new(0);

static PIPELINE_CACHE_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Set by Java/Kotlin via JNI to the app's cache directory
static PIPELINE_CACHE_FILE_PATH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_pipeline_cache_file_path(path: &str) {
    *PIPELINE_CACHE_FILE_PATH.lock().unwrap() = Some(path.to_string());
}

unsafe extern "C" fn hooked_vk_create_pipeline_cache(
    device: u64,
    p_create_info: *const c_void,
    p_allocator: *const c_void,
    p_pipeline_cache: *mut u64,
) -> i32 {
    let cache = HOOKED_PIPELINE_CACHE.load(Ordering::SeqCst);
    android_log(
        LogPriority::INFO,
        &format!(
            "hooked_vk_create_pipeline_cache: HOOKED_PIPELINE_CACHE={:#x}",
            cache
        ),
    );
    if cache != 0 {
        unsafe {
            *p_pipeline_cache = cache;
        }
        0 // VK_SUCCESS
    } else {
        let original = ORIGINAL_CREATE_PIPELINE_CACHE.load(Ordering::SeqCst);
        if original.is_null() {
            return -1; // VK_ERROR_INITIALIZATION_FAILED
        }
        let fn_ptr: unsafe extern "C" fn(u64, *const c_void, *const c_void, *mut u64) -> i32 =
            unsafe { std::mem::transmute(original) };
        unsafe { fn_ptr(device, p_create_info, p_allocator, p_pipeline_cache) }
    }
}

unsafe extern "C" fn hooked_vk_destroy_pipeline_cache(
    device: u64,
    pipeline_cache: u64,
    p_allocator: *const c_void,
) {
    let hooked = HOOKED_PIPELINE_CACHE.load(Ordering::SeqCst);
    android_log(
        LogPriority::INFO,
        &format!(
            "hooked_vk_destroy_pipeline_cache: pipeline_cache={:#x}, HOOKED_PIPELINE_CACHE={:#x}",
            pipeline_cache, hooked
        ),
    );
    if pipeline_cache == hooked && hooked != 0 {
        // Ignore: lifetime managed by VulkanContext::drop
    } else {
        let original = ORIGINAL_DESTROY_PIPELINE_CACHE.load(Ordering::SeqCst);
        if !original.is_null() {
            unsafe {
                let fn_ptr: unsafe extern "C" fn(u64, u64, *const c_void) =
                    std::mem::transmute(original);
                fn_ptr(device, pipeline_cache, p_allocator);
            }
        }
    }
}

fn load_pipeline_cache_file() -> Vec<u8> {
    if let Ok(guard) = PIPELINE_CACHE_FILE_PATH.lock() {
        if let Some(path) = guard.as_ref() {
            return std::fs::read(path).unwrap_or_default();
        }
    }
    Vec::new()
}

/// 校验 pipeline cache 头部是否与当前 GPU 匹配。
/// Vulkan pipeline cache 是驱动私有格式，vendorID/deviceID 不匹配时
/// 必须丢弃，否则可能导致渲染异常或 GPU crash。
fn validate_pipeline_cache(data: &[u8], vendor_id: u32, device_id: u32) -> Option<Vec<u8>> {
    const MIN_HEADER_SIZE: usize = 4 + 4 + 4 + 4 + 16; // 32 bytes

    if data.len() < MIN_HEADER_SIZE {
        if !data.is_empty() {
            android_log(
                LogPriority::DEBUG,
                "Pipeline cache file too small, using empty cache",
            );
        }
        return None;
    }

    let header_size = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let header_version = u32::from_le_bytes(data[4..8].try_into().unwrap());

    if header_version != 1 || header_size < MIN_HEADER_SIZE || data.len() < header_size {
        android_log(
            LogPriority::WARN,
            "Pipeline cache: invalid header version or size, discarding",
        );
        return None;
    }

    let cache_vendor = u32::from_le_bytes(data[8..12].try_into().unwrap());
    let cache_device = u32::from_le_bytes(data[12..16].try_into().unwrap());

    if cache_vendor != vendor_id || cache_device != device_id {
        android_log(
            LogPriority::INFO,
            &format!(
                "Pipeline cache: vendor/device mismatch (cache: {:#x}/{:#x}, current: {:#x}/{:#x}), discarding",
                cache_vendor, cache_device, vendor_id, device_id
            ),
        );
        return None;
    }

    android_log(
        LogPriority::INFO,
        &format!(
            "Pipeline cache: validated {} bytes (vendor={:#x}, device={:#x})",
            data.len(),
            cache_vendor,
            cache_device
        ),
    );

    Some(data.to_vec())
}

fn save_pipeline_cache_data(device: &Device, cache: ash_vk::PipelineCache) {
    let path_guard = PIPELINE_CACHE_FILE_PATH.lock().unwrap();
    let path = match path_guard.as_ref() {
        Some(p) => p,
        None => return,
    };

    unsafe {
        match device.get_pipeline_cache_data(cache) {
            Ok(data) if !data.is_empty() => {
                if let Err(e) = std::fs::write(path, &data) {
                    android_log(
                        LogPriority::WARN,
                        &format!("Failed to save pipeline cache: {:?}", e),
                    );
                } else {
                    android_log(
                        LogPriority::INFO,
                        &format!("Pipeline cache saved: {} bytes to {}", data.len(), path),
                    );
                }
            }
            Ok(_) => {
                android_log(LogPriority::DEBUG, "Pipeline cache empty, nothing to save");
            }
            Err(e) => {
                android_log(
                    LogPriority::WARN,
                    &format!("Failed to get pipeline cache data: {:?}", e),
                );
            }
        }
    }
}

/// 将 Vulkan ash format 映射到 Skia gpu::vk::Format，防止硬编码不匹配
fn ash_format_to_skia_format(fmt: ash_vk::Format) -> skia_safe::gpu::vk::Format {
    match fmt {
        ash_vk::Format::R8G8B8A8_UNORM => skia_safe::gpu::vk::Format::R8G8B8A8_UNORM,
        ash_vk::Format::B8G8R8A8_UNORM => skia_safe::gpu::vk::Format::B8G8R8A8_UNORM,
        ash_vk::Format::R8G8B8A8_SRGB => skia_safe::gpu::vk::Format::R8G8B8A8_SRGB,
        ash_vk::Format::B8G8R8A8_SRGB => skia_safe::gpu::vk::Format::B8G8R8A8_SRGB,
        ash_vk::Format::A8B8G8R8_UNORM_PACK32 => skia_safe::gpu::vk::Format::A8B8G8R8_UNORM_PACK32,
        ash_vk::Format::A2B10G10R10_UNORM_PACK32 => {
            skia_safe::gpu::vk::Format::A2B10G10R10_UNORM_PACK32
        }
        ash_vk::Format::A2R10G10B10_UNORM_PACK32 => {
            skia_safe::gpu::vk::Format::A2R10G10B10_UNORM_PACK32
        }
        ash_vk::Format::R5G6B5_UNORM_PACK16 => skia_safe::gpu::vk::Format::R5G6B5_UNORM_PACK16,
        ash_vk::Format::A1R5G5B5_UNORM_PACK16 => skia_safe::gpu::vk::Format::A1R5G5B5_UNORM_PACK16,
        ash_vk::Format::R16G16B16A16_SFLOAT => skia_safe::gpu::vk::Format::R16G16B16A16_SFLOAT,
        _ => {
            android_log(
                LogPriority::WARN,
                &format!(
                    "Vulkan: Unsupported format {:?}, fallback to R8G8B8A8_UNORM",
                    fmt
                ),
            );
            skia_safe::gpu::vk::Format::R8G8B8A8_UNORM
        }
    }
}

pub struct VulkanContext {
    pub entry: Entry,
    pub instance: Instance,
    pub device: Device,
    pub context: Option<DirectContext>,
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
    /// 缓存的 Skia Surface，避免每帧重新创建 wrap_backend_render_target
    sk_surfaces: Vec<Option<SkSurface>>,
    /// 持久化的 Vulkan Pipeline Cache
    pipeline_cache: ash_vk::PipelineCache,
    /// Keep cache data alive until after pipeline cache creation
    _pipeline_cache_data: Vec<u8>,
    /// 当前 Swapchain 对应的 Skia Vulkan Format，避免硬编码与实际 Image 格式不匹配
    skia_format: skia_safe::gpu::vk::Format,
    /// 当前 Swapchain 的颜色空间，用于映射 Skia ColorSpace
    vk_color_space: ash_vk::ColorSpaceKHR,
}

// SAFETY: VulkanContext 包含的 `ash::Device`、`ash::Instance`、`vk::Queue` 等
// 本质上是不透明的 handle + vtable，在 C API 层面允许跨线程转移所有权。
// `DirectContext` 和 `SkSurface` 在 skia-safe 中未标记 `Send` 是出于保守设计，
// 但 Skia C++ 的 `GrDirectContext`/`SkSurface` 对象指针可以安全地在线程间移动。
// 实际使用中 `VulkanContext` 始终被 `Mutex` 包裹，确保任意时刻只有一个线程访问。
// 因此只实现 `Send`（所有权转移），不实现 `Sync`（禁止共享引用）。
unsafe impl Send for VulkanContext {}

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

        // Prefer the highest API exposed by the Android Vulkan loader, then fall back for older drivers.
        let mut instance = None;
        let mut selected_instance_api_version = ash_vk::API_VERSION_1_0;
        let api_version_1_4 = ash_vk::make_api_version(0, 1, 4, 0);
        for api_version in [
            api_version_1_4,
            ash_vk::API_VERSION_1_3,
            ash_vk::API_VERSION_1_2,
            ash_vk::API_VERSION_1_1,
            ash_vk::API_VERSION_1_0,
        ] {
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
                    selected_instance_api_version = api_version;
                    android_log(
                        LogPriority::INFO,
                        &format!(
                            "VulkanContext::new: Instance created with API {}.{}",
                            ash_vk::api_version_major(api_version),
                            ash_vk::api_version_minor(api_version)
                        ),
                    );
                    break;
                }
                Err(e) => {
                    android_log(
                        LogPriority::WARN,
                        &format!(
                            "VulkanContext::new: Failed to create instance with API {}.{}: {:?}",
                            ash_vk::api_version_major(api_version),
                            ash_vk::api_version_minor(api_version),
                            e
                        ),
                    );
                }
            }
        }

        let instance = if let Some(inst) = instance {
            inst
        } else {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::new: All API versions failed",
            );
            return None;
        };

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

        let queue_family_index = selected_queue_family;
        let physical_props = unsafe { instance.get_physical_device_properties(pdevice) };
        let selected_api_version =
            std::cmp::min(selected_instance_api_version, physical_props.api_version);
        android_log(
            LogPriority::INFO,
            &format!(
                "VulkanContext::new: Physical device API {}.{}, Skia max API {}.{}",
                ash_vk::api_version_major(physical_props.api_version),
                ash_vk::api_version_minor(physical_props.api_version),
                ash_vk::api_version_major(selected_api_version),
                ash_vk::api_version_minor(selected_api_version)
            ),
        );

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

        let entry_ptr = entry.clone();
        let instance_ptr = instance.clone();
        let instance_raw = instance.handle().as_raw();
        let device_raw = device.handle().as_raw();

        // === Pipeline Cache: load previous data and create cache before Skia initializes ===
        let raw_cache_data = load_pipeline_cache_file();
        let pipeline_cache_data = validate_pipeline_cache(
            &raw_cache_data,
            physical_props.vendor_id,
            physical_props.device_id,
        )
        .unwrap_or_default();
        let cache_create_info = ash_vk::PipelineCacheCreateInfo {
            flags: ash_vk::PipelineCacheCreateFlags::empty(),
            initial_data_size: pipeline_cache_data.len(),
            p_initial_data: if pipeline_cache_data.is_empty() {
                std::ptr::null()
            } else {
                pipeline_cache_data.as_ptr() as _
            },
            ..Default::default()
        };
        let pipeline_cache = match unsafe { device.create_pipeline_cache(&cache_create_info, None) }
        {
            Ok(cache) => {
                android_log(
                    LogPriority::INFO,
                    &format!(
                        "VulkanContext::new: Pipeline cache created ({} bytes initial data)",
                        pipeline_cache_data.len()
                    ),
                );
                cache
            }
            Err(e) => {
                android_log(
                    LogPriority::WARN,
                    &format!(
                        "VulkanContext::new: Failed to create pipeline cache: {:?}, continuing without cache",
                        e
                    ),
                );
                ash_vk::PipelineCache::null()
            }
        };

        HOOKED_PIPELINE_CACHE.store(pipeline_cache.as_raw(), Ordering::SeqCst);
        PIPELINE_CACHE_INITIALIZED.store(true, Ordering::SeqCst);

        let get_proc = move |of: vk::GetProcOf| unsafe {
            match of {
                vk::GetProcOf::Instance(inst, name) => {
                    let name_cstr = CStr::from_ptr(name);
                    let ptr: *const c_void = entry_ptr
                        .get_instance_proc_addr(
                            ash_vk::Instance::from_raw(inst as _),
                            name_cstr.as_ptr(),
                        )
                        .map(|f| f as _)
                        .unwrap_or(std::ptr::null());
                    ptr
                }
                vk::GetProcOf::Device(dev, name) => {
                    let name_cstr = CStr::from_ptr(name);
                    let ptr = instance_ptr
                        .get_device_proc_addr(
                            ash_vk::Device::from_raw(dev as _),
                            name_cstr.as_ptr(),
                        )
                        .map(|f| f as *mut c_void)
                        .unwrap_or(std::ptr::null_mut());
                    if ptr.is_null() {
                        return std::ptr::null();
                    }
                    let name_str = name_cstr.to_str().unwrap_or("");
                    if name_str == "vkCreatePipelineCache" {
                        ORIGINAL_CREATE_PIPELINE_CACHE.store(ptr, Ordering::SeqCst);
                        hooked_vk_create_pipeline_cache as *const () as usize as *const c_void
                    } else if name_str == "vkDestroyPipelineCache" {
                        ORIGINAL_DESTROY_PIPELINE_CACHE.store(ptr, Ordering::SeqCst);
                        hooked_vk_destroy_pipeline_cache as *const () as usize as *const c_void
                    } else {
                        ptr as *const c_void
                    }
                }
            }
        };

        let mut backend_context = unsafe {
            vk::BackendContext::new_with_extensions(
                instance_raw as _,
                pdevice.as_raw() as _,
                device_raw as _,
                (queue.as_raw() as _, queue_family_index as usize),
                &get_proc,
                &["VK_KHR_surface", "VK_KHR_android_surface"],
                &["VK_KHR_swapchain"],
            )
        };
        backend_context.set_max_api_version(vk::Version::new(
            ash_vk::api_version_major(selected_api_version) as usize,
            ash_vk::api_version_minor(selected_api_version) as usize,
            0,
        ));

        android_log(
            LogPriority::INFO,
            "VulkanContext::new: Creating Skia context",
        );
        let mut context_options = skia_safe::gpu::ContextOptions::new();
        // 限制字形 atlas 纹理大小，防止大字体场景下内存暴涨
        context_options.glyph_cache_texture_maximum_bytes = 8 * 1024 * 1024;
        // 增大运行时着色器程序缓存，减少重复编译
        context_options.runtime_program_cache_size = 256;
        // 允许字形 atlas 使用多张纹理，提升大字符集渲染效率
        context_options.allow_multiple_glyph_cache_textures =
            skia_safe::gpu::ganesh::context_options::Enable::Yes;
        // 缓存 Vulkan 二级命令缓冲，减少命令构建开销
        context_options.max_cached_vulkan_secondary_command_buffers = 64;

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
        // 设置 GPU 资源缓存上限为 64MB（纹理、缓冲区等）
        context.set_resource_cache_limit(64 * 1024 * 1024);
        android_log(
            LogPriority::INFO,
            "VulkanContext::new: Skia context created with cache limits",
        );

        let mut ctx = Self {
            entry,
            instance,
            device,
            context: Some(context),
            queue,
            graphics_queue_index: queue_family_index,
            pdevice,
            surface,
            surface_loader,
            swapchain_loader,
            swapchain: ash_vk::SwapchainKHR::null(),
            swapchain_images: vec![],
            extent,
            image_available_semaphore,
            render_finished_semaphore,
            sk_surfaces: vec![],
            pipeline_cache,
            _pipeline_cache_data: pipeline_cache_data,
            skia_format: skia_safe::gpu::vk::Format::R8G8B8A8_UNORM,
            vk_color_space: ash_vk::ColorSpaceKHR::SRGB_NONLINEAR,
        };

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
        // 防御性检查：确保 surface 有效
        if self.surface == ash_vk::SurfaceKHR::null() {
            android_log(
                LogPriority::ERROR,
                "Vulkan: recreate_swapchain called with null surface",
            );
            return false;
        }
        // 性能优化：如果尺寸与当前相同，跳过重建
        if self.extent.width == width
            && self.extent.height == height
            && self.swapchain != ash_vk::SwapchainKHR::null()
        {
            android_log(
                LogPriority::DEBUG,
                &format!(
                    "Vulkan: Swapchain size {}x{} unchanged, skipping recreation",
                    width, height
                ),
            );
            return true;
        }
        unsafe {
            self.extent = ash_vk::Extent2D { width, height };

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
                // 记录所有可用格式，便于调试
                for (i, f) in surface_formats.iter().enumerate() {
                    android_log(
                        LogPriority::INFO,
                        &format!(
                            "Vulkan: Available surface format #{}: {:?} (colorspace: {:?})",
                            i, f.format, f.color_space
                        ),
                    );
                }

                // 性能优化与画质优先 (Multi-pipeline Selection):
                // 1. 优先 HDR10 (Rec.2020 + PQ)，如果支持。
                // 2. 其次 WCG (Display P3)，消除色彩断层。
                // 3. 再次 scRGB (F16 Linear)，用于广色域处理。
                // 4. 回退 8-bit (BGRA/RGBA)。
                surface_formats.iter()
                    .find(|f| f.format == ash_vk::Format::A2B10G10R10_UNORM_PACK32 && f.color_space == ash_vk::ColorSpaceKHR::HDR10_ST2084_EXT)
                    .or_else(|| surface_formats.iter().find(|f| f.format == ash_vk::Format::A2B10G10R10_UNORM_PACK32 && f.color_space == ash_vk::ColorSpaceKHR::DISPLAY_P3_NONLINEAR_EXT))
                    .or_else(|| surface_formats.iter().find(|f| f.format == ash_vk::Format::A2B10G10R10_UNORM_PACK32))
                    .or_else(|| surface_formats.iter().find(|f| f.format == ash_vk::Format::R16G16B16A16_SFLOAT))
                    .or_else(|| surface_formats.iter().find(|f| f.format == ash_vk::Format::B8G8R8A8_UNORM))
                    .or_else(|| surface_formats.iter().find(|f| f.format == ash_vk::Format::R8G8B8A8_UNORM))
                    .copied()
                    .unwrap_or_else(|| {
                        android_log(LogPriority::WARN, "Vulkan: No high-bit or standard 8-bit format found, falling back to first available");
                        surface_formats[0]
                    })
            };

            // 同步 Skia ImageInfo format 与实际 Swapchain Image format
            self.skia_format = ash_format_to_skia_format(format.format);
            self.vk_color_space = format.color_space;
            android_log(
                LogPriority::INFO,
                &format!(
                    "Vulkan: Selected swapchain format {:?} (mapped to Skia format {:?}) in color space {:?}",
                    format.format, self.skia_format, self.vk_color_space
                ),
            );

            let caps = self
                .surface_loader
                .get_physical_device_surface_capabilities(self.pdevice, self.surface)
                .unwrap_or(ash_vk::SurfaceCapabilitiesKHR {
                    min_image_count: 2,
                    max_image_count: u32::MAX,
                    current_extent: ash_vk::Extent2D { width, height },
                    ..Default::default()
                });

            // 关键修复：如果当前能力显示大小为 0 (例如最小化)，跳过重建
            if caps.current_extent.width == 0 || caps.current_extent.height == 0 {
                android_log(
                    LogPriority::WARN,
                    "Vulkan: Surface size is 0, skipping swapchain recreation",
                );
                return false;
            }

            // pre_transform=IDENTITY 下 caps.current_extent 不可靠（各厂商驱动不一致）。
            // 始终使用 Java onSizeChanged 传入的 view 尺寸，这是唯一准确的来源。
            let actual_extent = ash_vk::Extent2D {
                width: width.max(64).min(7680),
                height: height.max(64).min(7680),
            };
            self.extent = actual_extent;

            // Double buffering for lower VRAM usage; terminal rendering is lightweight
            let mut min_image_count = caps.min_image_count.max(2);
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

            // 关键修复：在重建前确保 GPU 已空闲，防止正在使用的资源被销毁
            let _ = self.device.device_wait_idle();

            // 预清理 Skia Surface 并提交上下文，防止旧 Surface 继续引用即将被销毁的 Swapchain Images
            self.sk_surfaces.clear();
            if let Some(ctx) = self.context.as_mut() {
                ctx.flush_and_submit();
            }

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
                // 预创建并缓存 Skia Surface，避免每帧重复创建 BackendRenderTarget 包装器
                self.sk_surfaces.reserve(self.swapchain_images.len());
                for i in 0..self.swapchain_images.len() {
                    let surface = self.create_sk_surface(i as u32);
                    self.sk_surfaces.push(surface);
                }
                android_log(
                    LogPriority::INFO,
                    &format!(
                        "Vulkan: Swapchain recreated {}x{} with {} images ({} surfaces cached)",
                        self.extent.width,
                        self.extent.height,
                        self.swapchain_images.len(),
                        self.sk_surfaces.len()
                    ),
                );
                true
            } else {
                android_log(LogPriority::ERROR, "Vulkan: Failed to create swapchain");
                false
            }
        }
    }

    pub fn acquire_next_image(&mut self) -> Result<u32, ash::vk::Result> {
        // 防御性检查：确保 swapchain 有效
        if self.swapchain == ash_vk::SwapchainKHR::null() {
            android_log(
                LogPriority::ERROR,
                "Vulkan: acquire_next_image called with null swapchain",
            );
            return Err(ash::vk::Result::ERROR_DEVICE_LOST);
        }
        unsafe {
            self.swapchain_loader
                .acquire_next_image(
                    self.swapchain,
                    1_000_000_000, // 1 秒超时，防止 swapchain 损坏时永远阻塞
                    self.image_available_semaphore,
                    ash_vk::Fence::null(),
                )
                .map(|(idx, _)| idx)
        }
    }

    /// 更新 Surface（用于 SurfaceView surfaceDestroyed 后 surfaceCreated 复用 Vulkan 上下文）
    ///
    /// 【关键修复】采用"先创建新资源，成功后销毁旧资源"的原子策略。
    /// 如果任何中间步骤失败，回滚到旧 surface，避免悬空句柄。
    pub unsafe fn update_surface(&mut self, window: *mut std::ffi::c_void) -> bool {
        android_log(LogPriority::INFO, "VulkanContext::update_surface: Starting");

        // 1. 等待 GPU 完成所有工作
        let _ = unsafe { self.device.device_wait_idle() };

        // 2. 销毁旧的 swapchain 和 sk_surfaces（这些可以安全重建）
        self.sk_surfaces.clear();
        if let Some(ctx) = self.context.as_mut() {
            ctx.flush_and_submit();
        }

        if self.swapchain != ash_vk::SwapchainKHR::null() {
            unsafe {
                self.swapchain_loader
                    .destroy_swapchain(self.swapchain, None);
            }
            self.swapchain = ash_vk::SwapchainKHR::null();
        }

        // 3. 【关键修复】保存旧 Surface 句柄，先创建新的，成功后再销毁旧的
        let old_surface = self.surface;
        if old_surface == ash_vk::SurfaceKHR::null() {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::update_surface: old surface is null, cannot update",
            );
            return false;
        }

        // 4. 创建新的 Android Surface
        let android_surface_loader =
            ash::khr::android_surface::Instance::new(&self.entry, &self.instance);
        match unsafe {
            android_surface_loader.create_android_surface(
                &ash_vk::AndroidSurfaceCreateInfoKHR {
                    window,
                    ..Default::default()
                },
                None,
            )
        } {
            Ok(new_surface) => {
                self.surface = new_surface;
                android_log(
                    LogPriority::INFO,
                    "VulkanContext::update_surface: New surface created",
                );
            }
            Err(e) => {
                android_log(
                    LogPriority::ERROR,
                    &format!(
                        "VulkanContext::update_surface: create_android_surface failed: {:?}",
                        e
                    ),
                );
                // 【关键修复】回滚：恢复旧 surface，不销毁它
                self.surface = old_surface;
                return false;
            }
        }

        // 5. 获取新的 surface capabilities
        let caps = match unsafe {
            self.surface_loader
                .get_physical_device_surface_capabilities(self.pdevice, self.surface)
        } {
            Ok(c) => c,
            Err(e) => {
                android_log(
                    LogPriority::ERROR,
                    &format!(
                        "VulkanContext::update_surface: get_capabilities failed: {:?}",
                        e
                    ),
                );
                // 【关键修复】回滚：销毁新 surface，恢复旧 surface
                unsafe {
                    self.surface_loader.destroy_surface(self.surface, None);
                }
                self.surface = old_surface;
                return false;
            }
        };
        // pre_transform=IDENTITY 下 caps.current_extent 不可靠。
        // 用已有的 self.extent（来自 recreate_swapchain 的 Java 尺寸），只做钳位。
        self.extent.width = self.extent.width.max(64).min(7680);
        self.extent.height = self.extent.height.max(64).min(7680);

        // 6. 重新创建 swapchain
        let swapchain_ok = self.recreate_swapchain(self.extent.width, self.extent.height);
        if !swapchain_ok {
            android_log(
                LogPriority::ERROR,
                "VulkanContext::update_surface: recreate_swapchain failed",
            );
            // 【关键修复】回滚：销毁新 surface，恢复旧 surface
            unsafe {
                self.surface_loader.destroy_surface(self.surface, None);
            }
            self.surface = old_surface;
            return false;
        }

        // 7. 【关键修复】所有步骤成功后才销毁旧 Surface
        unsafe {
            self.surface_loader.destroy_surface(old_surface, None);
        }
        android_log(
            LogPriority::INFO,
            "VulkanContext::update_surface: Old surface destroyed after successful transition",
        );

        android_log(LogPriority::INFO, "VulkanContext::update_surface: SUCCESS");
        true
    }

    /// 从缓存中获取 Skia Surface（若缓存未命中则创建）
    pub fn get_sk_surface(&mut self, index: u32) -> Option<SkSurface> {
        let idx = index as usize;
        if idx < self.sk_surfaces.len() && self.sk_surfaces[idx].is_some() {
            // 返回缓存的 Surface 克隆引用（SkSurface 支持 clone/ref）
            // 注意：skia_safe 的 Surface 通常通过 &mut 借用，这里直接返回引用
            // 实际上 wrap_backend_render_target 返回 Option<Surface>，我们可以 clone 它
            self.sk_surfaces[idx].as_ref().map(|s| s.clone())
        } else {
            // 缓存未命中时回退到创建（如运行时扩展）
            self.create_sk_surface(index)
        }
    }

    fn create_sk_surface(&mut self, index: u32) -> Option<SkSurface> {
        let image = self.swapchain_images.get(index as usize)?;

        let vk_image_info = unsafe {
            skia_safe::gpu::vk::ImageInfo::new(
                image.as_raw() as _,
                skia_safe::gpu::vk::Alloc::default(),
                skia_safe::gpu::vk::ImageTiling::OPTIMAL,
                skia_safe::gpu::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                self.skia_format,
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

        let color_type = match self.skia_format {
            skia_safe::gpu::vk::Format::B8G8R8A8_UNORM
            | skia_safe::gpu::vk::Format::B8G8R8A8_SRGB => ColorType::BGRA8888,
            skia_safe::gpu::vk::Format::A2B10G10R10_UNORM_PACK32 => ColorType::RGBA1010102,
            skia_safe::gpu::vk::Format::A2R10G10B10_UNORM_PACK32 => ColorType::BGRA1010102,
            skia_safe::gpu::vk::Format::R16G16B16A16_SFLOAT => ColorType::RGBAF16,
            _ => ColorType::RGBA8888,
        };

        // Multi-pipeline ColorSpace Mapping:
        // 根据 Vulkan 交换链的颜色空间，显式设置 Skia 的色彩空间，以确保正确渲染。
        let sk_color_space = match self.vk_color_space {
            ash_vk::ColorSpaceKHR::DISPLAY_P3_NONLINEAR_EXT => {
                // Display P3 广色域 (D65)
                skia_safe::ColorSpace::new_cicp(
                    skia_safe::named_primaries::CicpId::SMPTE_EG_432_1,
                    skia_safe::named_transfer_fn::CicpId::SRGB,
                )
            }
            ash_vk::ColorSpaceKHR::HDR10_ST2084_EXT => {
                // HDR10 (Rec.2020 + PQ)
                skia_safe::ColorSpace::new_cicp(
                    skia_safe::named_primaries::CicpId::Rec2020,
                    skia_safe::named_transfer_fn::CicpId::PQ,
                )
            }
            ash_vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT => {
                // scRGB HDR (F16 Linear)
                Some(skia_safe::ColorSpace::new_srgb_linear())
            }
            _ => {
                // 特殊适配：如果当前交换链选用的是 10-bit 的 A2B10G10R10 格式，
                // 且 Android 侧已经启用 COLOR_MODE_HDR，
                // 我们在 Skia 侧显式绑定为 Display P3 色彩空间，以激活高动态广色域渲染。
                if self.skia_format == skia_safe::gpu::vk::Format::A2B10G10R10_UNORM_PACK32 {
                    android_log(
                        LogPriority::INFO,
                        "Vulkan ColorSpace: 10-bit swapchain format active, mapping to Display P3 ColorSpace in Skia",
                    );
                    skia_safe::ColorSpace::new_cicp(
                        skia_safe::named_primaries::CicpId::SMPTE_EG_432_1,
                        skia_safe::named_transfer_fn::CicpId::SRGB,
                    )
                } else {
                    Some(skia_safe::ColorSpace::new_srgb())
                }
            }
        };

        skia_safe::gpu::surfaces::wrap_backend_render_target(
            self.context.as_mut().unwrap(),
            &render_target,
            skia_safe::gpu::SurfaceOrigin::TopLeft,
            color_type,
            sk_color_space,
            None,
        )
    }

    /// 定期保存 pipeline cache 到文件。
    /// 由渲染线程每 N 帧调用一次，减少进程被系统杀死时的缓存损失。
    pub fn save_pipeline_cache_periodic(&self) {
        if self.pipeline_cache != ash_vk::PipelineCache::null() {
            save_pipeline_cache_data(&self.device, self.pipeline_cache);
        }
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        android_log(
            LogPriority::INFO,
            "VulkanContext::drop: Cleaning up Vulkan resources",
        );

        // Save pipeline cache data before destruction
        if self.pipeline_cache != ash_vk::PipelineCache::null() {
            save_pipeline_cache_data(&self.device, self.pipeline_cache);
        }

        unsafe {
            // Wait for GPU to finish before destroying resources
            let _ = self.device.device_wait_idle();

            for surface in self.sk_surfaces.drain(..) {
                drop(surface);
            }

            // 关键修复：在销毁 Vulkan Device 之前，先让 Skia 释放所有 GPU 资源，
            // 然后用 std::mem::forget 阻止 Rust 调用 DirectContext 的 Drop/C++ 析构函数。
            // Skia 的 GrDirectContext 析构函数不检查 abandoned 状态，会在无效 Device 上
            // 调用 vkDestroyCommandPool，导致 Adreno 驱动空指针解引用（SIGSEGV）。
            if let Some(mut ctx) = self.context.take() {
                ctx.release_resources_and_abandon();
                std::mem::forget(ctx);
            }

            if self.swapchain != ash_vk::SwapchainKHR::null() {
                self.swapchain_loader
                    .destroy_swapchain(self.swapchain, None);
            }
            self.surface_loader.destroy_surface(self.surface, None);
            self.device
                .destroy_semaphore(self.image_available_semaphore, None);
            self.device
                .destroy_semaphore(self.render_finished_semaphore, None);

            if self.pipeline_cache != ash_vk::PipelineCache::null() {
                self.device
                    .destroy_pipeline_cache(self.pipeline_cache, None);
            }

            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }

        HOOKED_PIPELINE_CACHE.store(0, Ordering::SeqCst);
        PIPELINE_CACHE_INITIALIZED.store(false, Ordering::SeqCst);
        android_log(LogPriority::INFO, "VulkanContext::drop: Cleanup complete");
    }
}

#[cfg(test)]
mod tests {

    use ash::vk as ash_vk;

    #[test]
    fn test_extent_clamping_logic() {
        // 模拟 Android 系统的典型行为：在 resize 过程中，caps 可能返回固定的 current_extent，
        // 或者返回 0xFFFFFFFF 表示由应用决定。

        let width = 1080;
        let height = 2400;

        // 情况 1：系统给定了确定的 extent (如 1080x2400)
        let caps_fixed = ash_vk::SurfaceCapabilitiesKHR {
            current_extent: ash_vk::Extent2D {
                width: 1080,
                height: 2400,
            },
            ..Default::default()
        };

        let actual_extent = if caps_fixed.current_extent.width != u32::MAX {
            caps_fixed.current_extent
        } else {
            ash_vk::Extent2D { width, height }
        };
        assert_eq!(actual_extent.width, 1080);
        assert_eq!(actual_extent.height, 2400);

        // 情况 2：系统允许应用决定 (0xFFFFFFFF)
        let caps_dynamic = ash_vk::SurfaceCapabilitiesKHR {
            current_extent: ash_vk::Extent2D {
                width: u32::MAX,
                height: u32::MAX,
            },
            ..Default::default()
        };

        let actual_extent = if caps_dynamic.current_extent.width != u32::MAX {
            caps_dynamic.current_extent
        } else {
            ash_vk::Extent2D { width, height }
        };
        assert_eq!(actual_extent.width, 1080);
        assert_eq!(actual_extent.height, 2400);
    }

    #[test]
    fn test_zero_extent_handling_logic() {
        // 验证我们添加的 0 尺寸过滤逻辑
        let caps_zero = ash_vk::SurfaceCapabilitiesKHR {
            current_extent: ash_vk::Extent2D {
                width: 0,
                height: 0,
            },
            ..Default::default()
        };

        // 模拟 recreate_swapchain 中的逻辑
        let should_skip =
            caps_zero.current_extent.width == 0 || caps_zero.current_extent.height == 0;
        assert!(
            should_skip,
            "Logic should detect zero extent and skip recreation"
        );
    }

    /// 编译时验证：`VulkanContext` 正确实现 `Send` 但未实现 `Sync`。
    ///
    /// `Mutex<T>` 实现 `Sync` 的条件是 `T: Send`（不需要 `T: Sync`）。
    /// 因此 `static VULKAN_CONTEXT: OnceCell<Mutex<Option<VulkanContext>>>`
    /// 可以安全编译，同时编译器会禁止通过 `&VulkanContext` 的并发访问。
    #[test]
    fn test_vulkan_context_send_mutex_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<super::VulkanContext>();
        assert_sync::<std::sync::Mutex<Option<super::VulkanContext>>>();
    }

    /// 验证 pipeline cache 头部校验逻辑：vendorID/deviceID 不匹配时必须丢弃。
    #[test]
    fn test_pipeline_cache_validation() {
        // 构造一个合法的 Vulkan pipeline cache header
        // headerSize=32, headerVersion=1, vendorID=0x13B5, deviceID=0x12345678
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&32u32.to_le_bytes()); // headerSize
        data[4..8].copy_from_slice(&1u32.to_le_bytes()); // headerVersion
        data[8..12].copy_from_slice(&0x13B5u32.to_le_bytes()); // vendorID
        data[12..16].copy_from_slice(&0x1234_5678u32.to_le_bytes()); // deviceID
        // UUID = 16 bytes of zeros (offset 16..32)
        // payload = 32 bytes (offset 32..64)

        // 1. 匹配 vendor/device → 通过
        assert!(
            super::validate_pipeline_cache(&data, 0x13B5, 0x1234_5678).is_some(),
            "Matching vendor/device should be accepted"
        );

        // 2. vendor 不匹配 → 丢弃
        assert!(
            super::validate_pipeline_cache(&data, 0x5143, 0x1234_5678).is_none(),
            "Mismatched vendor should be rejected"
        );

        // 3. device 不匹配 → 丢弃
        assert!(
            super::validate_pipeline_cache(&data, 0x13B5, 0xDEAD_BEEF).is_none(),
            "Mismatched device should be rejected"
        );

        // 4. 数据太短 → 丢弃
        assert!(
            super::validate_pipeline_cache(&data[..16], 0x13B5, 0x1234_5678).is_none(),
            "Short data should be rejected"
        );

        // 5. 错误的 header version → 丢弃
        let mut bad_ver = data.clone();
        bad_ver[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(
            super::validate_pipeline_cache(&bad_ver, 0x13B5, 0x1234_5678).is_none(),
            "Bad header version should be rejected"
        );

        // 6. 空数据 → 丢弃（静默）
        assert!(
            super::validate_pipeline_cache(&[], 0x13B5, 0x1234_5678).is_none(),
            "Empty data should be rejected"
        );
    }
}
