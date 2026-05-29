# rust-exec linker64 包装导致的进程身份丢失

> 日期：2026-05-29
> 目标：记录 rust-exec 通过 `linker64` 执行 ELF 时，`process.execPath` / `argv[0]` 被替换为 linker 路径的副作用。

## 问题现象

在 `work/googleplay-base` 分支编译的 Termux App 中，运行某些依赖 `process.execPath` 或 `argv[0]` 做 self-relaunch 的程序时会失败。

**具体案例：Google Gemini CLI (`@google/gemini-cli`)**

```bash
$ gemini
error: expected absolute path: "--max-old-space-size=7501"
```

设置 `GEMINI_CLI_NO_RELAUNCH=true` 后可正常进入主逻辑：

```bash
$ GEMINI_CLI_NO_RELAUNCH=true gemini
Please set an Auth method in your .../settings.json or specify ...
```

## 根因分析

### 1. rust-exec 的执行转换

`terminal-emulator/src/main/rust-exec/src/lib.rs` 中，当目标路径是 ELF 时，会将其转换为通过 `linker64` 启动：

```rust
if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
    let mut new_argv = Vec::new();
    new_argv.push(CString::new(linker).unwrap());       // argv[0] = /system/bin/linker64
    new_argv.push(CString::new(path.clone()).unwrap()); // argv[1] = /data/.../usr/bin/node
    let mut i = 1;
    while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
        new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
        i += 1;
    }
    return Some((linker.to_string(), new_argv));
}
```

最终实际执行的是：

```bash
/system/bin/linker64 /data/.../usr/bin/node /path/to/script.js [args...]
```

这是 Android W^X 绕过的必要手段。

### 2. 进程身份丢失

Node.js 的 `process.execPath` 通过读取 `/proc/self/exe` 获取。由于真正 `execve` 的是 `linker64`，子进程看到的自身身份变成：

| 属性 | 正常 Linux/Termux (无 rust-exec) | work/googleplay-base (有 rust-exec) |
|---|---|---|
| `process.execPath` | `/data/.../usr/bin/node` | `/apex/com.android.runtime/bin/linker64` |
| `process.argv[0]` | `node`（或 `/data/.../usr/bin/node`） | `/apex/com.android.runtime/bin/linker64` |
| `/proc/self/exe` | 指向 `node` | 指向 `linker64` |

### 3. Gemini 的 relaunch 逻辑触发失败

Gemini CLI 启动时会自动计算内存参数并 relaunch 自身：

```js
// packages/cli/index.ts → getSpawnConfig
function getSpawnConfig(nodeArgs, scriptArgs) {
  const finalSpawnArgs = [];
  finalSpawnArgs.push(
    ...process.execArgv,
    ...nodeArgs,            // ['--max-old-space-size=7501']
    process.argv[1],        // gemini.js 路径
    ...scriptArgs
  );
  return { spawnArgs: finalSpawnArgs, env: newEnv };
}

// run()
const { spawnArgs, env } = getSpawnConfig(memoryArgs, scriptArgs);
const child = spawn(process.execPath, spawnArgs, { ... });
```

在 rust-exec 环境下，`process.execPath` 是 `linker64`，于是实际执行变成：

```bash
linker64 --max-old-space-size=7501 /data/.../usr/lib/node_modules/.../gemini.js
```

`linker64` 期望第一个参数是要加载的 ELF 文件路径，拿到 `--max-old-space-size=7501` 后报错：

```
error: expected absolute path: "--max-old-space-size=7501"
```

## 影响范围

这不是 gemini 独有的问题。**任何通过 `/proc/self/exe` 或 `argv[0]` 识别自身并做 self-relaunch、auto-update、daemonize 的程序都可能受影响**。

已知高风险场景：

- **Node.js CLI tools**：使用 `process.execPath` 做子进程 spawn 的工具（如 `gemini`, `npm`, `pnpm` 的某些子命令）。
- **Python**：`sys.executable` 同样读取 `/proc/self/exe`，在虚拟环境激活、子进程调用时可能指向 `linker64`。
- **Go**：`os.Executable()` 行为类似，可能影响自更新程序。
- **Shell scripts**：依赖 `$0` 定位自身路径的脚本（在 rust-exec 中 `argv[0]` 被替换为 `linker64`，但 shell 通常能从 `$1` 或参数中恢复，风险较低）。

## 临时绕过

### Gemini CLI

```bash
export GEMINI_CLI_NO_RELAUNCH=true
gemini
```

### 通用方案（其他工具）

如果某个工具报错且怀疑是 self-relaunch 导致，尝试：

1. 查找该工具的 "no relaunch" 或 "single process" 开关。
2. 手动用 `node <tool>` 或 `python -m <tool>` 的方式运行，绕过 shebang 层。

## 可能的修复方向

### 方案 1：rust-exec 注入原始路径环境变量

在 `transform_exec` 中，将原始真实可执行文件路径写入环境变量：

```rust
// 例如：TERMUX_ORIGINAL_EXE_PATH=/data/.../usr/bin/node
new_env.push(format!("TERMUX_ORIGINAL_EXE_PATH={}", path));
```

下游运行时（Node.js、Python 等）可以打补丁，在 `/proc/self/exe` 指向 `linker64` 时回退到这个环境变量。但这需要修改上游运行时。

### 方案 2：rust-exec 保留更友好的 argv[0]

某些 dynamic linker 支持通过 `argv[0]` 传递原始程序名（如 Linux ld-linux 的 `--argv0`）。Android `linker64` 目前不支持。如果未来支持，可改为：

```bash
linker64 --argv0 /data/.../usr/bin/node /data/.../usr/bin/node script.js
```

### 方案 3：Node.js / 运行时侧修复

给 Termux 的 Node.js 打补丁，当检测到 `/proc/self/exe` 指向 `linker64` 时：

1. 尝试从 `PATH` 中解析 `node` 真实路径；
2. 或读取 `TERMUX_ORIGINAL_EXE_PATH` 等环境变量。

### 方案 4：应用层防御

CLI 工具（如 gemini）在 relaunch 前验证 `process.execPath` 是否真的是目标运行时：

```js
function getNodePath() {
  if (!process.execPath.includes('linker')) {
    return process.execPath;
  }
  // fallback: try `which node` or PATH resolution
}
```

但这依赖每个工具作者实现，不可控。

### 方案 5：文档与兼容层

在 Termux 文档中明确记录这一行为，并为常用工具提供 wrapper：

```bash
# ~/.bashrc
gemini() { GEMINI_CLI_NO_RELAUNCH=true command gemini "$@"; }
```

## 最小复现脚本

```js
// test_relaunch.js
const { spawn } = require('child_process');

// 复现 gemini 的 relaunch 逻辑
const spawnArgs = [
  ...process.execArgv,
  '--max-old-space-size=7501',
  __filename
];

console.log('execPath:', process.execPath);
console.log('spawnArgs:', spawnArgs);

const child = spawn(process.execPath, spawnArgs, { stdio: 'inherit' });
child.on('error', (err) => console.error('spawn error:', err.message));
child.on('close', (code) => console.log('exit code:', code));
```

在 `work/googleplay-base` 环境下运行：

```bash
node test_relaunch.js
# 输出：
# execPath: /apex/com.android.runtime/bin/linker64
# spawnArgs: [ '--max-old-space-size=7501', '/data/.../test_relaunch.js' ]
# error: expected absolute path: "--max-old-space-size=7501"
# exit code: 1
```

---

**结论**：这是 rust-exec W^X bypass 机制与 Unix 进程身份语义之间的已知冲突。当前最务实的方案是为受影响工具设置 `NO_RELAUNCH` 类环境变量，并考虑在 rust-exec 中注入标准环境变量供下游运行时识别。
