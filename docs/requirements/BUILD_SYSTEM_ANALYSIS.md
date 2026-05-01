# 构建系统问题分析

> 背景: 多次重构后发现，构建脚本是当前项目的第一大技术债务。  
> 本文件分析现有 Gradle/Cargo 构建链的具体问题，并提出插件化重构方向。

---

## 一、构建系统现状全景

```
构建入口
    ├── settings.gradle (include 4 modules)
    ├── build.gradle (全局依赖: AGP 8.4.2, Kotlin 1.9.24)
    │
    ├── app/build.gradle (266 行)
    │   ├── Android 应用配置
    │   ├── com.termux.rust 插件 (rust-bootstrap)
    │   ├── Bootstrap 下载函数 + task (内联 50+ 行)
    │   ├── 签名配置 (硬编码密码)
    │   ├── afterEvaluate: task 依赖注入 (3 组)
    │   └── 版本号: 0.118.0
    │
    ├── terminal-emulator/build.gradle (93 行)
    │   ├── com.termux.rust 插件 (主 Rust 引擎)
    │   ├── afterEvaluate: publishing 配置
    │   └── 版本号: 0.118.0
    │
    ├── terminal-view/build.gradle (60 行)
    │   ├── afterEvaluate: publishing 配置
    │   └── 版本号: 0.118.0
    │
    ├── termux-shared/build.gradle (~100 行)
    │   ├── com.termux.rust 插件 (rust-shared)
    │   ├── task matching 注入 JniLibFolders 依赖
    │   ├── afterEvaluate: publishing 配置
    │   └── 版本号: 0.118.0
    │
    └── buildSrc/src/main/groovy/.../RustAndroidPlugin.groovy (105 行)
        ├── 自定义 Gradle 插件
        ├── 为每个 ABI 生成 cargoNdkBuild* + copyRust* task
        ├── buildAllRust 聚合 task
        └── afterEvaluate: hook preBuild + mergeJniLibFolders
```

---

## 二、具体问题清单

### 问题 1: 版本号四处硬编码 🔴

**现状:**
```
app/build.gradle:                versionCode 118, versionName "0.118.0"
terminal-emulator/build.gradle:  version = '0.118.0'
terminal-view/build.gradle:      version = '0.118.0'
termux-shared/build.gradle:      version = '0.118.0'
```

**后果:**
- 改版本号要改 4 个文件
- 模块版本与应用版本可能不一致（现在是一致的，但靠人工保证）
- 无法支持自动化的 version bump / CI 发版

---

### 问题 2: afterEvaluate 泛滥 🔴

**统计:** 4 个模块共有 4 个 `afterEvaluate` 块。

**具体表现:**

**app/build.gradle (L246-265):**
```groovy
afterEvaluate {
    android.applicationVariants.all { variant ->
        variant.javaCompileProvider.get().dependsOn(downloadBootstraps)
        tasks.matching { it.name.contains("externalNativeBuild") }
              .configureEach { it.dependsOn(downloadBootstraps) }
        tasks.matching { it.name.startsWith("cargoNdkBuild") }
              .configureEach { it.dependsOn(downloadBootstraps) }
    }
    tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }
          .configureEach {
              it.dependsOn(":terminal-emulator:buildAllRust")
              it.dependsOn("buildAllRust")
          }
}
```

**termux-shared/build.gradle (L65-67):**
```groovy
tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }
      .configureEach {
          it.dependsOn("buildAllRust")
      }
```

**RustAndroidPlugin.groovy (L81-85):**
```groovy
project.tasks.configureEach { task ->
    if (task.name == "preBuild" || (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders"))) {
        task.dependsOn buildAllRust
    }
}
```

**问题:**
- **三重注入**: `termux-shared` 同时被插件内部和模块自己的 build.gradle 注入依赖
- **跨模块耦合**: `app` 模块的 `afterEvaluate` 直接引用 `:terminal-emulator:buildAllRust`
- **构建图不可预测**: `afterEvaluate` 的执行顺序取决于 Gradle 配置阶段的行为，调试构建问题时极其痛苦
- **增量构建失效风险**: 错误的 task 依赖可能导致每次构建都触发全量 Rust 编译

---

### 问题 3: Rust 插件无法优雅支持多模块 🔴

**现状:**
3 个模块使用 `com.termux.rust` 插件：
- `app`: `libtermux_bootstrap.so`
- `terminal-emulator`: `libtermux_rust.so`
- `termux-shared`: `libtermux_shared.so`

**问题:**
- 插件生成的 task 名是全局的（`buildAllRust`, `cargoNdkBuildArm64-v8a`）
- 如果两个模块同时构建，task 列表里会有多个同名或类似名的 task，容易混淆
- 插件没有处理多 Rust crate 的 workspace 依赖（`terminal-emulator/src/main/rust` 和 `standalone/Cargo.toml` 是分离的）

---

### 问题 4: Bootstrap 下载逻辑耦合在 app/build.gradle 🟡

**现状:**
`app/build.gradle` 内联定义了：
- `downloadBootstrap(String arch, String expectedChecksum, String version)` 函数（~40 行）
- `downloadBootstraps` task（~20 行）
- 4 个架构的 SHA256 checksum 硬编码
- 远程 URL 硬编码

**问题:**
- build.gradle 变成业务逻辑文件
- checksum 更新需要改构建脚本
- 无法单元测试下载逻辑
- 无法复用（如果其他模块也需要下载外部资源）

---

### 问题 5: Skia 构建黑魔法 🟡

**CI 环境变量:**
```yaml
FORCE_SKIA_BUILD=1
SKIA_BINDINGS_SKIP_LAYOUT_ASSERTIONS=1
FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true
SKIA_NATIVE_API_LEVEL=35
SKIA_USE_VULKAN=1
SKIA_USE_GL=0
CXXFLAGS="-DSK_TYPEFACE_FACTORY_FREETYPE"
```

**问题:**
- 这些变量没有集中文档说明含义和必要性
- `FORCE_SKIA_BUILD=1` 意味着每次 CI 都重新编译 Skia（极其耗时）
- `SKIP_LAYOUT_ASSERTIONS` 暗示 Skia 绑定有已知 bug，通过跳过断言掩盖
- 环境变量散落在 3 个 CI workflow 文件中

---

### 问题 6: Publishing 配置重复 🟡

3 个 library 模块有几乎一样的：
```groovy
afterEvaluate {
    publishing {
        publications {
            release(MavenPublication) {
                from components.release
                groupId = 'com.termux'
                artifactId = 'xxx'
                version = '0.118.0'
                artifact(sourceJar)
            }
        }
    }
}
```

**问题:**
- 复制粘贴代码
- 版本号分散
- 如果Jitpack配置变更，需要改3个文件

---

### 问题 7: 构建脚本无插件扩展点 🟡

**现状:**
所有构建逻辑都是"写死的"。如果你想：
- 添加一个新的 Rust crate（比如未来的 GPU 计算模块）
- 替换 Bootstrap 下载源
- 切换 Skia 为纯软件渲染
- 添加新的 ABI（如 riscv64）

都需要直接修改 `buildSrc` 或 `build.gradle`。

**目标应该是:** 构建系统有明确的扩展点，新功能通过"配置"而非"改代码"加入。

---

## 三、重构方向: 构建脚本插件化

### 目标架构

```
buildSrc/ (或独立插件项目)
├── src/main/kotlin/com/termux/build/
│   ├── TermuxBasePlugin.kt          # 基础约定: 版本号、路径规范
│   ├── TermuxRustPlugin.kt          # Rust NDK 构建（替代现有 Groovy 插件）
│   ├── TermuxBootstrapPlugin.kt     # Bootstrap 下载（从 app/build.gradle 提取）
│   ├── TermuxPublishingPlugin.kt    # Jitpack/Maven 发布（统一配置）
│   └── TermuxSkiaPlugin.kt          # Skia 构建配置（集中环境变量管理）
│
gradle/
├── termux.versions.toml             # 版本号集中管理 (替代四处硬编码)

app/build.gradle.kts
├── plugins { termux-base, termux-rust, termux-bootstrap }
├── 无 afterEvaluate
├── 无内联下载函数
└── 版本号引用: termux.versionName

terminal-emulator/build.gradle.kts
├── plugins { termux-base, termux-rust, termux-publishing }
└── 无 afterEvaluate
```

### 具体改造点

#### 1. 版本号集中管理

创建 `gradle/termux.versions.toml` 或 `buildSrc/src/main/kotlin/TermuxVersion.kt`:
```kotlin
object TermuxVersion {
    const val NAME = "0.119.0-mu"
    const val CODE = 119
    const val BOOTSTRAP = "2026.03.01-r1+apt.android-7"
}
```

所有模块引用此单点。

#### 2. 提取 Bootstrap 下载为独立插件

```kotlin
// TermuxBootstrapPlugin.kt
class TermuxBootstrapPlugin : Plugin<Project> {
    override fun apply(project: Project) {
        val extension = project.extensions.create("termuxBootstrap", TermuxBootstrapExtension::class.java)
        
        project.tasks.register<DownloadBootstrapTask>("downloadBootstraps") {
            variant.set(extension.variant)
            architectures.set(extension.architectures)
            checksums.set(extension.checksums)
            outputDir.set(project.layout.buildDirectory.dir("bootstrap"))
        }
    }
}
```

app/build.gradle 变成：
```kotlin
termuxBootstrap {
    variant = "apt-android-7"
    architectures = listOf("aarch64", "arm", "i686", "x86_64")
    checksums = mapOf(
        "aarch64" to "dd2040ad9ba1445eaf0818f3305bf190e8bdd04bcc0019faf0279181c48e71e3",
        // ...
    )
}
```

#### 3. 消除 afterEvaluate

**现有逻辑:**
- Rust 编译需要在 Android 插件配置完成后才能确定 ABI 列表
- Bootstrap 下载需要在 variant 确定后挂载

**解决方案:**
- 使用 Gradle 的 `PluginManager.withPlugin("com.android.application")` 延迟注册，而非 `afterEvaluate`
- 使用 Android Gradle Plugin 的 `androidComponents.onVariants` API（AGP 7.0+）
- 明确声明 task 依赖图，而非运行时注入

示例:
```kotlin
// 替代 afterEvaluate 注入
project.plugins.withType<AppPlugin> {
    project.extensions.getByType<ApplicationAndroidComponentsExtension>()
        .onVariants { variant ->
            val downloadTask = project.tasks.named("downloadBootstraps")
            variant.sources.java?.addGeneratedSourceDirectory(downloadTask) { /* ... */ }
        }
}
```

#### 4. Rust 插件支持多模块命名空间

```kotlin
// 生成带模块前缀的 task 名
val modulePrefix = project.name // "app", "terminal-emulator", "termux-shared"
val cargoTaskName = "${modulePrefix}CargoNdkBuild${abi.capitalize()}"
val copyTaskName = "${modulePrefix}CopyRust${abi.capitalize()}"
val buildAllTaskName = "${modulePrefix}BuildAllRust"
```

避免全局 task 名冲突。

#### 5. Skia 配置集中化

创建 `skia.properties` 或插件扩展：
```kotlin
termuxSkia {
    forceBuild = true
    skipLayoutAssertions = true
    nativeApiLevel = 35
    useVulkan = true
    useGl = false
    extraCxxFlags = listOf("-DSK_TYPEFACE_FACTORY_FREETYPE")
}
```

CI workflow 只需简单设置，或完全通过配置文件管理。

---

## 四、优先级建议

构建系统重构虽然重要，但**不应阻塞功能开发**。建议按以下顺序：

| 阶段 | 改造内容 | 工作量 | 阻塞性 |
|------|---------|--------|--------|
| 1 | 版本号集中管理 | 2 小时 | 否 |
| 2 | 消除 app/build.gradle 的 afterEvaluate | 4 小时 | 高（影响每次构建稳定性） |
| 3 | Rust 插件多模块命名空间 | 4 小时 | 中 |
| 4 | Bootstrap 提取为独立插件/task | 1 天 | 低 |
| 5 | 全量迁移到 build.gradle.kts | 2 天 | 低 |
| 6 | Skia 配置集中化 | 4 小时 | 低 |

**最紧迫的是阶段 1 和 2:**
- 版本号集中管理是零风险、高收益
- `afterEvaluate` 消除能立即减少构建的不可预测性

---

## 五、验证标准

重构后的构建系统应满足：

1. `./gradlew clean assembleDebug` 成功且输出正确
2. 修改版本号只需改 1 个文件
3. 新增一个 Rust 模块只需在 build.gradle 中加 3 行配置
4. 没有 `afterEvaluate` 块（或仅剩 1 个在基础插件内）
5. CI 构建时间不比现在慢
6. 增量构建可靠（修改 Java 代码不触发 Rust 重编译，修改 Rust 代码正确触发）
