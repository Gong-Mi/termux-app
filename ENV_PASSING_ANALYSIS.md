# Termux 环境变量传递与继承链路分析

## 1. 完整传递链

```
[AndroidShellEnvironment] ──► [TermuxShellEnvironment] ──► [TermuxSession.execute]
         │                            │                         │
         │                            │                         ▼
    设置基础变量                  覆盖/添加 Termux            HashMap ──► String[]
    (HOME=/, PATH=system,         专用变量                     │
     TMPDIR=/data/local/tmp)                                    ▼
                              ┌──────────────────────────────────────┐
                              │   TerminalSession.initializeEmulator │
                              │      └── JNI.createSessionAsync()    │
                              │            (Kotlin → JNI)            │
                              └──────────────────────────────────────┘
                                              │
                                              ▼
                              ┌──────────────────────────────────────┐
                              │   Rust jni_bindings.rs               │
                              │      └── pty::create_subprocess_with_data
                              │            (JString[] → Vec<String>) │
                              └──────────────────────────────────────┘
                                              │
                                              ▼
                              ┌──────────────────────────────────────┐
                              │   子进程 (fork)                       │
                              │      clearenv() + putenv()           │
                              │      └── execvp/execv                │
                              └──────────────────────────────────────┘
```

## 2. 各层职责

### 2.1 AndroidShellEnvironment (基类)
设置 Android 系统级默认值：
- `HOME=/`
- `PATH=<System.getenv(PATH)>`
- `TMPDIR=/data/local/tmp`
- `TERM=xterm-256color`
- `COLORTERM=truecolor`
- `LANG=en_US.UTF-8`
- Android 系统变量 (`ANDROID_ROOT`, `BOOTCLASSPATH`, ...)

### 2.2 TermuxShellEnvironment (修复后)
在基类之上覆盖/设置 Termux 专用值：
- `HOME=/data/data/com.termux/files/home`  ← **覆盖 Android 默认的 `/`**
- `PREFIX=/data/data/com.termux/files/usr`
- `TMPDIR=/data/data/com.termux/files/usr/tmp`  ← **覆盖 Android 默认的 `/data/local/tmp`**
- `PATH`:
  - Android 5/6: `$PREFIX/bin:$PREFIX/bin/applets`
  - Android 7+: `$PREFIX/bin`
- `LD_LIBRARY_PATH`:
  - Android 5/6: `$PREFIX/lib`
  - Android 7+: **显式移除** (依赖 DT_RUNPATH)

> **修复前的问题**: 这一层被注释掉了，说"由 Rust 默认值处理"。但 Rust 只检查"变量是否存在"，而基类已经设了 `HOME=/` 和 `PATH`，所以 Rust 认为已存在，**不会覆盖成 Termux 值**。导致子进程拿到错误的 `HOME=/` 和系统 `PATH`。

### 2.3 TermuxSession.execute (Java)
- 调用 `setupShellCommandEnvironment()` 获取完整 HashMap
- 合并 `additionalEnvironment` (用户额外变量，可覆盖已有值)
- `Collections.sort()` 后转成 `String[]`
- 写入日志，便于调试

### 2.4 TerminalSession / JNI (Kotlin → C)
- `TerminalSession.initializeEmulator()` 调用 `JNI.createSessionAsync()`
- 将 `String[]` 作为 `jobjectArray` 传入 JNI
- JNI 层不做任何修改，直接透传

### 2.5 Rust pty.rs (修复后)
接收 `Vec<String>`，做**防御性 fallback**：

| 变量 | 策略 | 说明 |
|------|------|------|
| `PREFIX` | 缺失则添加 | 安全网 |
| `HOME` | 值为 `/` 或 `/data/local/tmp` 时覆盖 | 拦截 Android 默认值漏网 |
| `PATH` | 缺失或不包含 `$PREFIX/bin` 时前置 | 保证 Termux 命令优先 |
| `TMPDIR` | 值为 `/data/local/tmp` 时覆盖 | 拦截 Android 默认值漏网 |
| `LD_LIBRARY_PATH` | 缺失则添加 `$PREFIX/lib` | 安全网 |
| `LD_PRELOAD` | 缺失且文件存在时添加 | 条件加载，避免新鲜安装报错 |
| `TERM/COLORTERM/LANG` | 缺失则添加 | 安全网 |

> **修复前的问题**: PATH 缺失时直接 push `$PREFIX/bin`（不含系统路径）；LD_PRELOAD 无条件加载（文件不存在时会报错）。

### 2.6 子进程 (fork/exec)
```rust
libc::clearenv();           // 完全清空继承的环境
for env in final_env {
    libc::putenv(env);      // 逐个重建
}
libc::execvp(cmd, args);    // 执行目标程序
```

子进程**不会继承**父进程（Java/Rust）的任何环境变量，完全由上面链路构建的 `final_env` 决定。

## 3. 关键修复对比

### 3.1 Java 层: TermuxShellEnvironment.java

**修复前** (bug):
```java
// Core Termux variables are now handled by Rust defaults if missing.
// We only provide them here if they differ from the defaults or for backward compatibility.
return environment;
```

**修复后**:
```java
environment.put(ENV_HOME, TermuxConstants.TERMUX_HOME_DIR_PATH);
environment.put(ENV_PREFIX, TermuxConstants.TERMUX_PREFIX_DIR_PATH);
if (!isFailSafe) {
    environment.put(ENV_TMPDIR, TermuxConstants.TERMUX_TMP_PREFIX_DIR_PATH);
    if (TermuxBootstrap.isAppPackageVariantAPTAndroid5()) {
        environment.put(ENV_PATH, TermuxConstants.TERMUX_BIN_PREFIX_DIR_PATH + ":" + TermuxConstants.TERMUX_BIN_PREFIX_DIR_PATH + "/applets");
        environment.put(ENV_LD_LIBRARY_PATH, TermuxConstants.TERMUX_LIB_PREFIX_DIR_PATH);
    } else {
        environment.put(ENV_PATH, TermuxConstants.TERMUX_BIN_PREFIX_DIR_PATH);
        environment.remove(ENV_LD_LIBRARY_PATH);
    }
}
return environment;
```

### 3.2 Rust 层: pty.rs

**修复前** (bug):
```rust
if !has_home { final_env.push("HOME=/data/data/com.termux/files/home".to_string()); }
if !has_path { final_env.push("PATH=/data/data/com.termux/files/usr/bin".to_string()); }
if !has_ld_preload { final_env.push("LD_PRELOAD=/data/data/.../libtermux-exec.so".to_string()); }
```

**修复后**:
```rust
// HOME: 覆盖 Android 默认值
if let Some(pos) = find_env(&final_env, "HOME=") {
    let val = final_env[pos].split('=').nth(1).unwrap_or("");
    if val == "/" || val == "/data/local/tmp" || val.is_empty() {
        final_env[pos] = format!("HOME={}", termux_home);
    }
} else { final_env.push(format!("HOME={}", termux_home)); }

// PATH: 前置 Termux bin，而不是替换
if let Some(pos) = find_env(&final_env, "PATH=") {
    let val = final_env[pos].split('=').nth(1).unwrap_or("");
    if !val.contains(&termux_bin) {
        final_env[pos] = format!("PATH={}:{}", termux_bin, val);
    }
} else { final_env.push(format!("PATH={}:/system/bin:/system/xbin", termux_bin)); }

// LD_PRELOAD: 条件加载
if find_env(&final_env, "LD_PRELOAD=").is_none() {
    if std::path::Path::new(&termux_exec_path).exists() {
        final_env.push(format!("LD_PRELOAD={}", termux_exec_path));
    }
}
```

## 4. 继承关系总结

| 层级 | 能否被下层覆盖 | 说明 |
|------|---------------|------|
| AndroidShellEnvironment | ✅ 是 | 基类默认值，被 TermuxShellEnvironment 覆盖 |
| TermuxShellEnvironment | ✅ 是 | Termux 专用值，被 additionalEnvironment 覆盖 |
| additionalEnvironment | ✅ 是 | 用户传入，被 Rust fallback 逻辑部分覆盖（值错误时） |
| Rust fallback | ❌ 否 | 最终安全网，但仅修正"明显错误"的值 |
| clearenv + putenv | - | 完全重建，不继承任何父进程环境 |

## 5. 修复验证

- [x] Rust 侧 `cargo check` 通过
- [x] Java 侧改动仅为 HashMap put/remove，语法无风险
- [x] 与 `termux-app-rust` 参考实现行为对齐
