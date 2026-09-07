# Termux Crash & Code Analysis Report

> **Date:** 2026-05-30
> **Branch:** `work/googleplay-base`
> **HEAD:** `7269380d`
> **Device:** Xiaomi 25098PN5AC (HyperOS, Android 16/SDK 36, arm64-v8a)
> **Analyst:** Kimi Code CLI

---

## 1. Executive Summary

本次分析基于用户反馈的"刚才崩溃"问题，对 `termux-app-googleplay-base` 代码及系统日志进行了全面排查。共发现 **4 类问题**：

| # | 问题 | 层级 | 严重程度 |
|---|---|---|---|
| 1 | `dmesg` 触发 seccomp SIGSYS (syscall 116) | Native | 🔴 高 |
| 2 | MIUI `FloatingActionMode` `ClassCastException` | Java | 🟡 中 |
| 3 | rust-exec "linker relaunch" 对系统命令误触发 | Native | 🟡 中 |
| 4 | `coreutils` 无效选项错误 (`-f`/`-p`) | Shell | 🟡 中 |

---

## 2. Detailed Findings

### 2.1 `dmesg` SIGSYS Crash

**Crash Buffer Evidence:**

```
Fatal signal 31 (SIGSYS), code 1 (SYS_SECCOMP), syscall 116
Cmdline: /system/bin/linker64 /data/user/0/com.termux/files/usr/bin/dmesg
Cause: seccomp prevented call to disallowed arm64 system call 116
#00 klogctl+12
#01 dmesg
#02 __libc_init+124
```

**Root Cause:**
- `dmesg` 调用 `klogctl()` (syscall 116) 读取内核环缓冲区。
- Android 对 `untrusted_app` 域启用了 seccomp-bpf，明确禁止了 `syslog`/`klogctl` 系统调用。
- rust-exec 的 `transform_exec` 将 `dmesg` 包装为通过 `/system/bin/linker64` 启动，但 **seccomp 限制与是否走 linker64 无关**。
- 直接运行 `dmesg` 同样会触发 `SIGSYS`，表现为终端输出 `Unknown signal 31`。

**Status:** 这是 Android 沙箱的正常安全策略，不是 rust-exec 引入的新回归。但用户体验上是"崩溃"（进程被信号终止）。

---

### 2.2 Java `ClassCastException` on Text Selection

**Logcat Evidence:**

```
java.lang.ClassCastException: com.termux.view.TerminalView cannot be cast to android.widget.TextView
    at miuix.toolbar.internal.ActionView.<init>(ActionView.java:20)
    at miuix.toolbar.FloatingActionMode.<init>(FloatingActionMode.java:111)
    at miuix.toolbar.FloatingActionModeHelper.startActionMode(FloatingActionModeHelper.java:24)
```

**Root Cause:**
- MIUI/HyperOS 的文本选择浮动工具栏（`FloatingActionMode`）在构造 `ActionView` 时，**硬编码假设** ActionMode 的宿主是 `android.widget.TextView`。
- Termux 的 `TerminalView` 是自定义 `View`，不是 `TextView`，导致类型转换失败。

**Impact:** 长按终端触发文本选择时，ActionMode 可能弹不出或立即消失。这是一个 **MIUI/HyperOS ROM 级别的兼容性问题**。

---

### 2.3 rust-exec Linker Relaunch Mis-detection

**Logcat Evidence:**

```
execve hook: detected linker relaunch of "/data/user/0/com.termux/files/usr/bin/logcat", redirecting
execve hook: detected linker relaunch of "/system/bin/logcat", redirecting
```

**Code Location:** `terminal-emulator/src/main/rust-exec/src/lib.rs:257-278`

```rust
if path_str.starts_with("/system/") || path_str.starts_with("/vendor/") || path_str.contains("/linker") {
    let is_flag_start = unsafe {
        !argv.is_null() && !(*argv.offset(1)).is_null() && 
        (*(*argv.offset(1)) as u8) == b'-'
    };
    if is_flag_start {
        if let Ok(original_exe) = std::env::var("TERMUX_ORIGINAL_EXE_PATH") {
            // Redirects to transform_exec(original_exe, argv, 0)
        }
    }
}
```

**Root Cause:**
- 该逻辑本意是修复 Node.js 等程序因 `process.execPath` 被污染为 `/system/bin/linker64` 而错误重启自身的问题。
- 但判断条件过于宽泛：**任何**以 `/system/` 开头且 `argv[1]` 以 `-` 开头的 `execve` 都会被拦截。
- `/system/bin/logcat` 的正常调用（如 `logcat -d -s TAG`）完全匹配这个条件，被误识别为 "linker relaunch"，导致额外的、无意义的 `transform_exec` + `execveat` 包装。

**Impact:**
- 增加了一次不必要的进程启动开销。
- 对系统命令的 argv 构造引入额外风险（如 `logcat` wrapper 脚本中的 `unset LD_PRELOAD` 被 `.init_array` hook 重新注入，见 2.4）。

---

### 2.4 `coreutils` Invalid Option Error

**Evidence (`t.txt`):**

```
/data/data/com.termux/files/usr/bin/coreutils: invalid option -- 'f'
/data/data/com.termux/files/usr/bin/coreutils: invalid option -- 'p'
```

**Root Cause Analysis:**
- Termux 中 `id`, `ls`, `cp` 等命令是 `coreutils` 的符号链接。
- `coreutils` 以 multi-call binary 模式工作：通过 `basename(argv[0])` 判断运行哪个 applet。
- 报错路径是 `coreutils` 而非 `id`/`ls`，说明调用者以 `coreutils` 为 `argv[0]` 直接执行了它，并传递了 `-f`/`-p`。
- GNU coreutils 的全局参数中不存在 `-f` 或 `-p`；这些选项属于具体子命令（如 `cp -f`, `ls -p`）。因此 coreutils 报 `invalid option`。

**Relation to rust-exec:**
- `transform_exec` 在处理 ELF 时会**丢弃原始 `argv[0]`**，用 `resolve_exec_path` 返回的绝对路径替代（如 `/data/.../bin/id`）。
- 如果原始调用者直接执行的是 `coreutils -f`，rust-exec 会将其变为 `[linker64, /data/.../bin/coreutils, -f]`，coreutils 仍以 `coreutils` 为 basename 运行，于是报错。
- 这本身更可能是**上游调用者（脚本或别名）的错误用法**，但 rust-exec 的 `argv[0]` 替换策略会固定这种行为，无法通过原始 `argv[0]` 中的 applet 名称来修正。

---

## 3. Code Architecture Context

### 3.1 rust-exec `transform_exec` argv 构造逻辑

对于 PIE ELF (`e_type == 3`)：
```
orig_argv: ["id", "-u"]
new_argv:  ["/system/bin/linker64", "/data/.../bin/id", "-u"]
              ^^^^ dropped orig_argv[0]
```

对于 shebang 脚本（递归解析 interpreter）：
```
orig_argv: ["script.sh", "arg1"]
new_argv:  ["/system/bin/linker64", "/data/.../bin/sh", "/data/.../bin/script.sh", "arg1"]
```

**注意：** `orig_argv[0]` 始终被丢弃。对于依赖 `argv[0]` 进行程序名推断的 multi-call binary，当前逻辑依赖 linker64 传递 `argv[0] = path` 后，程序自行 `basename(path)`。这在绝大多数情况下是正确的，但无法处理调用者故意使用非标准 `argv[0]` 的场景。

### 3.2 `LD_PRELOAD` Persistence Loop

`termux-app-googleplay-base/terminal-emulator/src/main/rust-exec/src/lib.rs:46-60`:

```rust
fn ensure_ld_preload_is_exported() {
    // .init_array hook: unconditionally restores LD_PRELOAD from /proc/self/maps
}
```

- `logcat` wrapper 脚本明确执行 `unset LD_PRELOAD` 以防止系统命令被 termux-exec 再次拦截。
- 但由于 `.init_array` 在每个新进程启动时都会**强制恢复** `LD_PRELOAD`，这个 `unset` 只在当前 shell 进程有效。当 `sh` 执行 `exec /system/bin/logcat` 时，新 `logcat` 进程仍然会加载 `libtermux-exec.so`。
- 这是设计意图（保证子进程始终有 termux-exec），但对系统命令的 wrapper 脚本造成了干扰。

---

## 4. Recommended Actions

### Immediate

1. **Fix linker relaunch detection** (`lib.rs:257-278`)
   - 不应仅凭 `argv[1]` 以 `-` 开头就判定为 relaunch。
   - 正确条件应为：`path_str.contains("linker") && argv[0]` 也是 linker 路径，且 `argv[1]` 以 `-` 开头。
   - 或者：检查 `argv[0]` 是否与 `path_str` 相同（即 linker 调用自身）。

2. **Handle seccomp-sensitive commands gracefully**
   - `dmesg` 等命令的 `SIGSYS` 无法从应用层彻底避免，但可以在 Java 层或 shell 层添加前置检查：
     - 检查 `/proc/sys/kernel/dmesg_restrict` 或尝试 `klogctl(0, NULL, 0)` 探测权限。
     - 或者给用户更友好的错误提示，而不是让进程以信号终止。

### Short-term

3. **Audit `argv[0]` preservation in `transform_exec`**
   - 考虑在 `transform_exec` 中保留原始 `argv[0]` 作为辅助信息，或在 debug log 中记录原始 `argv[0]`，便于排查 multi-call binary 的识别问题。

4. **MIUI `FloatingActionMode` compatibility**
   - 这是一个 ROM bug。如果影响严重，可以考虑在 `TerminalView` 中拦截 `startActionMode`，使用自定义的 `ActionMode.Callback` 或完全禁用系统默认的浮动工具栏，自己实现文本选择菜单。

### Not Required

5. **`dmesg` seccomp crash** — 不需要修改 rust-exec 代码，这是 Android 安全架构的预期行为。

---

## 5. Compilation Notes

- `cargo check --all-targets` 在当前 HEAD (`7269380d`) 上可以通过（SVE2 的 `performance_sve2.rs` 已在之前的提交中删除）。
- 但 `simd/mod.rs`, `pixel.rs`, `cpu_features.rs` **仍未在 `lib.rs` 中注册**，属于死代码。如果未来注册进去，`simd/mod.rs` 中引用的 `sve2` 子模块缺失会导致 aarch64 构建失败。详见 `AUDIT_branch_work_googleplay_base.md` 第 7.2 节。
- `cargo test` 在 Termux 本机环境无法运行（W^X 限制导致 `cc` linker 无法执行），需要通过 `cargo-ndk` 交叉编译或在 CI 环境中测试。

---

*End of report.*
