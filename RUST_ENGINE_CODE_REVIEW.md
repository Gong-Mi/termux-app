# Rust Engine 代码审查报告

**审查日期**: 2026-04-22  
**审查范围**: `terminal-emulator/src/main/rust/src/` 核心模块  
**重点**: 渲染管线、JNI 边界、生命周期管理、并发安全

---

## 🔴 P0 — 必须修复（运行时崩溃/画面异常）

### 1. Vulkan Present 缺失 GPU 同步信号量

**位置**: `render_thread.rs:271-278`

```rust
let present_info = ash::vk::PresentInfoKHR {
    wait_semaphore_count: 0,
    p_wait_semaphores: std::ptr::null(), // ← 无等待
    swapchain_count: 1,
    p_swapchains: &ctx.swapchain,
    p_image_indices: &image_index,
    ..Default::default()
};
```

**缺陷**: `queue_present` 未等待任何 semaphore，GPU 渲染与上屏之间零同步。这会导致：
- 画面撕裂（tearing）
- 渲染未完成即上屏，出现残缺帧
- 在 Mali/Adreno 驱动上可能触发 `VK_ERROR_OUT_OF_DATE_KHR`

**修复方案**:
- `flush_and_submit()` 后获取 Skia 的 GPU completion semaphore
- 在 `PresentInfoKHR` 中设置 `wait_semaphore_count = 1` 并指向 `ctx.render_finished_semaphore`
- 确保每帧的 `acquire_next_image` → `render` → `queue_present` 信号量链完整

---

### 2. 渲染线程直接解引用裸指针（UAF 风险）

**位置**: `render_thread.rs:189`

```rust
let term_ctx = unsafe { &*(current_engine_ptr as *const TerminalContext) };
```

**缺陷**: 渲染线程通过裸指针直接借用 `TerminalContext`，未增加 `Arc` 强引用计数。若此时 Java 层调用 `destroyEngine` 释放 `Arc`，渲染线程将立即访问已释放内存。

**触发场景**:
1. 用户关闭 Session（Java 调用 `destroyEngine`）
2. 渲染线程恰好在 `acquire_next_image` 与 `from_engine` 之间执行
3. `term_ctx.lock.try_read()` 访问悬空指针 → SIGSEGV

**修复方案**:
将 `ENGINE_POINTER` 改为存储 `Arc<TerminalContext>` 的裸指针，渲染线程内执行：
```rust
let term_ctx = unsafe { Arc::from_raw(current_engine_ptr) };
let frame = { /* 使用 Arc 克隆确保生命周期 */ };
let _ = Arc::into_raw(term_ctx); // 平衡引用计数
```
或更安全的方案：将 `ENGINE_POINTER` 本身改为 `OnceCell<Arc<TerminalContext>>` 全局存储。

---

## 🟡 P1 — 严重缺陷（稳定性/正确性）

### 3. `getColors` 中 `transmute` 导致 UB

**位置**: `terminal_emulator.rs:672-673`

```rust
unsafe {
    let _ = env.set_int_array_region(&j_array, 0,
        std::mem::transmute::<&[u32], &[i32]>(&colors));
}
```

**缺陷**: Rust 中 `transmute` 不同类型的切片引用违反类型别名规则，属于 undefined behavior。即使 `u32` 与 `i32` 大小相同，编译器仍可能基于此进行非法优化。

**修复方案**（零成本，已有依赖）：
```rust
let _ = env.set_int_array_region(&j_array, 0, bytemuck::cast_slice(&colors));
```
`bytemuck` 已在 `Cargo.toml` 中声明，应优先使用其提供的安全转换。

---

### 4. `nativeSetFontPath` 是空实现

**位置**: `terminal_view.rs:150-152`

```rust
if let Ok(path_str) = env.get_string(&path) {
    let _ = String::from(path_str); // ← 直接丢弃
}
```

**缺陷**: 函数读取字体路径后未保存到 `RENDER_FONT_PATH`，后续渲染仍使用旧字体或默认字体。

**修复方案**:
```rust
if let Ok(path_str) = env.get_string(&path) {
    crate::render_thread::set_render_font_path(&String::from(path_str));
}
```

---

### 5. `surface_ready` 在 Vulkan 初始化失败时仍置为 true

**位置**: `terminal_view.rs:60-65`

```rust
let _ = ctx_cell.get_or_init(|| {
    let ctx = unsafe { VulkanContext::new(window as _) };
    std::sync::Mutex::new(ctx) // ctx 可能为 None
});
crate::render_thread::get_surface_ready().store(true, Ordering::SeqCst); // ← 无条件置 true
```

**缺陷**: `VulkanContext::new` 返回 `Option`，失败时为 `None`。但代码无论成功与否都将 `surface_ready` 设为 `true`。渲染线程被唤醒后发现 `VULKAN_CONTEXT` 为 `None`，直接 break 并永久停止渲染。

**修复方案**:
```rust
let ctx = unsafe { VulkanContext::new(window as _) };
if ctx.is_some() {
    let _ = ctx_cell.get_or_init(|| std::sync::Mutex::new(ctx));
    crate::render_thread::get_surface_ready().store(true, Ordering::SeqCst);
} else {
    android_log(LogPriority::ERROR, "VulkanContext::new failed, surface_ready remains false");
}
```

---

### 6. `waitpid(-1)` 全局收割子进程

**位置**: `coordinator.rs:99`

```rust
let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
```

**缺陷**:
1. `-1` 会收割**任意**子进程，不仅限于 Termux 创建的进程。若其他库/线程也在调用 `waitpid`，产生事件竞争。
2. `waitpid` 返回 `-1` 且 `errno == EINTR` 时，代码将其与 `ECHILD` 同样处理（sleep 500ms），但 `EINTR` 应由用户立即重试。

**修复方案**:
对已知 PID 列表轮询非阻塞等待：
```rust
// 在 monitor 线程中
let pids: Vec<i32> = { /* 从 pid_map 读取当前追踪的 PID 列表 */ };
for pid in pids {
    let mut status: i32 = 0;
    let ret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    if ret == pid { /* 处理退出 */ }
}
std::thread::sleep(Duration::from_millis(100));
```

---

## 🟠 P2 — 架构风险（长期稳定性）

### 7. RwLock Poison 未处理

**影响范围**: `terminal_emulator.rs`, `context.rs`, `render_thread.rs`

全代码中大量使用：
```rust
context.lock.write().unwrap()
```

**缺陷**: 若某 JNI 函数在持有写锁时 panic（即使被 `catch_unwind` 捕获闭包内 panic，锁本身仍会被标记为 poison），后续所有 `.unwrap()` 都会直接 panic。

**典型后果**:
- `processBatch` 的 `catch_unwind` 捕获一次 panic 后，锁已 poison
- 此后所有来自 Java 的输入处理都会 panic → 终端冻结
- IO 线程（`context.rs:128`）因 `unwrap()`  panic 而静默死亡，PTY 不再读取

**修复方案**:
对引擎锁统一使用 `write()` 并恢复 poison：
```rust
let mut engine = match context.lock.write() {
    Ok(g) => g,
    Err(poisoned) => poisoned.into_inner(), // 恢复数据，继续运行
};
```

---

### 8. `destroyEngine` 与 `createSessionAsync` 的竞态窗口

**位置**: `terminal_emulator.rs:169-185` 与 `terminal_emulator.rs:957-991`

**缺陷**: `destroyEngine` 在主线程执行 `pty_fd.swap(-1)` 并 `close(fd)`，而 `createSessionAsync` 在后台线程中执行 `dup(pty_fd)`。若 `destroyEngine` 恰好在 `dup` 之前执行，后台线程将 `dup(-1)`，IO 线程读取无效 fd 后立即退出。

**缓解方案**: 在 `TerminalContext` 中增加一个 `AtomicBool` 标志（如 `destroyed`），`createSessionAsync` 在 `dup` 前检查；或确保 Java 层在 `createSessionAsync` 完成前不调用 `destroyEngine`。

---

## 修复优先级建议

| 优先级 | 问题 | 影响 |
|--------|------|------|
| P0 | Vulkan 信号量缺失 | 画面撕裂、GPU 崩溃 |
| P0 | 渲染线程裸指针 UAF | 切后台/关 Session 时闪退 |
| P1 | `transmute` UB | 未定义行为，未来 Rust 版本可能爆炸 |
| P1 | `nativeSetFontPath` 空实现 | 自定义字体功能失效 |
| P1 | `surface_ready` 错误设置 | Vulkan 初始化失败时黑屏 |
| P1 | `waitpid(-1)` | 子进程状态监控不可靠 |
| P2 | RwLock Poison | 终端核心永久冻结 |
| P2 | `destroyEngine` 竞态 | 偶发性 Session 创建失败 |

---

## 附录：验证命令

```bash
# 检查 Vulkan present 信号量
cd terminal-emulator/src/main/rust
grep -n "wait_semaphore_count" src/render_thread.rs src/vulkan_context.rs

# 检查裸指针解引用
grep -n "as \*const TerminalContext" src/render_thread.rs

# 检查 transmute
grep -rn "transmute" src/

# 检查空实现
grep -A2 "nativeSetFontPath" src/jni/terminal_view.rs
```
