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

        let app_info = ash_vk::ApplicationInfo { api_version: ash_vk::API_VERSION_1_1, ..Default::default() };
        let extension_names = [ash::khr::surface::NAME.as_ptr(), ash::khr::android_surface::NAME.as_ptr()];
        let create_info = ash_vk::InstanceCreateInfo { p_application_info: &app_info, enabled_extension_count: extension_names.len() as u32, pp_enabled_extension_names: extension_names.as_ptr(), ..Default::default() };

        let instance = unsafe { entry.create_instance(&create_info, None) };
        if instance.is_err() {
            android_log(LogPriority::ERROR, &format!("VulkanContext::new: create_instance failed: {:?}", instance.err()));
            return None;
        }
        let instance = instance.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Instance created");

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
            android_log(LogPriority::ERROR, "VulkanContext::new: enumerate_physical_devices failed or empty");
            return None;
        }
        let pdevices = pdevices.unwrap();
        let pdevice = pdevices[0];
        android_log(LogPriority::INFO, "VulkanContext::new: Physical device found");

        let queue_family_index = 0;

        let device_extensions = [swapchain::NAME.as_ptr()];
        let queue_info = ash_vk::DeviceQueueCreateInfo { queue_family_index, queue_count: 1, p_queue_priorities: [1.0].as_ptr(), ..Default::default() };
        let device_create_info = ash_vk::DeviceCreateInfo { queue_create_info_count: 1, p_queue_create_infos: &queue_info, enabled_extension_count: device_extensions.len() as u32, pp_enabled_extension_names: device_extensions.as_ptr(), ..Default::default() };

        let device = unsafe { instance.create_device(pdevice, &device_create_info, None) };
        if device.is_err() {
            android_log(LogPriority::ERROR, &format!("VulkanContext::new: create_device failed: {:?}", device.err()));
            return None;
        }
        let device = device.unwrap();
        android_log(LogPriority::INFO, "VulkanContext::new: Device created");

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
            surface, surface_loader, swapchain_loader,
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
            let swapchain_create_info = ash_vk::SwapchainCreateInfoKHR {
                surface: self.surface,
                min_image_count: 2,
                image_format: ash_vk::Format::R8G8B8A8_UNORM,
                image_color_space: ash_vk::ColorSpaceKHR::SRGB_NONLINEAR,
                image_extent: self.extent,
                image_array_layers: 1,
                image_usage: ash_vk::ImageUsageFlags::COLOR_ATTACHMENT,
                pre_transform: ash_vk::SurfaceTransformFlagsKHR::IDENTITY,
                composite_alpha: ash_vk::CompositeAlphaFlagsKHR::OPAQUE,
                present_mode: ash_vk::PresentModeKHR::FIFO,
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
