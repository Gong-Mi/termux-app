# 插件子窗口化方案分析

> 方案: 将 Termux 插件（Termux:API、Termux:Float 等）不再作为独立 APK，而是作为主应用内部的子窗口/子 Activity 运行，类似微信小程序的宿主内嵌模式。

---

## 一、方案理解

### 你想做的

现状是多 APK 通过 sharedUserId 协作，目标是合并为单一 APK，插件以内部子窗口呈现。

技术路径大概率是：
- **Freeform 模式**: 内部 Activity 以自由窗口边界启动
- **悬浮窗叠加**: WindowManager.addView() 叠加在终端上方
- **BottomSheet/Dialog**: 主应用内的模态面板

---

## 二、优势

| 优势 | 说明 |
|------|------|
| 彻底消灭 sharedUserId | 单一 APK，多用户问题迎刃而解 |
| 安装简化 | 用户只需装一个 APK |
| 统一更新 | 主应用更新即全部更新 |
| 数据共享零开销 | 同一进程，不需要 IPC |

---

## 三、问题分析

### 3.1 权限集中爆炸（致命）

Termux 插件各自申请的权限：

| 插件 | 关键权限 |
|------|---------|
| Termux:API | CAMERA, ACCESS_FINE_LOCATION, RECORD_AUDIO, READ_CONTACTS, SEND_SMS, CALL_PHONE |
| Termux:Boot | RECEIVE_BOOT_COMPLETED |
| Termux:Float | SYSTEM_ALERT_WINDOW |
| Termux:Tasker | 需特定 Intent 协议 |
| Termux:Widget | BIND_APPWIDGET（系统级，必须独立应用） |
| Termux:Styling | 无特殊权限 |

合并后 AndroidManifest 权限列表会极其吓人，Google Play 审核大概率拒绝，用户安装时被十几条权限弹窗吓跑。

**缓解:** 运行时权限（requestPermissions）动态申请，但清单里仍然要声明。

---

### 3.2 APK 体积膨胀（严重）

预估额外增量 10-20 MB。虽然 Termux 本身已经 ~180MB，但问题是很多用户从不使用某些插件功能（如相机、定位），却被迫下载包含这些代码的 APK。

---

### 3.3 并非所有插件都适合子窗口化（中高风险）

| 插件 | 可行性 | 问题 |
|------|--------|------|
| Termux:API | 困难 | API 是被脚本调用的，不是用户手动打开的窗口。脚本调用期望 IPC 响应，没有 UI 窗口需求。 |
| Termux:Float | 适合 | 本来就是悬浮窗，最自然 |
| Termux:Widget | 不可能 | Android AppWidget 必须是独立应用的 AppWidgetProvider，无法内嵌 |
| Termux:Boot | 不可能 | 需要 BOOT_COMPLETED 后台生命周期，与窗口无关 |
| Termux:Tasker | 困难 | Tasker 插件机制要求插件是独立 APK，通过特定 Intent Action 被调用 |
| Termux:Styling | 适合 | 只是配置面板 |

**6 个官方插件中，只有 2 个适合子窗口化，2 个不可能，2 个需架构大改。**

---

### 3.4 生命周期耦合（中风险）

独立 APK 的进程隔离优势：插件崩溃不影响主终端，主终端被杀不影响插件后台任务。

子窗口化后全部绑定到主进程，一荣俱荣，一损俱损。用户划掉 Termux 最近任务，所有插件也一起死。

---

### 3.5 焦点与输入冲突（中风险）

Termux 的核心是键盘输入。子窗口弹出时焦点可能被抢走，导致 Ctrl+C 等快捷键被子窗口吃掉而不是传给终端。这对键盘驱动的终端应用是致命体验问题。

---

### 3.6 自由窗口兼容性差（中风险）

Freeform 窗口在 Pixel/三星 DeX 上支持好，但在小米/OPPO/vivo 等国产 ROM 上部分机型阉割或行为不一致。Android TV/平板无自由窗口概念。

---

### 3.7 后台执行受限（中风险）

Android 12+ 对后台启动 Activity 有严格限制。后台触发的插件功能弹出子窗口时可能报错 BackgroundActivityStartException。

---

## 四、折中方案

### 方案 A: 分级插件策略（推荐）

核心内置（随主 APK）：
- Termux:Styling -> 集成到设置面板
- Termux:Float -> 主应用悬浮窗模式

仍然独立（保持插件 APK）：
- Termux:API（权限太杂）
- Termux:Widget（技术上不可能内置）
- Termux:Boot（需要独立后台生命周期）
- Termux:Tasker（需要 Tasker 插件协议）

兼容层：放弃 sharedUserId，改用 Binder AIDL / LocalSocket 通信。

---

### 方案 B: 动态功能模块 (Dynamic Feature Module)

Android App Bundle 支持按需下载模块。但 F-Droid 不支持 AAB，Termux 用户大量侧载，不适合。

---

### 方案 C: 插件降级为纯脚本包

把插件逻辑改为 shell/python 脚本，通过主应用暴露的本地 socket 调用。需要主应用先实现稳定的 RPC 服务。

---

## 五、结论

| 维度 | 评分 |
|------|------|
| 技术可行性 | 部分可行 |
| 用户体验 | 有得有失 |
| 维护成本 | 增加 |
| 多用户问题解决 | 有效 |
| Google Play 可行性 | 极低 |
| F-Droid 可行性 | 勉强 |

**最终建议: 不要全部子窗口化。**

真正的核心问题不是插件怎么显示，而是不同用户的插件怎么独立安装。

Android 多用户模型下，每个用户本来就会独立安装应用（APK 在 /data/app/ 共享只读，数据在 /data/user/N/ 隔离）。动态路径修复（REQ-001）后，插件问题自然就不存在了。

每个 Android 用户各自独立安装 Termux 主应用 + 所需插件，这是系统默认行为，不需要额外架构改造。

---

## 六、如果仍坚持子窗口化

可以帮你：
1. 设计插件抽象层（库 module 形式被主应用依赖）
2. 实现 Freeform 窗口启动器
3. 权限动态申请框架
4. 焦点管理和生命周期隔离

但强烈建议先做 REQ-001（动态路径），验证多用户是否还存在插件问题，再决定是否推翻现有插件架构。
