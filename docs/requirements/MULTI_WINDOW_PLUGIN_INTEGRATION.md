# 多窗口插件整合方案分析

> 目标: 将官方插件（Termux:API、Termux:Float 等）整合进主应用，以"同一应用多窗口"形式呈现，支持后台同时显示多个界面并方便切换。

---

## 一、需求澄清

### 你要的"子窗口"究竟是什么

```
官方现状（多个独立 APK）:
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Termux 主界面  │     │  Termux:API     │     │  Termux:Float   │
│  (com.termux)   │     │ (com.termux.api)│     │(com.termux.float)│
│                 │     │  [相机预览]      │     │  [悬浮终端]      │
│  $ vim file    │     │                 │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        ↑ 通过 sharedUserId / LocalSocket 通信 ↑

目标形态（单一 APK，多窗口）:
┌─────────────────────────────────────────────────────────────────┐
│                        Termux (单一 APK)                         │
│                                                                  │
│  ┌──────────────────────────────┐  ┌──────────────────────────┐ │
│  │      终端主窗口               │  │     插件子窗口            │ │
│  │                              │  │  [文件浏览器/API面板/...]  │ │
│  │  $ apt search ...            │  │                          │ │
│  │                              │  │                          │ │
│  └──────────────────────────────┘  └──────────────────────────┘ │
│                                                                  │
│  或叠加形态:                                                      │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  $ vim config.py                                          │   │
│  │                                                           │   │
│  │         ┌─────────────┐                                   │   │
│  │         │ 快捷键面板   │  ← 悬浮在终端上方                  │   │
│  │         └─────────────┘                                   │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

核心特征：
- **单一 APK**（不需要 sharedUserId）
- **同一进程**（不需要 IPC）
- **多窗口并存**（终端 + 插件功能同时可见）
- **后台存活**（切换到别的应用后，Termux 的多个窗口仍能在后台显示/快速唤回）

---

## 二、Android 多窗口技术路径

Android 支持同一应用多窗口的几种方式：

| 技术 | 版本要求 | 窗口形态 | 适用场景 |
|------|---------|---------|---------|
| **Freeform Activity** | API 24+ (7.0) | 自由调整大小的矩形窗口 | 平板/DeX/大屏手机的多窗口并排 |
| **Picture-in-Picture** | API 26+ (8.0) | 右下角小窗（视频比例） | 不适合通用插件（有尺寸和比例限制） |
| **悬浮窗 (SYSTEM_ALERT_WINDOW)** | API 1+ | 任意位置任意大小 | 适合小工具面板（已有此权限） |
| **Multi-Display** | API 29+ (10) | 跨物理屏幕 | 外接显示器场景 |
| **Activity Embedding** | API 32+ (12L) | 主 Activity 内嵌子 Activity | 需要大屏，手机不支持 |

对于你的目标（手机/平板上同时显示终端和插件），最实际的是 **Freeform** 或 **悬浮窗**。

---

## 三、各插件整合方案

### 3.1 Termux:API → 子窗口/页面整合

**现状**: Termux:API 是被脚本通过 `termux-camera-photo`、`termux-sensor` 等命令调用的。它本身没有常驻 UI，只在被调用时弹出一个快速执行的 Activity。

**整合方式**:
```
脚本调用: termux-camera-photo
    ↓
不再跳转外部 APK，而是：
方案 A: 直接调用主应用内的 Service + 内部 Camera API
方案 B: 弹出一个内部 Dialog/Activity 做相机预览和拍照
```

**多窗口化**: API 调用本身不需要多窗口（它是瞬时的）。但如果你想要一个"传感器实时监控面板"常驻显示，可以做成一个可拖拽的悬浮窗。

**权限**: `CAMERA`、`ACCESS_FINE_LOCATION`、`RECORD_AUDIO` 等全部集中到主应用。不上架，无所谓。

---

### 3.2 Termux:Float → 悬浮窗模式

**现状**: Termux:Float 就是一个悬浮在其他应用之上的终端窗口。

**整合方式**: 最简单的整合对象。
- 在主应用内新增一个 Service + `WindowManager.addView()`
- 复用现有的 `TerminalView` + Rust 引擎
- 通过 `SYSTEM_ALERT_WINDOW` 权限显示全局悬浮窗

**多窗口化**: 本身就是多窗口。用户可以在主 Termux 界面外，再开 1-N 个悬浮终端窗口。

---

### 3.3 Termux:Styling → BottomSheet / Dialog

**现状**: 选择配色方案和字体的配置应用。

**整合方式**: 最简单。
- 改为 `BottomSheetDialogFragment` 或 `DialogFragment`
- 从主应用的设置菜单呼出
- 不需要多窗口，因为它只是配置面板

---

### 3.4 Termux:Widget → 应用内快捷方式面板

**现状**: 桌面小部件，在 Android 主屏幕上显示快捷脚本列表。

**整合难点**:
- `AppWidgetProvider` 必须是独立应用组件，无法内嵌
- 但可以做一个**功能等价**的内部面板：
  - 主应用内一个可滑出的侧边栏/悬浮按钮面板
  - 显示常用脚本快捷方式
  - 点击后在当前终端执行

**多窗口化**: 不需要独立窗口，做成侧滑面板或悬浮快捷按钮即可。

---

### 3.5 Termux:Boot → 后台 Service

**现状**: 开机启动后台脚本。

**整合方式**: 纯后台逻辑，无 UI。
- 把 `BOOT_COMPLETED` receiver 合并到主应用
- 启动 `TermuxService` 执行用户配置的启动脚本

**多窗口化**: 无关。

---

### 3.6 Termux:Tasker → 保留独立或改为广播

**现状**: Tasker 通过特定 Intent 调用 Termux 执行脚本。

**整合难点**:
- Tasker 的插件机制要求插件是独立 APK，通过 `com.twofortyfouram.locale.intent.action.EDIT_SETTING` 等 action 交互
- 如果合并到主应用，Tasker 可能识别不到插件

**方案**:
- 方案 A: 保留 Tasker 插件为独立轻量 APK（只转发 Intent 到主应用）
- 方案 B: 主应用暴露 `RunCommandService`，Tasker 直接通过 `am startservice` 或广播调用

---

## 四、核心实现: 同一应用多 Activity 多窗口

### 4.1 基础配置

**AndroidManifest.xml**:
```xml
<!-- 主终端 Activity -->
<activity
    android:name=".app.TermuxActivity"
    android:launchMode="standard"
    android:resizeableActivity="true"
    ... />

<!-- 插件子窗口 Activity -->
<activity
    android:name=".plugin.PluginWindowActivity"
    android:launchMode="standard"
    android:resizeableActivity="true"
    android:documentLaunchMode="always"
    android:excludeFromRecents="false" />

<!-- 悬浮窗 Service -->
<service
    android:name=".plugin.FloatWindowService"
    android:foregroundServiceType="specialUse" />
```

**注意 `launchMode="standard"`**:
- 必须设为标准模式，这样每次 `startActivity` 都会创建新实例
- 不能用 `singleTask` 或 `singleInstance`（会阻止多实例）

### 4.2 启动子窗口

```kotlin
// 从终端启动一个插件子窗口
fun launchPluginWindow(context: Context, pluginType: String) {
    val intent = Intent(context, PluginWindowActivity::class.java).apply {
        putExtra("plugin_type", pluginType)
        flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_MULTIPLE_TASK
    }
    
    if (isFreeformSupported()) {
        // 自由窗口模式（平板/DeX/大屏）
        val options = ActivityOptions.makeBasic().apply {
            setLaunchBounds(Rect(100, 100, 900, 600))
        }
        context.startActivity(intent, options.toBundle())
    } else {
        // 手机：以分屏或悬浮窗方式启动
        context.startActivity(intent)
    }
}
```

### 4.3 悬浮窗实现（手机最实用）

```kotlin
class FloatWindowService : Service() {
    override fun onCreate() {
        val windowManager = getSystemService(Context.WINDOW_SERVICE) as WindowManager
        
        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
            WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
            PixelFormat.TRANSLUCENT
        )
        
        val floatView = LayoutInflater.from(this).inflate(R.layout.float_terminal, null)
        // floatView 内嵌一个 TerminalView
        
        windowManager.addView(floatView, params)
    }
}
```

### 4.4 窗口间通信

由于所有窗口在同一进程，不需要 IPC：

```kotlin
// 单例 SessionCoordinator（Rust 层已有）
// Java/Kotlin 层用 EventBus / SharedViewModel / 本地广播

class PluginWindowActivity : AppCompatActivity() {
    // 直接访问主应用的 TermuxService
    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as TermuxService.LocalBinder
            // 直接操作 binder.service.sessions
        }
    }
}
```

---

## 五、问题与风险

### 5.1 内存压力 🔴

每个子窗口/悬浮窗都包含一个 `TerminalView` → 都绑定一个 Rust 引擎实例 → 都占用 GPU/内存资源。

```
主终端: 1 个 Rust Engine + Skia Context
悬浮窗 1: 另 1 个 Rust Engine + Skia Context
悬浮窗 2: 再 1 个 Rust Engine + Skia Context
```

如果开 3 个以上窗口，低端设备可能 OOM。

**缓解**:
- 悬浮窗使用轻量级渲染（简化版 TerminalView，不需要全功能）
- 多个窗口共享同一个 Rust `SessionCoordinator`，只是视图分离
- 限制同时打开的悬浮窗数量（如最多 3 个）

### 5.2 生命周期管理 🟡

```
用户场景:
1. 打开主 Termux，运行编译任务
2. 打开悬浮窗，运行 htop 监控
3. 切换到微信聊天
4. 系统内存不足，杀掉 Termux 进程
5. 结果: 主终端和悬浮窗同时死亡，编译中断
```

独立 APK 的优势是进程隔离。合并后所有窗口共享同一进程，系统一次性全部回收。

**缓解**:
- 前台 Service 保活（已有 `TermuxService`）
- 悬浮窗本身也绑定前台 Service
- 但 Android 12+ 的 phantom process killer 仍然可能下手

### 5.3 返回键和焦点管理 🟡

```
用户按返回键:
- 如果焦点在悬浮窗上: 悬浮窗应该关闭还是最小化？
- 如果焦点在主终端上: 正常返回行为是什么？
- 系统手势返回 ( predictive back ) 在哪个窗口响应？
```

需要为每个窗口类型定义明确的焦点策略。

### 5.4 配置变更（旋转/分屏）🟡

多窗口环境下，任意一个窗口的旋转/resize 都可能触发所有 Activity 的 `onConfigurationChanged`。需要确保 Rust 渲染器能正确处理多实例并发 resize。

---

## 六、实施建议

### 第一阶段: 验证概念（1-2 天）

只做最简单的验证：
1. 在主应用内新增一个 `PluginTestActivity`
2. 设置 `launchMode="standard"`, `resizeableActivity="true"`
3. 从 `TermuxActivity` 启动它，观察是否能同时显示两个窗口
4. 在平板上测试 Freeform，在手机上测试分屏

### 第二阶段: 整合 Termux:Float（1 周）

把悬浮终端做进主应用：
1. 提取 Termux:Float 的核心逻辑（WindowManager 悬浮窗 + TerminalView）
2. 在主应用内作为 `FloatWindowService` 实现
3. 菜单项: "新建悬浮终端"

### 第三阶段: 整合 Termux:API（1-2 周）

把 API 脚本调用从"跳转外部 APK"改为"内部 Service 调用"：
1. 将 Termux:API 的 Java 代码作为 `termux-api` module 合并
2. 脚本命令 `termux-camera-photo` 直接调用主应用内的 Camera API
3. 如需 UI（如相机预览），弹出内部 Dialog/Activity

### 第四阶段: 整合其他插件（视需求）

- Styling → 设置面板（简单）
- Widget → 应用内快捷面板（中等）
- Boot → 后台 receiver（简单）
- Tasker → 保留独立或改为广播（困难）

---

## 七、与构建系统问题的关系

你把"构建脚本的插件"和"子窗口"分开提了，说明这是**两个独立的大问题**。

**建议处理顺序**:
1. **先搞构建系统**（地基）
   - 构建不稳定，功能开发会被频繁打断
   - 把插件代码合并进主应用，首先需要构建系统能优雅地处理多 module
2. **再搞多窗口插件整合**（上层建筑）
   - 需要先把插件代码作为 library module 合并
   - 然后再做 Activity/Service 的多窗口封装

或者如果你急着想看多窗口效果，可以**并行**：
- 主分支继续做构建重构
- 开一个新分支快速验证 Freeform 多窗口概念

---

## 八、关键决策

### 决策 1: 悬浮窗 vs Freeform

| 场景 | 推荐方案 |
|------|---------|
| 手机小屏 | 悬浮窗（可拖拽的小面板） |
| 平板/折叠屏 | Freeform（真正多窗口并排） |
| 外接显示器/DeX | Freeform + Multi-Display |

**建议**: 两种都做，根据设备能力自动选择。

### 决策 2: 是否保留插件独立包

| 插件 | 建议 |
|------|------|
| API | 合并（功能内嵌） |
| Float | 合并（本身就是窗口功能） |
| Styling | 合并（配置面板） |
| Boot | 合并（纯后台） |
| Widget | 功能替代（内部快捷面板） |
| Tasker | 暂留独立（Tasker 协议限制） |

---

## 九、验证标准

整合完成后应满足：
1. 安装单一 APK，不需要任何外部插件
2. 从菜单可以打开悬浮终端，与主终端同时显示
3. `termux-camera-photo` 命令在主应用内完成拍照，不跳转外部应用
4. 后台切换后，前台 Service 保持所有窗口状态
5. 构建系统仍能正常编译（验证构建重构没有破坏）
