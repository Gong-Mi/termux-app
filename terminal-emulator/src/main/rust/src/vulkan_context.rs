use skia_safe::{gpu::vk, gpu::DirectContext, Surface as SkSurface, ColorType};
use ash::{vk as ash_vk, Entry, Instance, Device};
use ash::khr::swapchain;
use ash::vk::Handle;
use std::ffi::CStr;

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
}

unsafe impl Send for VulkanContext {}
unsafe impl Sync for VulkanContext {}

impl VulkanContext {
    pub unsafe fn new(window: *mut std::ffi::c_void) -> Option<Self> {
        let entry = unsafe { Entry::load().ok()? };
        
        // 1. Instance
        let app_info = ash_vk::ApplicationInfo {
            api_version: ash_vk::API_VERSION_1_1,
            ..Default::default()
        };
        
        let extension_names = [
            ash::khr::surface::NAME.as_ptr(),
            ash::khr::android_surface::NAME.as_ptr(),
        ];

        let create_info = ash_vk::InstanceCreateInfo {
            p_application_info: &app_info,
            enabled_extension_count: extension_names.len() as u32,
            pp_enabled_extension_names: extension_names.as_ptr(),
            ..Default::default()
        };

        let instance = unsafe { entry.create_instance(&create_info, None).ok()? };
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let android_surface_loader = ash::khr::android_surface::Instance::new(&entry, &instance);
        
        let surface_create_info = ash_vk::AndroidSurfaceCreateInfoKHR {
            window,
            ..Default::default()
        };
        let surface = unsafe { android_surface_loader.create_android_surface(&surface_create_info, None).ok()? };

        let pdevices = unsafe { instance.enumerate_physical_devices().ok()? };
        if pdevices.is_empty() { return None; }
        let pdevice = pdevices[0];
        let queue_family_index = 0;
        
        let device_extensions = [swapchain::NAME.as_ptr()];
        let queue_priorities = [1.0];
        let queue_info = ash_vk::DeviceQueueCreateInfo {
            queue_family_index,
            queue_count: 1,
            p_queue_priorities: queue_priorities.as_ptr(),
            ..Default::default()
        };

        let device_create_info = ash_vk::DeviceCreateInfo {
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_info,
            enabled_extension_count: device_extensions.len() as u32,
            pp_enabled_extension_names: device_extensions.as_ptr(),
            ..Default::default()
        };

        let device = unsafe { instance.create_device(pdevice, &device_create_info, None).ok()? };
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        let swapchain_loader = swapchain::Device::new(&instance, &device);

        let caps = unsafe { surface_loader.get_physical_device_surface_capabilities(pdevice, surface).ok()? };
        let extent = caps.current_extent;
        let swapchain_create_info = ash_vk::SwapchainCreateInfoKHR {
            surface,
            min_image_count: 2.max(caps.min_image_count),
            image_format: ash_vk::Format::R8G8B8A8_UNORM,
            image_color_space: ash_vk::ColorSpaceKHR::SRGB_NONLINEAR,
            image_extent: extent,
            image_array_layers: 1,
            image_usage: ash_vk::ImageUsageFlags::COLOR_ATTACHMENT,
            pre_transform: caps.current_transform,
            composite_alpha: ash_vk::CompositeAlphaFlagsKHR::OPAQUE,
            present_mode: ash_vk::PresentModeKHR::FIFO,
            clipped: ash_vk::TRUE,
            ..Default::default()
        };
        
        let swapchain = unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None).ok()? };
        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain).ok()? };

        let entry_ptr = entry.clone();
        let get_proc = move |of: vk::GetProcOf| {
            unsafe {
                match of {
                    vk::GetProcOf::Instance(inst, name) => {
                        let name_cstr = CStr::from_ptr(name);
                        entry_ptr.get_instance_proc_addr(ash_vk::Instance::from_raw(inst as _), name_cstr.as_ptr())
                            .map(|f| f as _)
                            .unwrap_or(std::ptr::null())
                    }
                    vk::GetProcOf::Device(dev, name) => {
                        let name_cstr = CStr::from_ptr(name);
                        entry_ptr.get_instance_proc_addr(ash_vk::Instance::from_raw(dev as _), name_cstr.as_ptr())
                            .map(|f| f as _)
                            .unwrap_or(std::ptr::null())
                    }
                }
            }
        };

        let backend_context = unsafe {
            vk::BackendContext::new(
                instance.handle().as_raw() as _,
                pdevice.as_raw() as _,
                device.handle().as_raw() as _,
                (queue.as_raw() as _, queue_family_index as usize),
                &get_proc,
            )
        };

        let context = skia_safe::gpu::direct_contexts::make_vulkan(&backend_context, None)?;

        Some(Self {
            instance, device, context, queue, graphics_queue_index: queue_family_index,
            surface, surface_loader, swapchain_loader, swapchain, swapchain_images, extent
        })
    }

    pub fn get_sk_surface(&mut self, index: usize) -> Option<SkSurface> {
        let image = self.swapchain_images.get(index)?;
        let image_info = vk::ImageInfo {
            image: image.as_raw() as _,
            alloc: vk::Alloc::default(),
            tiling: vk::ImageTiling::OPTIMAL,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            format: vk::Format::R8G8B8A8_UNORM,
            image_usage_flags: ash_vk::ImageUsageFlags::COLOR_ATTACHMENT.as_raw() as _,
            sample_count: 1,
            level_count: 1,
            ..Default::default()
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
