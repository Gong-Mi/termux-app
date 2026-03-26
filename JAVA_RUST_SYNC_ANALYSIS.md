# Java-Rust 同步机制分析

**分析日期**: 2026-03-26  
**问题**: 屏幕外内容丢失，扩大屏幕时显示异常

---

## 数据流

```
PTY → Java InputReader → mProcessToTerminalIOQueue → MSG_NEW_INPUT → 
mEmulator.append() → Rust process_bytes() → sync_screen_to_flat_buffer() → 
共享内存 → notifyScreenUpdate() → UI 渲染
```

---

## 同步机制

### ✅ 已正确同步的部分

| 操作 | Rust 侧 | Java 侧 | 同步方式 | 状态 |
|------|--------|--------|----------|------|
| **屏幕数据写入** | `process_bytes()` | `append()` | 共享内存 | ✅ |
| **屏幕数据读取** | `copy_row_codepoints()` | `readRow()` | 共享内存 | ✅ |
| **active_transcript_rows 查询** | `getActiveTranscriptRowsFromRust()` | `getActiveTranscriptRows()` | JNI 调用 | ✅ |
| **first_row 查询** | 内部使用 | 不直接访问 | - | ✅ |

### ⚠️ 潜在问题的部分

| 操作 | Rust 侧 | Java 侧 | 同步方式 | 状态 |
|------|--------|--------|----------|------|
| **resize** | `resize_with_reflow()` | `resize()` | JNI 调用 | ⚠️ |
| **active_transcript_rows 更新** | 自动更新 | 无本地副本 | - | ⚠️ |
| **first_row 更新** | 自动更新 | 无本地副本 | - | ⚠️ |

---

## 关键发现

### 1. Rust 完全绕过了 TerminalBuffer

**Java 版本**：
```java
// TerminalEmulator.java
private TerminalBuffer mScreen;  // 使用 TerminalBuffer 管理屏幕

// TerminalBuffer.java
private int mActiveTranscriptRows = 0;  // 独立维护
private int mScreenFirstRow = 0;
TerminalRow[] mLines;  // 环形缓冲区
```

**Rust 版本**：
```rust
// engine.rs
pub main_screen: Screen,  // 直接使用 Screen
pub active_transcript_rows: usize,  // Rust 侧维护
pub first_row: usize,  // Rust 侧维护
// 共享内存直接存储屏幕数据
```

### 2. Java 侧没有本地副本

**Rust 版本的 `TerminalEmulator.java`**：
```java
// 没有 mActiveTranscriptRows 字段！
// 没有 mScreenFirstRow 字段！
// 没有 TerminalBuffer 实例！

public int getActiveTranscriptRows() {
    if (mEnginePtr != 0) return getActiveTranscriptRowsFromRust(mEnginePtr);
    return 0;  // 直接从 Rust 获取
}

public void readRow(int row, int[] text, long[] styles) {
    if (mEnginePtr != 0) {
        readRowFromRust(mEnginePtr, row, text, styles);  // 直接读共享内存
    }
}
```

### 3. 同步时机

**写入同步**：
```rust
// engine.rs
pub fn process_bytes(&mut self, data: &[u8]) {
    let mut handler = PerformHandler { state: &mut self.state };
    self.parser.advance(&mut handler, data);
    self.state.sync_screen_to_flat_buffer();  // ✅ 同步到共享内存
    if !self.state.shared_buffer_ptr.is_null() {
        unsafe { 
            if let Some(flat) = &self.state.flat_buffer { 
                flat.sync_to_shared(self.state.shared_buffer_ptr); 
            } 
        }
    }
}
```

**Java 通知**：
```java
// TerminalSession.java
public void handleMessage(Message msg) {
    while ((bytesRead = mProcessToTerminalIOQueue.read(mReceiveBuffer, false)) > 0) {
        mEmulator.append(mReceiveBuffer, bytesRead);  // 调用 Rust
        // ...
    }
    if (totalBytesRead > 0) {
        notifyScreenUpdate();  // ✅ 通知 UI 更新
    }
}
```

---

## 问题分析

### 扩大屏幕时的行为

**场景**：
1. 初始：80x10，写入 20 行
2. 扩大到：80x18

**Rust 侧**：
```
初始：
  active_transcript_rows = 11
  first_row = 11
  rows = 10
  可见内容：[11..20] (Line 12-Line 20)

扩大后：
  shift = 10 - 18 = -8
  active_transcript_rows = 11 - 8 = 3
  first_row = 11 - 8 = 3
  rows = 18
  可见内容：[3..20] (Line 04-Line 20)
```

**Java 侧**：
```
通过 JNI 获取 active_transcript_rows：
  getActiveTranscriptRowsFromRust() → 3

读取历史行：
  readRow(-3) → Line 01 ✓
  readRow(-2) → Line 02 ✓
  readRow(-1) → Line 03 ✓

读取可见行：
  readRow(0) → Line 04 ✓
  readRow(17) → (空) ✓
```

### 测试验证结果

```
扩大后可见内容 (18 行):
   行 0: 'Line 04'
   行 1: 'Line 05'
   ...
   行 16: 'Line 20'
   行 17: ''

历史行:
   历史行 -3: 'Line 01' ✓
   历史行 -2: 'Line 02' ✓
   历史行 -1: 'Line 03' ✓
```

**结论**：**Rust 侧的行为是正确的**！

---

## 可能的问题场景

### 场景 1：Java 侧缓存了旧值

如果 Java 侧有代码缓存了 `active_transcript_rows` 的旧值，可能导致访问错误的行。

**检查点**：
- `TerminalSession.java` 是否有缓存？
- `TerminalView.java` 是否有缓存？

### 场景 2：共享内存同步延迟

如果 Rust 更新了 `active_transcript_rows` 但共享内存还没同步，Java 可能读到旧数据。

**当前实现**：
```rust
// resize_with_reflow 结束时
self.active_transcript_rows = ...;
self.first_row = ...;
// 但没有调用 sync_screen_to_flat_buffer()！
```

**问题**：resize 后可能没有同步共享内存！

### 场景 3：边界检查缺失

如果 Java 请求超出范围的行，Rust 应该返回正确的数据而不是崩溃。

**修复**：已添加边界检查到 `get_row()` 和 `get_row_mut()`。

---

## 修复建议

### 修复 1：resize 后同步共享内存

```rust
// screen.rs - resize_with_reflow 结束时
self.rows = new_rows;

// ✅ 添加同步调用
self.sync_screen_to_flat_buffer();  // 需要添加到 ScreenState

// 通知 Java 侧（如果需要）
self.report_screen_update();  // 如果实现了这个回调
```

### 修复 2：确保 Java 侧不缓存状态

检查 Java 代码，确保：
- 不缓存 `active_transcript_rows`
- 每次都通过 JNI 获取最新值

### 修复 3：添加调试日志

```rust
// resize_rows_only 结束时
android_log(LogPriority::DEBUG, &format!(
    "resize: {}x{} -> {}x{}, active_transcript_rows: {} -> {}, first_row: {} -> {}",
    old_cols, old_rows, new_cols, new_rows,
    old_active, self.active_transcript_rows,
    old_first, self.first_row
));
```

---

## 结论

**Rust 侧的逻辑是正确的**，问题可能出在：

1. **共享内存同步时机** - resize 后可能没有同步
2. **Java 侧缓存** - 可能缓存了旧的状态值
3. **边界条件处理** - 已修复，但需要验证

**下一步**：
1. 在 resize 后添加共享内存同步
2. 检查 Java 侧是否有状态缓存
3. 在真实 Android 应用中测试
