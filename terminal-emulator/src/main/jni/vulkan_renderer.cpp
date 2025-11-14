#include <jni.h>
#include <vector>
#include <string>
#include <stdexcept>
#include <android/native_window.h>
#include <android/native_window_jni.h>
#include <vulkan/vulkan.h>
#include <vulkan/vulkan_android.h>
#include <android/log.h>

#define LOG_TAG "VulkanRenderer"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)

#define VK_CHECK(result) \
    if (result != VK_SUCCESS) { \
        LOGE("Vulkan error in %s at line %d: %d", __FILE__, __LINE__, result); \
        throw std::runtime_error("Vulkan error"); \
    }

struct VulkanContext {
    VkInstance instance = VK_NULL_HANDLE;
    VkPhysicalDevice physicalDevice = VK_NULL_HANDLE;
    VkDevice device = VK_NULL_HANDLE;
    uint32_t queueFamilyIndex = 0;
    VkQueue queue = VK_NULL_HANDLE;
    VkSurfaceKHR surface = VK_NULL_HANDLE;

    VkSwapchainKHR swapchain = VK_NULL_HANDLE;
    VkFormat swapchainFormat = VK_FORMAT_UNDEFINED;
    VkExtent2D swapchainExtent{};
    std::vector<VkImage> swapchainImages;
    std::vector<VkImageView> swapchainImageViews;

    VkRenderPass renderPass = VK_NULL_HANDLE;
    std::vector<VkFramebuffer> framebuffers;

    VkCommandPool commandPool = VK_NULL_HANDLE;
    std::vector<VkCommandBuffer> commandBuffers;

    VkSemaphore imageAvailableSemaphore = VK_NULL_HANDLE;
    VkSemaphore renderFinishedSemaphore = VK_NULL_HANDLE;
    std::vector<VkFence> inFlightFences;
    size_t currentFrame = 0;

    ANativeWindow* window = nullptr;
    bool running = false;
};

VulkanContext g_ctx;

void initVulkan() {
    VkApplicationInfo appInfo{};
    appInfo.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    appInfo.pApplicationName = "Termux";
    appInfo.applicationVersion = VK_MAKE_VERSION(1, 0, 0);
    appInfo.pEngineName = "No Engine";
    appInfo.engineVersion = VK_MAKE_VERSION(1, 0, 0);
    appInfo.apiVersion = VK_API_VERSION_1_1;

    const std::vector<const char*> requiredExtensions = {
        VK_KHR_SURFACE_EXTENSION_NAME,
        VK_KHR_ANDROID_SURFACE_EXTENSION_NAME
    };

    VkInstanceCreateInfo createInfo{};
    createInfo.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    createInfo.pApplicationInfo = &appInfo;
    createInfo.enabledExtensionCount = static_cast<uint32_t>(requiredExtensions.size());
    createInfo.ppEnabledExtensionNames = requiredExtensions.data();
    createInfo.enabledLayerCount = 0;

    VK_CHECK(vkCreateInstance(&createInfo, nullptr, &g_ctx.instance));
    LOGI("Vulkan instance created.");
}

void pickPhysicalDevice() {
    uint32_t deviceCount = 0;
    vkEnumeratePhysicalDevices(g_ctx.instance, &deviceCount, nullptr);
    if (deviceCount == 0) throw std::runtime_error("Failed to find GPUs with Vulkan support!");
    std::vector<VkPhysicalDevice> devices(deviceCount);
    vkEnumeratePhysicalDevices(g_ctx.instance, &deviceCount, devices.data());
    g_ctx.physicalDevice = devices[0]; // Pick the first one
    LOGI("Physical device selected.");
}

void createLogicalDevice() {
    uint32_t queueFamilyCount = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(g_ctx.physicalDevice, &queueFamilyCount, nullptr);
    std::vector<VkQueueFamilyProperties> queueFamilies(queueFamilyCount);
    vkGetPhysicalDeviceQueueFamilyProperties(g_ctx.physicalDevice, &queueFamilyCount, queueFamilies.data());

    bool found = false;
    for (uint32_t i = 0; i < queueFamilies.size(); i++) {
        if (queueFamilies[i].queueFlags & VK_QUEUE_GRAPHICS_BIT) {
            g_ctx.queueFamilyIndex = i;
            found = true;
            break;
        }
    }
    if (!found) throw std::runtime_error("Failed to find a graphics queue family.");

    float queuePriority = 1.0f;
    VkDeviceQueueCreateInfo queueCreateInfo{};
    queueCreateInfo.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
    queueCreateInfo.queueFamilyIndex = g_ctx.queueFamilyIndex;
    queueCreateInfo.queueCount = 1;
    queueCreateInfo.pQueuePriorities = &queuePriority;

    const std::vector<const char*> deviceExtensions = { VK_KHR_SWAPCHAIN_EXTENSION_NAME };
    VkDeviceCreateInfo deviceCreateInfo{};
    deviceCreateInfo.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO;
    deviceCreateInfo.pQueueCreateInfos = &queueCreateInfo;
    deviceCreateInfo.queueCreateInfoCount = 1;
    deviceCreateInfo.enabledExtensionCount = static_cast<uint32_t>(deviceExtensions.size());
    deviceCreateInfo.ppEnabledExtensionNames = deviceExtensions.data();
    deviceCreateInfo.enabledLayerCount = 0;

    VK_CHECK(vkCreateDevice(g_ctx.physicalDevice, &deviceCreateInfo, nullptr, &g_ctx.device));
    vkGetDeviceQueue(g_ctx.device, g_ctx.queueFamilyIndex, 0, &g_ctx.queue);
    LOGI("Logical device and queue created.");
}

void createSurface(ANativeWindow* window) {
    g_ctx.window = window;
    VkAndroidSurfaceCreateInfoKHR createInfo{};
    createInfo.sType = VK_STRUCTURE_TYPE_ANDROID_SURFACE_CREATE_INFO_KHR;
    createInfo.window = window;
    VK_CHECK(vkCreateAndroidSurfaceKHR(g_ctx.instance, &createInfo, nullptr, &g_ctx.surface));
    LOGI("Android surface created.");
}

void createSwapchain() {
    VkSurfaceCapabilitiesKHR capabilities;
    vkGetPhysicalDeviceSurfaceCapabilitiesKHR(g_ctx.physicalDevice, g_ctx.surface, &capabilities);

    g_ctx.swapchainExtent = capabilities.currentExtent;
    g_ctx.swapchainFormat = VK_FORMAT_B8G8R8A8_UNORM; // Common format

    uint32_t imageCount = capabilities.minImageCount + 1;
    if (capabilities.maxImageCount > 0 && imageCount > capabilities.maxImageCount) {
        imageCount = capabilities.maxImageCount;
    }

    VkSwapchainCreateInfoKHR createInfo{};
    createInfo.sType = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR;
    createInfo.surface = g_ctx.surface;
    createInfo.minImageCount = imageCount;
    createInfo.imageFormat = g_ctx.swapchainFormat;
    createInfo.imageColorSpace = VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;
    createInfo.imageExtent = g_ctx.swapchainExtent;
    createInfo.imageArrayLayers = 1;
    createInfo.imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT;
    createInfo.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE;
    createInfo.preTransform = capabilities.currentTransform;
    createInfo.compositeAlpha = VK_COMPOSITE_ALPHA_INHERIT_BIT_KHR;
    createInfo.presentMode = VK_PRESENT_MODE_FIFO_KHR;
    createInfo.clipped = VK_TRUE;
    createInfo.oldSwapchain = VK_NULL_HANDLE;

    VK_CHECK(vkCreateSwapchainKHR(g_ctx.device, &createInfo, nullptr, &g_ctx.swapchain));

    vkGetSwapchainImagesKHR(g_ctx.device, g_ctx.swapchain, &imageCount, nullptr);
    g_ctx.swapchainImages.resize(imageCount);
    vkGetSwapchainImagesKHR(g_ctx.device, g_ctx.swapchain, &imageCount, g_ctx.swapchainImages.data());
    LOGI("Swapchain created with %d images.", imageCount);
}

void createImageViews() {
    g_ctx.swapchainImageViews.resize(g_ctx.swapchainImages.size());
    for (size_t i = 0; i < g_ctx.swapchainImages.size(); i++) {
        VkImageViewCreateInfo createInfo{};
        createInfo.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
        createInfo.image = g_ctx.swapchainImages[i];
        createInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
        createInfo.format = g_ctx.swapchainFormat;
        createInfo.components.r = VK_COMPONENT_SWIZZLE_IDENTITY;
        createInfo.components.g = VK_COMPONENT_SWIZZLE_IDENTITY;
        createInfo.components.b = VK_COMPONENT_SWIZZLE_IDENTITY;
        createInfo.components.a = VK_COMPONENT_SWIZZLE_IDENTITY;
        createInfo.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
        createInfo.subresourceRange.baseMipLevel = 0;
        createInfo.subresourceRange.levelCount = 1;
        createInfo.subresourceRange.baseArrayLayer = 0;
        createInfo.subresourceRange.layerCount = 1;
        VK_CHECK(vkCreateImageView(g_ctx.device, &createInfo, nullptr, &g_ctx.swapchainImageViews[i]));
    }
    LOGI("Image views created.");
}

void createRenderPass() {
    VkAttachmentDescription colorAttachment{};
    colorAttachment.format = g_ctx.swapchainFormat;
    colorAttachment.samples = VK_SAMPLE_COUNT_1_BIT;
    colorAttachment.loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR;
    colorAttachment.storeOp = VK_ATTACHMENT_STORE_OP_STORE;
    colorAttachment.stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE;
    colorAttachment.stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE;
    colorAttachment.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    colorAttachment.finalLayout = VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;

    VkAttachmentReference colorAttachmentRef{};
    colorAttachmentRef.attachment = 0;
    colorAttachmentRef.layout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;

    VkSubpassDescription subpass{};
    subpass.pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS;
    subpass.colorAttachmentCount = 1;
    subpass.pColorAttachments = &colorAttachmentRef;

    VkRenderPassCreateInfo renderPassInfo{};
    renderPassInfo.sType = VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO;
    renderPassInfo.attachmentCount = 1;
    renderPassInfo.pAttachments = &colorAttachment;
    renderPassInfo.subpassCount = 1;
    renderPassInfo.pSubpasses = &subpass;

    VK_CHECK(vkCreateRenderPass(g_ctx.device, &renderPassInfo, nullptr, &g_ctx.renderPass));
    LOGI("Render pass created.");
}

void createFramebuffers() {
    g_ctx.framebuffers.resize(g_ctx.swapchainImageViews.size());
    for (size_t i = 0; i < g_ctx.swapchainImageViews.size(); i++) {
        VkImageView attachments[] = { g_ctx.swapchainImageViews[i] };
        VkFramebufferCreateInfo framebufferInfo{};
        framebufferInfo.sType = VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO;
        framebufferInfo.renderPass = g_ctx.renderPass;
        framebufferInfo.attachmentCount = 1;
        framebufferInfo.pAttachments = attachments;
        framebufferInfo.width = g_ctx.swapchainExtent.width;
        framebufferInfo.height = g_ctx.swapchainExtent.height;
        framebufferInfo.layers = 1;
        VK_CHECK(vkCreateFramebuffer(g_ctx.device, &framebufferInfo, nullptr, &g_ctx.framebuffers[i]));
    }
    LOGI("Framebuffers created.");
}

void createCommandPool() {
    VkCommandPoolCreateInfo poolInfo{};
    poolInfo.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.queueFamilyIndex = g_ctx.queueFamilyIndex;
    poolInfo.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    VK_CHECK(vkCreateCommandPool(g_ctx.device, &poolInfo, nullptr, &g_ctx.commandPool));
    LOGI("Command pool created.");
}

void createCommandBuffers() {
    g_ctx.commandBuffers.resize(g_ctx.framebuffers.size());
    VkCommandBufferAllocateInfo allocInfo{};
    allocInfo.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocInfo.commandPool = g_ctx.commandPool;
    allocInfo.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    allocInfo.commandBufferCount = (uint32_t)g_ctx.commandBuffers.size();
    VK_CHECK(vkAllocateCommandBuffers(g_ctx.device, &allocInfo, g_ctx.commandBuffers.data()));
    LOGI("Command buffers allocated.");
}

void recordCommandBuffer(uint32_t imageIndex) {
    VkCommandBufferBeginInfo beginInfo{};
    beginInfo.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    VK_CHECK(vkBeginCommandBuffer(g_ctx.commandBuffers[g_ctx.currentFrame], &beginInfo));

    VkRenderPassBeginInfo renderPassInfo{};
    renderPassInfo.sType = VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO;
    renderPassInfo.renderPass = g_ctx.renderPass;
    renderPassInfo.framebuffer = g_ctx.framebuffers[imageIndex];
    renderPassInfo.renderArea.offset = {0, 0};
    renderPassInfo.renderArea.extent = g_ctx.swapchainExtent;

    VkClearValue clearColor = {{0.05f, 0.05f, 0.05f, 1.0f}};
    renderPassInfo.clearValueCount = 1;
    renderPassInfo.pClearValues = &clearColor;

    vkCmdBeginRenderPass(g_ctx.commandBuffers[g_ctx.currentFrame], &renderPassInfo, VK_SUBPASS_CONTENTS_INLINE);
    // TODO: Record actual drawing commands here
    vkCmdEndRenderPass(g_ctx.commandBuffers[g_ctx.currentFrame]);

    VK_CHECK(vkEndCommandBuffer(g_ctx.commandBuffers[g_ctx.currentFrame]));
}

void createSyncObjects() {
    g_ctx.inFlightFences.resize(g_ctx.commandBuffers.size());
    VkSemaphoreCreateInfo semaphoreInfo{};
    semaphoreInfo.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
    VkFenceCreateInfo fenceInfo{};
    fenceInfo.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
    fenceInfo.flags = VK_FENCE_CREATE_SIGNALED_BIT;

    VK_CHECK(vkCreateSemaphore(g_ctx.device, &semaphoreInfo, nullptr, &g_ctx.imageAvailableSemaphore));
    VK_CHECK(vkCreateSemaphore(g_ctx.device, &semaphoreInfo, nullptr, &g_ctx.renderFinishedSemaphore));

    for (size_t i = 0; i < g_ctx.inFlightFences.size(); i++) {
        VK_CHECK(vkCreateFence(g_ctx.device, &fenceInfo, nullptr, &g_ctx.inFlightFences[i]));
    }
    LOGI("Sync objects created.");
}

void cleanupSwapchain() {
    for (auto framebuffer : g_ctx.framebuffers) {
        vkDestroyFramebuffer(g_ctx.device, framebuffer, nullptr);
    }
    for (auto imageView : g_ctx.swapchainImageViews) {
        vkDestroyImageView(g_ctx.device, imageView, nullptr);
    }
    vkDestroySwapchainKHR(g_ctx.device, g_ctx.swapchain, nullptr);
}

void cleanup() {
    vkDeviceWaitIdle(g_ctx.device);
    cleanupSwapchain();
    vkDestroySemaphore(g_ctx.device, g_ctx.renderFinishedSemaphore, nullptr);
    vkDestroySemaphore(g_ctx.device, g_ctx.imageAvailableSemaphore, nullptr);
    for(auto fence : g_ctx.inFlightFences) {
        vkDestroyFence(g_ctx.device, fence, nullptr);
    }
    vkDestroyCommandPool(g_ctx.device, g_ctx.commandPool, nullptr);
    vkDestroyRenderPass(g_ctx.device, g_ctx.renderPass, nullptr);
    vkDestroyDevice(g_ctx.device, nullptr);
    vkDestroySurfaceKHR(g_ctx.instance, g_ctx.surface, nullptr);
    vkDestroyInstance(g_ctx.instance, nullptr);
    if (g_ctx.window) {
        ANativeWindow_release(g_ctx.window);
    }
    g_ctx = {};
    LOGI("Vulkan context cleaned up.");
}

void recreateSwapchain() {
    vkDeviceWaitIdle(g_ctx.device);
    cleanupSwapchain();
    createSwapchain();
    createImageViews();
    createFramebuffers();
}

void drawFrame() {
    vkWaitForFences(g_ctx.device, 1, &g_ctx.inFlightFences[g_ctx.currentFrame], VK_TRUE, UINT64_MAX);

    uint32_t imageIndex;
    VkResult result = vkAcquireNextImageKHR(g_ctx.device, g_ctx.swapchain, UINT64_MAX, g_ctx.imageAvailableSemaphore, VK_NULL_HANDLE, &imageIndex);

    if (result == VK_ERROR_OUT_OF_DATE_KHR) {
        recreateSwapchain();
        return;
    } else if (result != VK_SUCCESS && result != VK_SUBOPTIMAL_KHR) {
        throw std::runtime_error("failed to acquire swap chain image!");
    }

    vkResetFences(g_ctx.device, 1, &g_ctx.inFlightFences[g_ctx.currentFrame]);
    vkResetCommandBuffer(g_ctx.commandBuffers[g_ctx.currentFrame], 0);
    recordCommandBuffer(imageIndex);

    VkSubmitInfo submitInfo{};
    submitInfo.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
    VkSemaphore waitSemaphores[] = {g_ctx.imageAvailableSemaphore};
    VkPipelineStageFlags waitStages[] = {VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT};
    submitInfo.waitSemaphoreCount = 1;
    submitInfo.pWaitSemaphores = waitSemaphores;
    submitInfo.pWaitDstStageMask = waitStages;
    submitInfo.commandBufferCount = 1;
    submitInfo.pCommandBuffers = &g_ctx.commandBuffers[g_ctx.currentFrame];
    VkSemaphore signalSemaphores[] = {g_ctx.renderFinishedSemaphore};
    submitInfo.signalSemaphoreCount = 1;
    submitInfo.pSignalSemaphores = signalSemaphores;

    VK_CHECK(vkQueueSubmit(g_ctx.queue, 1, &submitInfo, g_ctx.inFlightFences[g_ctx.currentFrame]));

    VkPresentInfoKHR presentInfo{};
    presentInfo.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR;
    presentInfo.waitSemaphoreCount = 1;
    presentInfo.pWaitSemaphores = signalSemaphores;
    VkSwapchainKHR swapChains[] = {g_ctx.swapchain};
    presentInfo.swapchainCount = 1;
    presentInfo.pSwapchains = swapChains;
    presentInfo.pImageIndices = &imageIndex;

    result = vkQueuePresentKHR(g_ctx.queue, &presentInfo);
    if (result == VK_ERROR_OUT_OF_DATE_KHR || result == VK_SUBOPTIMAL_KHR) {
        recreateSwapchain();
    } else if (result != VK_SUCCESS) {
        throw std::runtime_error("failed to present swap chain image!");
    }

    g_ctx.currentFrame = (g_ctx.currentFrame + 1) % g_ctx.inFlightFences.size();
}


extern "C" {
    JNIEXPORT void JNICALL
    Java_com_termux_hg_view_VulkanTerminalView_nativeInit(JNIEnv* /*env*/, jobject /*thiz*/) {
        try {
            initVulkan();
            pickPhysicalDevice();
            createLogicalDevice();
        } catch (const std::exception& e) {
            LOGE("Vulkan Init failed: %s", e.what());
        }
    }

    JNIEXPORT void JNICALL
    Java_com_termux_hg_view_VulkanTerminalView_nativeSetSurface(JNIEnv* env, jobject /*thiz*/, jobject surface) {
        try {
            if (surface != nullptr) {
                ANativeWindow* window = ANativeWindow_fromSurface(env, surface);
                LOGI("Setting surface: %p", window);
                createSurface(window);
                createSwapchain();
                createImageViews();
                createRenderPass();
                createFramebuffers();
                createCommandPool();
                createCommandBuffers();
                createSyncObjects();
                g_ctx.running = true;
            } else {
                LOGI("Clearing surface");
                g_ctx.running = false;
                vkDeviceWaitIdle(g_ctx.device);
                cleanupSwapchain();
                // Keep other resources for now
            }
        } catch (const std::exception& e) {
            LOGE("Vulkan SetSurface failed: %s", e.what());
        }
    }

    JNIEXPORT void JNICALL
    Java_com_termux_hg_view_VulkanTerminalView_nativeDestroy(JNIEnv* /*env*/, jobject /*thiz*/) {
        g_ctx.running = false;
        cleanup();
    }

    JNIEXPORT void JNICALL
    Java_com_termux_hg_view_VulkanTerminalView_nativeRender(JNIEnv* /*env*/, jobject /*thiz*/) {
        if (g_ctx.running) {
            try {
                drawFrame();
            } catch (const std::exception& e) {
                LOGE("Vulkan drawFrame failed: %s", e.what());
                g_ctx.running = false;
            }
        }
    }
}
