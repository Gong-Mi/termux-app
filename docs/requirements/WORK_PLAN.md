# 工作计划

> 分支: `feature/rust-integration`  
> 基线: `Gong-Mi/termux-app` 私有分支  
> 日期: 2026-05-01

---

## 当前基线

| 指标 | 状态 |
|------|------|
| minSdk | 26 |
| targetSdk | 30 |
| compileSdk | 36 |
| NDK | r29 |
| Rust 引擎 | 已有 (117 个 .rs 文件) |
| 构建系统 | Groovy DSL + 自定义 `com.termux.rust` 插件 |
| Kotlin | 有 (18 .kt 文件) |
| 测试 | Rust 240+ 单元测试通过 |
| CI | 有 (debug_build.yml, rust-ci.yml) |

---

## 核心目标

### 1. 多用户适配
- [ ] 动态路径系统（消灭 `/data/data/com.termux` 硬编码）
- [ ] 每个 Android 用户独立 PREFIX / HOME
- [ ] Bootstrap 按用户隔离安装
- [ ] 用户切换检测与日志增强

### 2. 多窗口插件整合
- [ ] 同一 APK 内多 Activity/悬浮窗并存
- [ ] Termux:API 功能内置化（相机、传感器等）
- [ ] Termux:Float 合并为悬浮窗 Service
- [ ] 窗口间状态共享（同一进程）

### 3. LLM CLI 体验优化
- [ ] 端到端渲染延迟测量与优化
- [ ] 键盘事件零丢包（Ctrl+Space, Alt+数字等）
- [ ] Sixel 图像渲染闭环
- [ ] 文本选择区稳定性

### 4. 构建系统改进
- [ ] 消除 `afterEvaluate` 泛滥
- [ ] 版本号集中管理
- [ ] Rust 插件多模块命名空间
- [ ] 16KB 页面对齐验证

---

## 近期优先级

| 优先级 | 任务 | 说明 |
|--------|------|------|
| P0 | **动态用户路径** | 85 处硬编码路径需改为 `Context.getFilesDir()` |
| P0 | **修复 CI 失败测试** | `extended_features::test_sixel_extended_parsing` 断言错误 |
| P1 | **多窗口框架验证** | 快速验证 Freeform/悬浮窗概念 |
| P1 | **键盘组合补全** | 添加 LLM CLI 特定按键测试 |
| P2 | **构建系统清理** | 清理过程文档和调试脚本 |
| P2 | **版本号独立** | 脱离官方 0.118.x 体系 |

---

## 关键决策

### 基线选择
- **本分支** (`feature/rust-integration`): 已有 Rust 引擎，构建系统需清理
- **Google Play 分支** (`work/googleplay-base`): 构建现代，无 Rust，需从零引入

**建议**: 继续在本分支开发，逐步吸收 Play Store 版本的构建改进（Kotlin DSL、无 sharedUserId）。

### 子窗口方案
- 手机: `WindowManager.addView()` 悬浮窗
- 平板/DeX: Freeform Activity
- 插件整合: API/Float/Styling 内置，Widget/Tasker 保留独立

---

## 验证标准

- [ ] `./gradlew :terminal-emulator:test` 全绿
- [ ] `./gradlew :terminal-emulator:buildAllRust` 成功
- [ ] `cargo test --lib` 240+ 测试通过
- [ ] 多用户路径测试（工作资料/第二用户）
- [ ] 悬浮窗与主终端同时显示
