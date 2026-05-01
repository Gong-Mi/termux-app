# Termux 分支需求规格说明书

> 分支: `feature/target-api-35`  
> 目标: 适配 Android 多用户环境 + 现代 LLM CLI 终端体验  
> 日期: 2026-05-01  

---

## 一、项目背景与核心约束

### 1.1 为什么要做这个分支

官方 Termux 存在以下无法通过简单 PR 解决的结构性问题：

| 问题 | 影响 |
|------|------|
| **单用户架构** | 无法在 Android 多用户/工作资料中独立运行 |
| **性能天花板** | Java 终端引擎在流式 True Color 输出时 CPU 占用高、帧率低 |
| **渲染老旧** | 基于 Canvas 的文本渲染不支持现代终端图像协议 (Sixel inline) |
| **键盘延迟** | Java 层键盘事件处理链路长，复杂 modifier 组合易丢包 |

### 1.2 已确认资产

- **Rust 引擎**: 240+ 单元测试通过，VTE 解析/屏幕缓冲/颜色管理/键盘处理已完整 Rust 化
- **自定义 bootstrap**: 来源 `github.com/Gong-Mi/termux-packages`，自主可控
- **Android 15 适配**: targetSdk 35, compileSdk 36, minSdk 28

### 1.3 核心约束

- 必须保持与官方 Termux 插件生态的 `sharedUserId` 兼容（或明确放弃兼容）
- Rust 引擎 JNI 桥接已稳定，死锁问题已通过事件队列机制解决
- 构建链依赖 Skia + Vulkan，短期内无法剥离

---

## 二、需求总览

```
需求分类
├── P0 基础架构（不做则其他全部崩塌）
│   ├── REQ-001 动态用户路径
│   ├── REQ-002 路径常量去硬编码
│   └── REQ-003 多用户 Bootstrap 隔离
│
├── P1 多用户运行时（核心差异化）
│   ├── REQ-101 多用户独立 PREFIX
│   ├── REQ-102 多用户独立 HOME
│   ├── REQ-103 多用户进程隔离
│   ├── REQ-104 插件多用户兼容
│   └── REQ-105 用户切换检测
│
├── P2 LLM CLI 体验（性能与美化）
│   ├── REQ-201 端到端渲染延迟 < 16ms
│   ├── REQ-202 True Color 零开销直通
│   ├── REQ-203 Sixel 图像内联渲染
│   ├── REQ-204 键盘事件零丢包
│   ├── REQ-205 滚动物理与惯性
│   └── REQ-206 文本选择管线
│
├── P3 可维护性（长期生存）
│   ├── REQ-301 清理过程文档与调试脚本
│   ├── REQ-302 版本号独立体系
│   ├── REQ-303 CI 稳定化
│   └── REQ-304 废弃权限清理
│
└── P4 扩展性（未来预留）
    ├── REQ-401 自定义包管理器接口
    └── REQ-402 主题/配色动态切换
```

---

## 三、P0 基础架构需求

### REQ-001: 动态用户路径解析

**现状:**
`TermuxConstants.java` 硬编码:
```java
TERMUX_INTERNAL_PRIVATE_APP_DATA_DIR_PATH = "/data/data/" + TERMUX_PACKAGE_NAME;
// 实际即 "/data/data/com.termux"
```
在 Android 多用户下，用户 ID 10 的真实路径应为 `/data/user/10/com.termux`。

**问题:**
- 85 处代码直接引用这些硬编码常量
- `TermuxInstaller` 检查路径时使用了 `replaceAll("^/data/user/0/", "/data/user/0/")` 这种无意义的替换（本身就是 user 0）
- Rust 层通过 JNI 收到的是 Java 层传来的路径，如果 Java 层传错，Rust 层也会错

**目标:**
应用启动时通过 `Context.getFilesDir().getAbsolutePath()` 动态解析真实路径，而非编译期常量。

**实现思路:**
1. 将 `TermuxConstants` 中的路径常量改为运行时初始化
2. 在 `TermuxApplication.onCreate()` 中执行一次路径探测，写入全局单例
3. 提供 `TermuxPaths.getPrefixDir()` 等运行时方法替代静态常量
4. Rust 层通过 JNI 接收动态路径，替代硬编码字符串

**优先级:** P0-1 (最紧急)

---

### REQ-002: 路径常量去硬编码

**现状:**
 grep 统计有 **85 处**直接引用 `TERMUX_*_DIR_PATH` 静态常量。

**目标:**
将所有硬编码路径引用替换为运行时路径获取。

**实现思路:**
1. 创建 `TermuxPaths` 运行时路径管理类（替代静态常量）
2. 逐模块迁移:
   - `termux-shared`: 基础路径（PREFIX, HOME, TMP）
   - `app`: Installer, FileReceiver, DocumentsProvider
   - `terminal-emulator`: Rust JNI 路径注入
3. 保留常量作为 fallback（仅用于无法获取 Context 的场景）

**优先级:** P0-2

---

### REQ-003: 多用户 Bootstrap 隔离

**现状:**
Bootstrap 安装逻辑在 `TermuxInstaller.java` 中，目标路径为 `TERMUX_PREFIX_DIR_PATH`。

**问题:**
- 用户 A 和用户 B 如果共享同一 PREFIX 路径，会产生包数据库冲突
- 不同用户可能需要不同的 bootstrap 版本或包集合

**目标:**
每个 Android 用户拥有独立的 bootstrap 安装。

**实现思路:**
1. 将 PREFIX 从 `/data/user/<N>/com.termux/files/usr` 确认正确
2. bootstrap zip 的校验和检查按用户隔离（或不做缓存共享）
3. 安装流程检查 `PREFIX/bin` 存在性时，使用动态路径
4. 卸载/重置功能只影响当前用户的数据

**优先级:** P0-3

---

## 四、P1 多用户运行时需求

### REQ-101: 多用户独立 PREFIX

**目标:**
每个 Android 用户的 Termux 拥有完全独立的 `/usr` 环境。

**验证标准:**
- 用户 0 安装 `git`，用户 10 看不到
- 用户 10 运行 `apt update` 不影响用户 0 的包数据库

**依赖:** REQ-001, REQ-002

**优先级:** P1-1

---

### REQ-102: 多用户独立 HOME

**目标:**
每个用户的 `$HOME` 独立，默认配置（`.bashrc`, `.termux/` 等）互不影响。

**验证标准:**
- `echo $HOME` 输出 `/data/user/<N>/com.termux/files/home`
- 不同用户的 shell 历史、SSH 密钥、配置文件隔离

**依赖:** REQ-001

**优先级:** P1-2

---

### REQ-103: 多用户进程隔离

**目标:**
各用户的 PTY/session 进程完全隔离，一个用户的 `killall` 不会杀掉另一个用户的进程。

**现状分析:**
Rust `coordinator.rs` 已实现会话协调器，管理 session 状态和互锁。

**实现思路:**
1. 确认 `fork()` 后的子进程继承正确的 UID/GID（Android 自动处理）
2. SessionCoordinator 的锁不需要跨用户（因为不同用户的进程空间本就隔离）
3. 本地 socket (`termux-am`) 按用户隔离命名，或确认 Android 已自动隔离

**优先级:** P1-3

---

### REQ-104: 插件多用户兼容

**目标:**
Termux:API、Termux:Widget 等插件在多用户环境下正确工作。

**问题:**
`sharedUserId` 在多用户下是**根本性问题**。Android 的 `sharedUserId` 在同一用户的应用间共享 UID，但不同用户的应用本就隔离。问题在于：
- 如果插件和主应用不在同一用户下安装，sharedUserId 无法生效
- 插件尝试通过 `getContextForPackage("com.termux")` 获取主应用 Context 时可能失败

**实现思路:**
方案 A: 放弃 sharedUserId，改用 Binder IPC（工作量极大）  
方案 B: 明确声明 "每个用户需独立安装主应用+全套插件"  
方案 C: 为插件提供降级模式（无法获取主应用 Context 时， graceful failure）

**推荐:** 先实现方案 C（短期），长期评估方案 B。

**优先级:** P1-4

---

### REQ-105: 用户切换检测

**目标:**
应用能检测当前运行环境是否发生了用户切换（如从主用户切换到工作资料）。

**实现思路:**
1. 在 `TermuxApplication.onCreate()` 缓存当前 `UserHandle`
2. 提供 `UserUtils.getCurrentUserSerial()` 工具方法
3. 日志和崩溃报告中带上用户 ID，便于多用户环境调试

**优先级:** P1-5

---

## 五、P2 LLM CLI 体验需求

### REQ-201: 端到端渲染延迟 < 16ms

**现状:**
Rust 引擎处理速度声称 10x 提升，但**端到端延迟**（PTY 字节 → 屏幕像素）未测量。

**目标:**
在 80x24 终端中，流式输出 1000 行/秒时，肉眼无感知卡顿。

**测量方法:**
1. Rust 层在 `process_bytes()` 入口和 `renderer.submit_frame()` 出口打时间戳
2. 统计 P99 延迟
3. 瓶颈可能在：Java UI 线程、Surface 提交、Skia 字体缓存

**优化方向:**
- 如果瓶颈在 Java UI：让 Rust 直接渲染到 Surface（绕过 Java Canvas）
- 如果瓶颈在 Skia：字体缓存预热、glyph atlas

**优先级:** P2-1

---

### REQ-202: True Color 零开销直通

**现状:**
`terminal::colors.rs` 已支持 256 色 + True Color，OSC 4/10/11/12 序列已解析。

**目标:**
`printf '\e[38;2;255;100;50mHello\e[0m\n'` 颜色完全准确，无性能衰减。

**验证:**
已有 7 个 colors 单元测试通过，需补充集成测试验证端到端渲染。

**优先级:** P2-2

---

### REQ-203: Sixel 图像内联渲染

**现状:**
- Rust `terminal::sixel.rs` 已能解析 Sixel 数据流
- `extended_features` 测试中有 Sixel 测试（但有一个断言错误）
- 渲染管线未确认是否能把 Sixel 像素贴到最终帧

**目标:**
在终端中直接显示 Sixel 图像（如 `img2sixel` 输出）。

**实现思路:**
1. Sixel 解码器输出 RGBA buffer
2. 在 Rust `renderer.rs` 中把图像 buffer 作为 glyph atlas 的一部分上传
3. 终端行中预留图像区域，使用 Skia `drawImageRect` 绘制
4. 处理图像与文本的混合（z-order）

**优先级:** P2-3

---

### REQ-204: 键盘事件零丢包

**现状:**
`terminal::key_handler.rs` 已 Rust 化，支持多种 modifier 组合。

**目标:**
以下按键组合在 LLM CLI 中可用且无丢包：
- `Ctrl+Space` (aider accept)
- `Ctrl+Shift+Enter` (多行提交)
- `Alt+数字` (某些 TUI 标签切换)
- `Ctrl+[` (Vim Esc 替代)

**验证:**
`key_event_handling.rs` 集成测试已有 19 个用例通过，需补充上述特定组合。

**优先级:** P2-4

---

### REQ-205: 滚动物理与惯性

**现状:**
已实现 Logarithmic Damping + Pixel Space 滚动（`SCROLLING_PHYSICS.md`）。

**目标:**
万行历史缓冲下，快速滑动仍保持 60fps。

**验证:**
- 性能测试 `performance.rs` 通过
- 实际设备上 `adb logcat -s TerminalView-Scroll:D` 观察阻尼值

**优先级:** P2-5 (已实现，维持现状)

---

### REQ-206: 文本选择管线

**现状:**
Rust `screen.rs` 已支持 selection API，Java 层有 `TextSelectionCursorController`。

**目标:**
在流式输出时仍能正确选择文本，选择区域不因新内容滚动而漂移。

**问题:**
选择区域目前基于行/列坐标，如果终端 resize 或内容滚动，需要 anchor 跟踪。

**优先级:** P2-6

---

## 六、P3 可维护性需求

### REQ-301: 清理过程文档与调试脚本

**现状:**
- 根目录有 8+ 个 AI 生成的过程性 `.md` 文件
- `tests/` 目录有 70+ 个文件，其中大量是调试/探索脚本

**目标:**
- 过程文档移至 `docs/archive/` 或删除
- 调试脚本（`lnm_problems.rs`, `resize_history_bug.rs` 等）移至 `dev-scripts/` 或删除
- 保留有价值的文档（`SCROLLING_PHYSICS.md` 有价值）

**优先级:** P3-1

---

### REQ-302: 版本号独立体系

**现状:**
- `app/build.gradle`: `0.118.0`
- README: 声称 `v0.118.3`
- 分支名: `feature/target-api-35`

**目标:**
建立独立于官方的版本体系，例如 `0.1.0-mu` (Multi-User) 或 `2026.05.0`。

**优先级:** P3-2

---

### REQ-303: CI 稳定化

**现状:**
- Rust CI 工作流存在
- `debug_build.yml` 设置了 `FORCE_SKIA_BUILD=1`
- 测试 `extended_features` 有一个失败用例

**目标:**
- 修复 `test_sixel_extended_parsing` 断言错误
- CI 全绿（Rust 测试 + Android 构建）

**优先级:** P3-3

---

### REQ-304: 废弃权限清理

**现状:**
`AndroidManifest.xml` 申请了大量高危权限，部分标记为 `tools:ignore="ProtectedPermissions"`。

**目标:**
移除无实际用途的权限：
- `READ_LOGS`（除非有调试功能真正使用）
- `DUMP`（同上）
- `WRITE_SECURE_SETTINGS`（同上）
- `PACKAGE_USAGE_STATS`（同上）

**优先级:** P3-4

---

## 七、P4 扩展性需求

### REQ-401: 自定义包管理器接口

**现状:**
`TermuxBootstrap` 已预留 `TAPM`、`PACMAN` enum（被注释掉）。

**目标:**
为 future 的自定义包管理预留清晰的接口，不局限于 apt。

**优先级:** P4-1 (远期)

---

### REQ-402: 主题/配色动态切换

**现状:**
Rust `colors.rs` 支持从 Properties 更新颜色，Java 层有颜色检查逻辑。

**目标:**
支持在运行时切换完整主题（背景、前景、强调色、光标色），无需重启 session。

**优先级:** P4-2 (远期)

---

## 八、需求依赖关系图

```
REQ-001 动态用户路径
    ├─→ REQ-002 路径常量去硬编码
    │       ├─→ REQ-101 独立 PREFIX
    │       ├─→ REQ-102 独立 HOME
    │       └─→ REQ-003 Bootstrap 隔离
    │
    └─→ REQ-103 进程隔离 (自动满足，依赖路径正确)

REQ-105 用户切换检测
    └─→ REQ-104 插件兼容 (降级模式)

REQ-201 渲染延迟
    ├─→ REQ-202 True Color (已实现)
    ├─→ REQ-203 Sixel (依赖渲染管线)
    └─→ REQ-205 滚动 (已实现)

REQ-301 ~ REQ-304 (相互独立，可并行)
```

---

## 九、实施建议（阶段化）

### 第一阶段: 地基（1-2 周）
- [ ] REQ-301: 清理文档和调试脚本
- [ ] REQ-302: 确立版本号体系
- [ ] REQ-001 + REQ-002: 动态路径系统
- [ ] REQ-003: Bootstrap 按用户隔离验证
- [ ] REQ-303: 修复 CI 失败测试

### 第二阶段: 多用户核心（2-3 周）
- [ ] REQ-101: 独立 PREFIX 验证（安装/卸载/升级）
- [ ] REQ-102: 独立 HOME 验证
- [ ] REQ-103: 进程隔离确认
- [ ] REQ-105: 用户检测与日志增强
- [ ] REQ-104: 插件降级模式

### 第三阶段: LLM CLI 优化（持续）
- [ ] REQ-201: 端到端延迟测量与优化
- [ ] REQ-204: 键盘组合补全测试
- [ ] REQ-203: Sixel 图像渲染闭环
- [ ] REQ-206: 选择区稳定性

### 第四阶段: 打磨（持续）
- [ ] REQ-304: 权限清理
- [ ] REQ-401/402: 扩展接口

---

## 十、风险与假设

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| Android `sharedUserId` 在多用户下根本不可用 | 插件生态断裂 | 明确文档化限制，或评估 Binder IPC 改造 |
| Skia/Vulkan 构建链在某设备上崩溃 | 无法安装 | 保留软件渲染 fallback |
| 动态路径改动引入回归 bug | 应用启动失败 | 在 `TermuxApplication.onCreate()` 加断言检查 |
| Bootstrap 二进制 hardcoded `/data/data/com.termux` | 多用户路径不匹配 | 确认 bootstrap 中的二进制是否使用相对路径或环境变量 |

---

## 附录: 关键代码位置速查

| 功能 | 文件 |
|------|------|
| 路径常量定义 | `termux-shared/.../TermuxConstants.java` (L580+) |
| Bootstrap 安装 | `app/.../TermuxInstaller.java` |
| 环境变量注入 | `termux-shared/.../TermuxAppShellEnvironment.java` |
| 用户 ID 获取 | `termux-shared/.../PackageUtils.java` (L598) |
| Rust 路径接收 | `app/.../TermuxApplication.java` (L99) |
| Rust 引擎 | `terminal-emulator/src/main/rust/src/lib.rs` |
| Rust 测试 | `terminal-emulator/src/main/rust/tests/` |
