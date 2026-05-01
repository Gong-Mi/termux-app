# 工作计划

> 分支: `feature/target-api-35`  
> 仓库: `Gong-Mi/termux-app`  
> 日期: 2026-05-01

---

## 当前基线

| 指标 | 状态 |
|------|------|
| minSdk | 28 |
| targetSdk | 35 |
| compileSdk | 36 |
| NDK | r29 |
| Rust 引擎 | 已集成，240+ 单元测试通过 |
| 渲染 | Skia + Vulkan |
| 构建系统 | Groovy DSL + 自定义 `com.termux.rust` 插件 |
| Kotlin | 有 (18 .kt 文件) |
| 死锁 | 已修复（事件队列机制） |

---

## 核心目标

### 1. 多用户适配
- [ ] 动态路径系统（消灭 85 处硬编码 `/data/data/com.termux`）
- [ ] 每个 Android 用户独立 PREFIX / HOME
- [ ] Bootstrap 按用户隔离
- [ ] 插件多用户降级模式

### 2. 多窗口插件整合
- [ ] 同一 APK 内多 Activity/悬浮窗并存
- [ ] Termux:API 功能内置化
- [ ] Termux:Float 合并为悬浮窗 Service
- [ ] 窗口间直接共享状态（无需 IPC）

### 3. LLM CLI 体验优化
- [ ] 端到端渲染延迟 < 16ms
- [ ] 键盘零丢包（Ctrl+Space, Alt+数字等）
- [ ] Sixel 图像内联渲染闭环
- [ ] 万行历史缓冲 60fps 滚动

### 4. 构建系统清理
- [ ] 消除 `afterEvaluate` 泛滥
- [ ] 版本号集中管理（4 处硬编码）
- [ ] Rust 插件多模块命名空间
- [ ] 清理过程文档和调试脚本

---

## 近期任务

| 优先级 | 任务 | 说明 |
|--------|------|------|
| P0 | 动态用户路径 | 从 `Context.getFilesDir()` 获取，替代 `TermuxConstants` 硬编码 |
| P0 | 修复 CI | `extended_features::test_sixel_extended_parsing` 断言错误 |
| P1 | 多窗口概念验证 | 快速验证 Freeform/悬浮窗 |
| P1 | 键盘组合补全 | 添加 LLM CLI 特定按键测试 |
| P2 | 版本号独立 | 脱离官方 0.118.x |
| P2 | 文档清理 | 删除 AI 过程文档，保留有价值的技术规格 |

---

## 关键决策

### 基线选择
- **本分支**: 已有 Rust 引擎 + targetSdk 35，构建需清理
- **Play Store 版本**: 构建现代但无 Rust，需从零引入

**结论**: 继续在本分支开发，逐步吸收 Play Store 构建改进。

### 渲染路径
- 默认: Rust Skia/Vulkan（已存在）
- 保留 Canvas fallback（兼容旧设备）

### 插件整合策略
- 内置: API / Float / Styling / Boot
- 保留独立: Widget（系统限制）/ Tasker（协议限制）

---

## 验证标准

- [ ] `./gradlew :terminal-emulator:buildAllRust` 成功
- [ ] `cargo test --lib` 240+ 通过
- [ ] `./gradlew :terminal-emulator:test` 全绿
- [ ] 多用户路径测试（工作资料/第二用户）
- [ ] 悬浮窗与主终端同时显示
