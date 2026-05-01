# 基线确认

> 工作区: `/data/user/0/com.termux/files/home/termux-app-rust`  
> 分支: `work/googleplay-base`  
> 追踪: `googleplay/main` (termux-play-store/termux-apps)  
> 日期: 2026-05-01

---

## 为什么选 Google Play 版本作为基线

| 对比维度 | mainline 官方 | Google Play (本基线) |
|---------|-------------|-------------------|
| **sharedUserId** | 有 | **无** |
| **targetSdk** | 28 | **37** |
| **构建脚本** | Groovy DSL | **Kotlin DSL** |
| **AGP** | 8.4.2 | **9.1.1** |
| **Gradle** | ~8.x | **9.4.1** |
| **Config Cache** | 无 | **已启用** |
| **Rust 引擎** | 无 | 无 |
| **多用户** | 无 | 无 |
| **多窗口** | 无 | 无 |

Play Store 版本已解决 **sharedUserId** 和 **构建现代化**，但缺少 Rust/多用户/多窗口。正是理想的基线。

---

## 当前代码状态

```
分支: work/googleplay-base
提交: 9ca18a6f (Bump gradle/actions from 5 to 6)
代码: 136 Java + 2 Kotlin
构建: Kotlin DSL, 6 模块
Native: NDK (jni/Android.mk), 无 Rust
```

### 模块结构

| 模块 | 类型 | 说明 |
|------|------|------|
| `termux-app` | application | 主应用, 60 Java 文件, versionCode 140 |
| `terminal-emulator` | library | 终端模拟器, 13 Java 文件, NDK |
| `terminal-view` | library | 终端视图, 6 Java 文件 |
| `termux-api` | application | API 插件, 独立 APK, versionCode 51 |
| `termux-style` | application | 样式插件, versionCode 35, 自动下载字体 |
| `termux-tasker` | application | Tasker 插件, versionCode 7 |

### 已清理残留

删除了旧分支带来的:
- `external_dependencies/` (rust-skia)
- `r.txt.bak`
- `terminal-emulator/src/main/rust/`

---

## Play Store 已解决 vs 仍需自己做的

**已解决:**
- [x] 无 sharedUserId (signature 权限通信)
- [x] targetSdk 37
- [x] Kotlin DSL + AGP 9.1 + Gradle 9.4
- [x] 前台服务 SPECIAL_USE
- [x] 预测性返回手势
- [x] Configuration Cache
- [x] lint warningsAsErrors

**仍需自己做:**
- [ ] minSdk 30 → 28 (如需 Android 9)
- [ ] 引入 Rust 引擎 (替换 NDK)
- [ ] 多用户路径动态化
- [ ] 多窗口/子窗口
- [ ] 16KB 对齐验证
- [ ] LLM CLI 优化
