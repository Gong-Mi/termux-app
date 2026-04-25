# 📜 Termux-Rust-New: `pty.rs` 执行逻辑技术文档

## 1. 核心挑战：Android 12+ 执行限制 (W^X)
从 Android 10 (API 29) 开始，Google 严禁应用执行其私有数据目录（`/data/data/com.termux/`）下的二进制文件。
- **限制本质**：内核级的 `noexec` 挂载。
- **后果**：直接调用 `execve("/data/data/com.termux/files/usr/bin/ls", ...)` 会报 `EACCES (Permission denied)`。

## 2. 绕过架构：Linker 包装与 LD_PRELOAD 劫持

为了击穿上述限制，`pty.rs` 采用了两级跳的绕过方案：

### 第一级：Linker 包装器 (Linker Wrapper)
通过系统动态链接器（Linker）手动加载目标程序，使其被视为“库加载”而非“直接执行”。
- **操作**：将执行命令替换为 `/system/bin/linker64`。
- **参数序位**：`[argv[0], 目标程序绝对路径, ...原始参数]`。
- **示例**：`bash` -> `/system/bin/linker64 /data/data/com.termux/files/usr/bin/bash`。

### 第二级：LD_PRELOAD 劫持 (The termux-exec bypass)
Linker 包装器只能解决第一个进程的启动问题。为了让第一个进程（如 `bash`）启动的所有子进程（如 `ls`, `apt`）也能自动绕过限制，必须注入 `libtermux-exec.so`。
- **工作原理**：该库劫持了 C 库中的 `execve` 函数，自动为所有子进程加上 Linker 包装。
- **注入方式**：在 `execve` 的环境变量数组中强制写入 `LD_PRELOAD`。

## 3. 核心实现细节

### A. 路径规范化 (Crucial Path Normalization)
- **规则**：所有环境变量和执行路径必须规范化为 `/data/data/` 形式。
- **原因**：Android 系统虽然内部使用 `/data/user/0/`，但系统的 Linker 和某些核心库对 `/data/data/` 有特殊的硬编码豁免或兼容逻辑。

### B. 环境接管 (Controlled Environment)
放弃继承父进程环境，使用 `execve` 显式传递：
1. **基础变量**：`PATH`, `PREFIX`, `HOME`, `TMPDIR`, `TERM`, `LANG` 等。
2. **强制注入**：`LD_PRELOAD` 指向发现的最优绕过库（如 `libtermux-exec-linker-ld-preload.so`）。
3. **变量屏蔽**：剔除 `LD_LIBRARY_PATH` 等可能导致 Linker 进入“限制模式”的干扰变量。

### C. 启动兼容性支持
- **Shebang 处理**：识别脚本首行的 `#!`，并自动将 `/usr/bin/` 或 `/bin/` 下的路径重定向到 Termux 内部路径。
- **Login Shell 支持**：确保 `argv[0]` 保留原始值（如 `-bash`），以便 shell 识别启动模式。

## 4. 健壮性保障
- **信号清理**：在子进程中清空信号掩码（`sigprocmask`），确保子进程可控。
- **句柄清理**：遍历并关闭 `/proc/self/fd` 下多余的文件描述符，防止继承泄露。
- **Phantom Killer 流控**：监控当前 UID 进程数，接近 Android 12 阈值（~32）时主动限流。

## 5. 调试工具箱 (Logcat 标记)

| 标签 | 关键日志 | 状态检查 |
| :--- | :--- | :--- |
| `[PTY]` | `INJECTING LD_PRELOAD=...` | 确认劫持库路径是否正确 |
| `[PTY]` | `ENV: ...` | 检查 `LD_PRELOAD` 和 `PATH` 是否被规范化为 `/data/data/` |
| `[PTY] ` | `FINAL_EXECUTE: cmd=...` | 确认最终执行的是否为 `/system/bin/linker64` |
| `[PTY]` | `execve FAILED! errno=13` | 表示 Linker 包装失败或库文件不可读 |

## 6. 维护红线
1. **统一路径**：严禁在环境变量中混合使用 `/data/user/0` 和 `/data/data`。
2. **底层检查**：严禁使用 Rust 高层 API（如 `exists()`）检查权限，必须使用 `libc::access`。
3. **环境传递**：修改环境变量必须在 `final_env` 数组中处理，严禁依赖子进程的自继承。
