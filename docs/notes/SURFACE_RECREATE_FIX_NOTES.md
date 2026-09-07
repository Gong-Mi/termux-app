# Termux Vulkan 渲染 Surface 重建排坑笔记

> 设备：Xiaomi 25098PN5AC (HyperOS, Android 16/SDK 36, arm64-v8a)
> 问题：Activity 启动 / IME 收起后终端黑屏，SurfaceView 不重建
> 时间：2026-05-02

---

## 1. 问题现象

### 1.1 Activity 启动即黑屏

应用启动后，logcat 出现以下序列：

```
I TerminalView-Surface: >>> surfaceCreated
D Termux-Rust: nativeSetSurface: Non-null surface received
I Termux-Rust: VulkanContext::new: SUCCESS          ← Vulkan 初始化成功
I TerminalView-Surface: >>> surfaceChanged: 1220x2506
I TerminalView-Surface: >>> surfaceDestroyed          ← 58ms 后就被销毁！
I Termux-Rust: nativeSetSurface: Surface is NULL
```

然后**长达数分钟**没有 `surfaceCreated` 回调，终端完全黑屏。

### 1.2 输入法收起后黑屏

IME 弹出时 `surfaceChanged` 正常，但 IME 收起后：
- `surfaceChanged` 有时不被调用
- 即使调用，swapchain 尺寸未同步，渲染错位或黑屏

---

## 2. 根因分析

### 2.1 系统层根因：MIUI/HyperOS 窗口动画 Bug

**Android SurfaceView 在 BLAST 模式下的生命周期：**

```
Activity.onCreate()
  → setContentView() inflate SurfaceView
  → ViewRootImpl.performTraversals()
    → SurfaceView.updateSurface()      ← 创建 SurfaceControl
      → surfaceCreated()               ← 回调 Java
  → 窗口动画播放（Window Transition）
    → ViewRootImpl 重新布局
    → SurfaceView.updateSurface()      ← 销毁旧 Surface，创建新 Surface
      → surfaceDestroyed()             ← 回调 Java
      → surfaceCreated()               ← 正常情况下应该立即回调
```

**MIUI/HyperOS 的问题：** transition 动画期间 `SurfaceView` 的 Surface 被销毁后，
`ViewRootImpl` 的 `relayoutWindow()` 没有重新创建 `SurfaceControl`，导致 `surfaceCreated`
**永远不会**被调用（或延迟 4 分钟以上）。

这是系统级 Bug，与应用层代码无关。所有尝试的 Java 层 workaround 均无效：

| 尝试的 Workaround | 结果 |
|---|---|
| `visibility = GONE → VISIBLE` | ❌ 无效 |
| `holder.setFixedSize(1, 1)` | ❌ 无效 |
| `removeView(this) + addView(this)` | ❌ 无效（且会清除 postDelayed） |
| `onConfigurationChanged()` 手动调用 | ❌ 无效（不会触发 ViewRootImpl.dispatchConfigurationChanged） |
| `requestLayout() / invalidate()` | ❌ 无效 |
| `window.attributes = window.attributes` | ❌ 无效 |

### 2.2 应用层根因 1：Vulkan 销毁崩溃

最初的代码在 `surfaceDestroyed` 中调用 `nativeSetSurface(null)`，直接销毁 Vulkan 上下文：

```rust
// 问题代码：VulkanContext::drop
fn drop(&mut self) {
    // ... 其他销毁 ...
    self.device.destroy_device(None);      ← Device 先被销毁
    // ... 但 self.context (Skia DirectContext) 还没 drop！
}
```

Skia 的 `GrDirectContext` 析构时会调用 `vkDestroyCommandPool`，此时 Device 已无效，
Adreno 驱动空指针解引用（`fault addr 0x5b0`），导致 **SIGSEGV 崩溃**。

### 2.3 应用层根因 2：IME 尺寸不同步

`SurfaceView.surfaceChanged` 在 MIUI 上 IME 弹出/收起时不可靠（有时不调用）。
原始代码只在 `surfaceChanged` 中调用 `nativeOnSizeChanged()`，导致 IME 收起后 swapchain
尺寸未更新，渲染黑屏。

---

## 3. 修复方案

### 3.1 禁用窗口动画（核心修复）

**文件：** `termux-app/src/main/java/com/termux/app/TermuxActivity.java`

```java
public void onCreate(Bundle savedInstanceState) {
    super.onCreate(savedInstanceState);
    getWindow().setWindowAnimations(0);  // ← 禁用 transition 动画
    ...
}
```

**原理：** 阻止 MIUI/HyperOS 在窗口 transition 期间创建临时 Surface 并错误销毁，
从而避免 `surfaceDestroyed` 在 Activity 启动时被调用。

**效果：** 禁用后 Activity 启动时 `surfaceCreated` → 正常渲染，`surfaceDestroyed` 不再出现。

### 3.2 Vulkan 上下文保持存活（防御性修复）

**文件：** `terminal-emulator/src/main/rust/src/jni_bindings.rs`

```rust
// surfaceDestroyed 时：停止渲染线程，但不销毁 Vulkan 上下文
if surface.as_raw().is_null() {
    render_thread::get_surface_ready().store(false, Ordering::SeqCst);
    render_thread::get_render_thread_running().store(false, Ordering::SeqCst);
    render_thread::request_render();
    if let Some(handle) = render_thread::get_render_thread_handle().lock().unwrap().take() {
        let _ = handle.join();
    }
    // 【关键】不销毁 Vulkan 上下文，保持存活等待 surfaceCreated
    return;
}

// surfaceCreated 时：复用已有上下文，只更新 Surface
if let Some(mutex) = render_thread::get_vulkan_context().get() {
    let mut guard = mutex.lock().unwrap();
    if guard.is_some() {
        if let Some(ctx) = guard.as_mut() {
            if ctx.update_surface(window as _) {  // ← 只更新 Surface，不重建 Instance/Device
                render_thread::get_surface_ready().store(true, Ordering::SeqCst);
                render_thread::try_start_render_thread();
                return;
            }
        }
    }
}
```

**文件：** `terminal-emulator/src/main/rust/src/vulkan_context.rs`

新增 `update_surface()` 方法：

```rust
pub unsafe fn update_surface(&mut self, window: *mut c_void) -> bool {
    let _ = self.device.device_wait_idle();
    // 销毁旧 swapchain
    for surface in self.sk_surfaces.drain(..) { drop(surface); }
    if self.swapchain != ash_vk::SwapchainKHR::null() {
        self.swapchain_loader.destroy_swapchain(self.swapchain, None);
    }
    // 销毁旧 Surface，创建新 Surface
    self.surface_loader.destroy_surface(self.surface, None);
    let android_surface_loader = ash::khr::android_surface::Instance::new(&self.entry, &self.instance);
    self.surface = android_surface_loader.create_android_surface(...)?;
    // 重建 swapchain
    self.recreate_swapchain(self.extent.width, self.extent.height)
}
```

**同时修复 Vulkan 销毁崩溃：**

```rust
impl Drop for VulkanContext {
    fn drop(&mut self) {
        // 关键修复：先让 Skia 放弃所有 GPU 资源
        if let Some(mut ctx) = self.context.take() {
            ctx.release_resources_and_abandon();
            std::mem::forget(ctx);  // ← 阻止 C++ 析构在无效 Device 上操作
        }
        let _ = self.device.device_wait_idle();
        // ... 其他销毁 ...
        self.device.destroy_device(None);
    }
}
```

### 3.3 IME 尺寸同步（兜底修复）

**文件：** `terminal-view/src/main/java/com/termux/view/TerminalView.kt`

```kotlin
override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
    updateSize()
    // SurfaceView 在 IME 弹出/收起等场景下，surfaceChanged 可能不会被调用或延迟调用，
    // 因此必须在 onSizeChanged 中直接通知，确保 swapchain 尺寸始终与 View 尺寸同步。
    if (w > 0 && h > 0) {
        try { nativeOnSizeChanged(w, h) }
        catch (e: Exception) { ... }
    }
}
```

---

## 4. 验证结果

### 4.1 Activity 启动

```
surfaceCreated → VulkanContext::new → SUCCESS
spawn_render_thread: Starting Vulkan render thread
Vulkan: Swapchain recreated 1220x2506 with 5 images
```

✅ **无 `surfaceDestroyed`，终端正常显示**

### 4.2 IME 弹出

```
nativeOnSizeChanged: 1220x1204
surfaceChanged: 1220x1204
Vulkan: Swapchain recreated 1220x1204 with 5 images
```

✅ **Swapchain 正确重建为 IME 弹出后的高度**

### 4.3 IME 收起

```
nativeOnSizeChanged: 1220x2506
surfaceChanged: 1220x2506
Vulkan: Swapchain recreated 1220x2506 with 5 images
```

✅ **Swapchain 正确恢复为全屏高度，无黑屏**

---

## 5. 踩坑记录

### 坑 1：removeView() 会清除 postDelayed

```kotlin
// ❌ 错误：使用 View.postDelayed
postDelayed({ ... }, 200)
removeView(this)  // 这会触发 onDetachedFromWindow，清除所有 postDelayed！

// ✅ 正确：使用独立 Handler
private val mSurfaceRecreateHandler = Handler(Looper.getMainLooper())
mSurfaceRecreateHandler.postDelayed({ ... }, 200)
removeView(this)  // 不会清除独立 Handler 的任务
```

### 坑 2：TextureView 不可行

尝试将 `SurfaceView` 改为 `TextureView`，但发现：
- `TextureView.onDraw()` 是 **final**
- `TextureView.draw()` 也是 **final**

无法重写绘制方法，Sixel 图像和文本选择无法叠加绘制。TextureView 方案不可行。

### 坑 3：手动 onConfigurationChanged 无效

```kotlin
// ❌ 无效：不会触发 ViewRootImpl.dispatchConfigurationChanged
onConfigurationChanged(resources.configuration)

// ✅ 有效：只有系统真正调用 Activity.onConfigurationChanged 时才会触发
// 但手动触发系统级配置变化（如旋转屏幕）影响用户体验
```

### 坑 4：ANativeWindow 生命周期

```rust
// ❌ 错误：Surface 销毁后继续访问 ANativeWindow
// Surface 被系统销毁后，ANativeWindow 可能仍然有效一段时间，
// 但 Vulkan 操作最终会失败（VK_ERROR_SURFACE_LOST_KHR）

// ✅ 正确：surfaceDestroyed 时立即停止渲染，等待 surfaceCreated 重新创建
```

---

## 6. 后续优化建议

1. **考虑持久化 Pipeline Cache**：已部分实现（`vulkan_context.rs` 中的 `save_pipeline_cache_data`），但首次启动仍需 ~1s 初始化 Vulkan
2. **SurfaceView 重建兜底**：保留 `mSurfaceRecreateHandler` 的 fallback 逻辑，即使未来系统更新修复了 Bug 也不会影响功能
3. **监控 Surface 状态**：在 `onResume()` 中检查 `holder.surface?.isValid`，如果无效可尝试 `requestLayout()`

---

## 7. 相关文件变更

| 文件 | 变更 |
|---|---|
| `termux-app/src/main/java/com/termux/app/TermuxActivity.java` | `onCreate()` 中添加 `getWindow().setWindowAnimations(0)` |
| `terminal-view/src/main/java/com/termux/view/TerminalView.kt` | `onSizeChanged()` 中添加 `nativeOnSizeChanged()`；`surfaceDestroyed` 中保留 workaround |
| `terminal-emulator/src/main/rust/src/jni_bindings.rs` | `nativeSetSurface(null)` 不销毁 Vulkan 上下文；`nativeSetSurface(surface)` 支持 `update_surface` |
| `terminal-emulator/src/main/rust/src/vulkan_context.rs` | 新增 `update_surface()`；`Drop` 中先 `release_resources_and_abandon()` 再 `std::mem::forget` |
| `terminal-emulator/src/main/rust/src/vulkan_context.rs` | `VulkanContext` struct 新增 `entry` 字段 |
