# Termux-Rust 引擎硬重置与部署指南

## 1. 为什么需要硬重置？
由于我们从 Java 模拟器切换到了 **Rust 高性能引擎**，且 Android 11+ (Target SDK 30) 引入了极其严格的 **W^X (Write or Execute)** 安全策略，旧的 `$PREFIX` 环境或旧版 APK 的残留配置会导致：
*   **PTY 闪退**：内核禁止在数据目录直接执行二进制文件。
*   **dpkg 权限错误**：子进程丢失 `LD_PRELOAD` 劫持。
*   **Vulkan 挂起**：旧版 JNI 逻辑无法正确拉起 Rust 渲染线程。

## 2. 彻底清理（手机端操作）
在安装新版前，请务必执行以下清理步骤：
1.  **卸载旧版应用**：在系统设置中找到 Termux，选择“卸载”。
2.  **清理残留数据**（如果手机已 Root）：
    ```bash
    su -c "rm -rf /data/data/com.termux"
    su -c "rm -rf /data/user/0/com.termux"
    ```
3.  **确认设备状态**：确保手机的“开发者选项”中已开启“USB 调试”或“无线调试”，以便后续通过 ADB 验证。

## 3. 获取最新修复版本
**严禁使用本地缓存的旧 APK**。请从 GitHub Actions 获取包含以下 Commit 修复的最新产物：
*   **分支**：`feature/rust-integration`
*   **核心修复说明**：
    *   `11ed887d`: 智能 Shebang 解析（解决 `not an ELF` 错误）。
    *   `346350ef`: `LD_PRELOAD` 环境变量注入（解决 `dpkg` 权限错误）。
    *   `cd926903`: `targetSdkVersion 30` 下的 Linker 劫持（解决 W^X 限制）。
    *   `dee8edec`: Vulkan 渲染线程唤醒（解决黑屏）。

**下载步骤**：
1. 访问 GitHub 项目的 **Actions** 页面。
2. 点击最近一次成功的 **"Fast Build"**。
3. 在页面底部的 **Artifacts** 区域下载 `termux-app_...-arm64-v8a.zip`。

## 4. 安装与初始化
1.  **通过 ADB 安装**：
    ```bash
    adb install -r <下载好的APK文件名>.apk
    ```
2.  **首次启动**：
    *   启动 Termux。
    *   由于是硬重置，系统会重新下载 Bootstrap 基础包（约 50MB）。
    *   **注意**：如果启动时依然闪退，请立刻查看 Logcat。

## 5. 验证修复状态 (关键)
启动后，请在本地或通过 ADB 执行以下命令，确认我们的“三重劫持”逻辑是否生效：

### A. 验证渲染线程
检查日志中是否包含：
`Termux-Rust: VulkanContext::new: SUCCESS`
`Termux-Rust: try_start_render_thread: Both conditions met. STARTING LOOP.`
*如果没有第二句，Vulkan 依然会黑屏。*

### B. 验证进程启动 (W^X 绕过)
观察执行 `/usr/bin/login` 时的日志：
`Termux-Rust: [PTY_EXEC] Attempting to exec: /system/bin/linker64`
*如果执行路径是原路径而非 linker64，说明劫持失败，会报 Permission denied。*

### C. 验证 dpkg/子进程 (LD_PRELOAD)
在终端输入 `env | grep LD_PRELOAD`，确保输出包含：
`LD_PRELOAD=/data/data/com.termux/files/usr/lib/libtermux-exec.so`

## 6. 常见问题排除
*   **依然提示 Permission denied？**
    运行 `termux-fix-shebang /data/data/com.termux/files/usr/bin/*`。这会修正由于 Bootstrap 解压导致的脚本头错误。
*   **Vulkan 画面卡死？**
    确认设备是否支持 Vulkan 1.1+。如果硬件不支持，可在 `termux.properties` 中暂时切回 `renderer = skia-canvas`。
