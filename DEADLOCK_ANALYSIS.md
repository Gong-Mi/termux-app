# Rust 集成中的死锁崩溃分析

## 🔴 问题：为什么没有互锁会导致崩溃？

### 崩溃场景重现

**时间:** 2026-03-29  
**现象:** 创建第 2 个 session 时应用卡死/重启  
**根本原因:** **死锁 (Deadlock)**

---

## 📊 死锁形成过程

### 步骤 1: 第 2 个 session 创建

```
用户点击 '+' 创建第 2 个 session
    ↓
TermuxActivity.addNewSession()
    ↓
TermuxService.createTermuxSession()
    ↓
TerminalSession.initializeEmulator()
    ↓
JNI.createSessionAsync()  ← Rust 后台线程
    ↓
Rust: 创建 PTY 和 Engine
    ↓
Rust: onEngineInitialized 回调 Java
    ↓
Java: TerminalEmulator.<init>()
    ↓
Java: checkForFontAndColors()  ← 检查颜色
    ↓
Java: getCurrentColors()
    ↓
Java: getColorsFromRust(mEnginePtr)  ← JNI 调用
    ↓
Rust: 获取读锁 (lock.read())
    ↓
Rust: 返回颜色数组
    ↓
Java: 收到颜色数据
    ↓
Java: updateBackgroundColor()
    ↓
Java: resetColorsFromRust(mEnginePtr)  ← 重置颜色
    ↓
Rust: 获取写锁 (lock.write())  ← 【持有锁】
    ↓
Rust: engine.state.colors.reset()
    ↓
Rust: engine.state.report_colors_changed()  ← 【死锁点!】
    ↓
Rust: vm.get_env()  ← 尝试获取 JNIEnv
    ↓
Rust: env.call_method("onColorsChanged")  ← 回调 Java
    ↓
Java: onColorsChanged() 被调用
    ↓
Java: checkForFontAndColors()  ← 又调用回来了!
    ↓
Java: getCurrentColors()
    ↓
Java: getColorsFromRust(mEnginePtr)  ← 又调用 Rust!
    ↓
Rust: 尝试获取读锁...  ← 【死锁!】
```

---

## 🔍 死锁分析

### 死锁四要素

| 要素 | 是否满足 | 说明 |
|------|---------|------|
| 互斥条件 | ✅ | RwLock 同一时间只允许一个写锁 |
| 请求与保持 | ✅ | Rust 持有写锁，请求 Java 回调 |
| 不剥夺 | ✅ | 锁不能被强制释放 |
| 循环等待 | ✅ | Java 等 Rust 返回，Rust 等 Java 回调 |

### 锁依赖图

```
┌─────────────┐
│  Java 线程  │
│  (主线程)   │
└──────┬──────┘
       │
       │ 1. getColorsFromRust()
       ▼
┌─────────────┐
│   Rust 读锁  │
└──────┬──────┘
       │
       │ 2. 返回颜色
       ▼
┌─────────────┐
│  Java 线程  │
└──────┬──────┘
       │
       │ 3. resetColorsFromRust()
       ▼
┌─────────────┐
│   Rust 写锁  │ ← 【持有】
└──────┬──────┘
       │
       │ 4. report_colors_changed()
       ▼
┌─────────────┐
│  JNI 回调   │
│ onColorsChanged()
└──────┬──────┘
       │
       │ 5. checkForFontAndColors()
       ▼
┌─────────────┐
│  Java 线程  │
└──────┬──────┘
       │
       │ 6. getColorsFromRust()
       ▼
┌─────────────┐
│ 等待 Rust 读锁│ ← 【死锁!】
│ (但写锁未释放)│
└─────────────┘
```

---

## 💥 为什么会崩溃？

### 直接原因：ANR (Application Not Responding)

```
主线程被阻塞 > 5 秒
    ↓
Android 系统检测到 ANR
    ↓
显示"应用无响应"对话框
    ↓
用户等待或强制关闭
    ↓
如果用户不操作，系统可能杀掉进程
```

### 根本原因：架构设计缺陷

**错误的代码模式:**

```rust
// ❌ 错误代码 (engine.rs 修复前)
pub fn report_colors_changed(&self) {
    if let Some(obj) = &self.java_callback_obj {
        if let Some(vm) = crate::JAVA_VM.get() {
            if let Ok(mut env) = vm.get_env() {
                // 【问题】在持有锁的情况下回调 Java
                let _ = env.call_method(obj.as_obj(), "onColorsChanged", "()V", &[]);
            }
        }
    }
}

// ❌ 错误代码 (lib.rs 修复前)
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong
) {
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    
    // 【问题】在锁内调用 report_colors_changed
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();  // ← 持有写锁
        engine.state.colors.reset();
        engine.state.report_colors_changed();  // ← 回调 Java
        (engine.take_events(), engine.state.java_callback_obj.clone())
    };
    
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}
```

---

## 🔧 修复方案

### 修复 1: 移除直接回调

```rust
// ✅ 修复后 (engine.rs)
/// 修复：只标记颜色变化标志，不再直接回调 Java
/// 颜色变化事件会在下次取事件时被处理，避免在持有锁时调用 JNI 导致的死锁
pub fn report_colors_changed(&self) {
    // 不再直接调用 Java 回调，由调用者在锁外通过事件机制处理
    // 这样可以避免死锁问题
}
```

### 修复 2: 在锁外回调

```rust
// ✅ 修复后 (lib.rs)
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_resetColorsFromRust(
    mut env: JNIEnv, _class: JClass, ptr: jlong
) {
    let context = unsafe { Arc::from_raw(ptr as *const TerminalContext) };
    
    // ✅ 修复：在锁外回调，避免死锁
    let (events, cb) = {
        let mut engine = context.lock.write().unwrap();
        engine.state.colors.reset();
        // ✅ 不再调用 report_colors_changed()，而是手动添加事件
        let mut events = engine.take_events();
        events.push(crate::engine::TerminalEvent::ColorsChanged);
        (events, engine.state.java_callback_obj.clone())
    }; // ✅ 锁在此处释放
    
    // ✅ 在锁外安全回调 Java
    flush_events_to_java(&mut env, &cb, events);
    let _ = Arc::into_raw(context);
}
```

---

## 📋 为什么互锁机制能防止崩溃？

### Session 协调器的作用

```rust
// coordinator.rs
pub struct SessionCoordinator {
    pkg_lock: AtomicBool,           // ← 互斥锁
    pkg_lock_owner: AtomicUsize,    // ← 锁所有者
    session_states: Mutex<HashMap<usize, SessionState>>,
}
```

### 互锁防止崩溃的原理

**没有互锁时:**
```
Session 1: pkg upgrade  → 直接执行
Session 2: pkg upgrade  → 直接执行
    ↓
两个进程同时修改 dpkg 数据库
    ↓
数据竞争 → 崩溃
```

**有互锁时:**
```
Session 1: try_acquire_pkg_lock() → ✓ 成功 → 执行 pkg
Session 2: try_acquire_pkg_lock() → ✗ 失败 → 等待
    ↓
只有一个进程执行 pkg
    ↓
避免数据竞争 → 不崩溃
```

---

## 🎯 总结

### 崩溃原因

1. **死锁** - Rust 持有锁时回调 Java，Java 又调用回 Rust
2. **ANR** - 主线程被阻塞超过 5 秒
3. **架构缺陷** - 在锁内执行 JNI 回调

### 为什么之前没有互锁？

1. **设计疏忽** - 没有预见到回调链会导致死锁
2. **Rust-Java 交互复杂** - 跨语言边界容易出错
3. **测试不足** - 多 session 并发测试不够

### 互锁机制的价值

1. **防止死锁** - SessionCoordinator 统一管理锁
2. **应用层感知** - 知道 pkg 操作状态
3. **友好提示** - 可以显示用户友好的错误信息
4. **可扩展** - 可以实现排队、等待等高级功能

---

**一句话:** 没有互锁 → 死锁 → ANR → 崩溃/卡死  
**解决方案:** Session 协调器 + 事件机制 + 锁外回调
