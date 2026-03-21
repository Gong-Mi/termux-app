# 崩溃修复报告 - Use-After-Free 竞争条件

## 问题描述

Termux 应用在运行过程中出现崩溃，crash log 显示：
```
signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), fault addr 0x0000000000000000
Cause: null pointer dereference
```

崩溃堆栈显示问题出在：
```
#03 pc 0000000000065de8 ... (Java_com_termux_terminal_TerminalEmulator_processBatchRust+108)
#10 pc 000000000000bc3c ... (com.termux.terminal.TerminalEmulator.processBatch+0)
#15 pc 000000000000bb9c ... (com.termux.terminal.TerminalEmulator.append+0)
#20 pc 000000000000c8c8 ... (com.termux.terminal.TerminalSession$MainThreadHandler.handleMessage+0)
```

## 根本原因分析

### 1. 竞争条件导致的 Use-After-Free

**场景：**
1. 用户关闭终端或进程退出
2. `cleanupResources()` 被调用，`mEmulator.destroy()` 将 `mEnginePtr` 设为 0
3. **但是** `InputReader` 线程可能仍在读取 PTY 数据
4. 如果还有数据到达，`MSG_NEW_INPUT` 消息会被发送到 Handler
5. `handleMessage` 调用 `mEmulator.append()` 时，`mEnginePtr` 已经是 0
6. Rust 层的 `processBatchRust` 虽然有 `if ptr == 0` 检查，但存在竞争条件：
   - Java 层检查 `mEnginePtr != 0` 时指针有效
   - 但在调用 `processBatchRust` 后，指针可能立即被销毁
   - Rust 代码中的检查通过后，`unsafe { &mut *(ptr as *mut TerminalContext) }` 解引用已释放的内存

### 2. 其他方法缺少生命周期检查

以下方法虽然有 null 检查，但在 `mEmulator` 不为 null 但 `mEnginePtr` 已销毁的情况下仍可能崩溃：
- `updateTerminalSessionClient()`
- `updateSize()` (resize 调用)
- `getTitle()`
- `reset()`

### 3. 关于日志中的其他错误

日志中显示的以下错误**不是 Termux 代码问题**：
- `FrameInsert open fail: No such file or directory` - 来自 MIUI 系统的 SurfaceFlinger
- `Failed to query component interface for required system resources: 6` - 来自 LSposed Hidden API Bypass 模块

这些是系统级错误，不影响 Termux 功能。

## 修复方案

### 1. 添加 `isAlive()` 方法到 `TerminalEmulator.java`

```java
/**
 * 检查终端引擎是否仍然有效（未被销毁）
 */
public synchronized boolean isAlive() {
    return mEnginePtr != 0;
}
```

### 2. 在 `TerminalSession.MainThreadHandler.handleMessage()` 中添加生命周期检查

在处理 `MSG_NEW_INPUT` 和追加退出消息前，检查终端是否仍然有效：

```java
@Override
public void handleMessage(Message msg) {
    // 检查终端是否已被销毁，防止在销毁后继续处理数据导致崩溃
    if (msg.what != MSG_PROCESS_EXITED && (mEmulator == null || !mEmulator.isAlive())) {
        return;
    }

    int totalBytesRead = 0;
    int bytesRead;
    while ((bytesRead = mProcessToTerminalIOQueue.read(mReceiveBuffer, false)) > 0) {
        // 在每次 append 前检查终端是否仍然有效
        if (mEmulator == null || !mEmulator.isAlive()) {
            break;
        }
        mEmulator.append(mReceiveBuffer, bytesRead);
        // ...
    }
    // ...

    if (msg.what == MSG_PROCESS_EXITED) {
        // ...
        // 在进程退出后追加消息前，检查终端是否仍然有效
        if (mEmulator != null && mEmulator.isAlive()) {
            mEmulator.append(bytesToWrite, bytesToWrite.length);
            notifyScreenUpdate();
        }
        mClient.onSessionFinished(TerminalSession.this);
    }
}
```

### 3. 在其他方法中添加 `isAlive()` 检查

修复以下方法：
- `updateTerminalSessionClient()`
- `updateSize()`
- `getTitle()`
- `reset()`

### 4. 在 Rust 层添加 `catch_unwind` 保护

防止 Rust panic 导致 JVM 崩溃：

```rust
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_processBatchRust(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    batch: jbyteArray,
    length: jint,
) {
    if ptr == 0 || batch.is_null() { return; }
    // 使用 catch_unwind 防止 Rust panic 导致 JVM 崩溃
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &mut *(ptr as *mut TerminalContext) };
        let j_array = unsafe { jni::objects::JByteArray::from_raw(batch) };
        if let Ok(bytes) = env.convert_byte_array(&j_array) {
            let len = length as usize;
            let actual_len = std::cmp::min(len, bytes.len());
            context.engine.process_bytes(&bytes[..actual_len]);
        }
    }));
    if result.is_err() {
        android_log(LogPriority::ERROR, "processBatchRust: panic caught, possible use-after-free");
    }
}
```

## 修改的文件

1. `terminal-emulator/src/main/java/com/termux/terminal/TerminalEmulator.java`
   - 添加 `isAlive()` 方法

2. `terminal-emulator/src/main/java/com/termux/terminal/TerminalSession.java`
   - 修改 `MainThreadHandler.handleMessage()` 添加生命周期检查
   - 修改 `updateTerminalSessionClient()` 添加 `isAlive()` 检查
   - 修改 `updateSize()` 添加 `isAlive()` 检查
   - 修改 `getTitle()` 添加 `isAlive()` 检查
   - 修改 `reset()` 添加 `isAlive()` 检查

3. `terminal-emulator/src/main/rust/src/lib.rs`
   - 修改 `nativeProcess()` 添加 `catch_unwind` 保护
   - 修改 `processBatchRust()` 添加 `catch_unwind` 保护

## 测试建议

1. **快速开关终端测试**：反复快速创建和关闭终端会话
2. **进程退出测试**：在终端中运行命令并快速关闭窗口
3. **大数据量测试**：在终端中输出大量数据时关闭会话
4. **压力测试**：多线程同时操作多个终端会话

## 预期结果

修复后，上述场景不应再出现 SIGSEGV 崩溃。
