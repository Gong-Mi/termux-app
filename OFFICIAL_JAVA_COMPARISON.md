# 官方 Java 版本对比分析

## 关键发现

### 官方 Java 版本的坐标系统

```java
// TerminalBuffer.java
/**
 * - External coordinate system: -mActiveTranscriptRows to mScreenRows-1
 *   with the screen being 0..mScreenRows-1.
 * - Internal coordinate system: the mScreenRows lines starting at mScreenFirstRow
 *   comprise the screen, while the mActiveTranscriptRows lines ending at
 *   mScreenFirstRow-1 form the transcript (as a circular buffer).
 *
 * External ↔ Internal:
 * [ -mActiveTranscriptRows         ]  ↔  [ mScreenFirstRow - mActiveTranscriptRows ]
 * [ 0 (visible screen starts here) ]  ↔  [ mScreenFirstRow                         ]
 * [ mScreenRows-1                  ]  ↔  [ mScreenFirstRow + mScreenRows-1         ]
 */
public int externalToInternalRow(int externalRow) {
    if (externalRow < -mActiveTranscriptRows || externalRow > mScreenRows)
        throw new IllegalArgumentException(...);
    final int internalRow = mScreenFirstRow + externalRow;
    return (internalRow < 0) ? (mTotalRows + internalRow) : (internalRow % mTotalRows);
}
```

**关键点**：
- **外部坐标**：`[-mActiveTranscriptRows, mScreenRows-1]`
- **外部行 0** = 屏幕第一行 = 内部 `mScreenFirstRow`
- **外部行 -1** = 历史最后一行 = 内部 `mScreenFirstRow - 1`

---

### 我们的 Rust 实现

```rust
// screen.rs
#[inline]
pub fn internal_row(&self, row: i32) -> usize {
    let t = self.buffer.len() as i64;
    if t == 0 { return 0; }
    (((self.first_row as i64 + row as i64) % t + t) % t) as usize
}

pub fn get_row(&self, row: i32) -> &TerminalRow { 
    &self.buffer[self.internal_row(row)] 
}
```

**对比**：
- Java: `internalRow = mScreenFirstRow + externalRow`
- Rust: `internal_row = first_row + row`

**完全一致**！✅

---

## 关键差异：getActiveTranscriptRows()

### 官方 Java 版本

```java
// TerminalBuffer.java
private int mActiveTranscriptRows = 0;

public int getActiveTranscriptRows() {
    return mActiveTranscriptRows;
}

// resize() 快速路径
mActiveTranscriptRows = altScreen ? 0 : Math.max(0, mActiveTranscriptRows + shiftDownOfTopRow);

// scrollDownOneLine()
if (mActiveTranscriptRows < mTotalRows - mScreenRows) mActiveTranscriptRows++;
```

**特点**：
- `mActiveTranscriptRows` 是**独立维护的状态变量**
- 每次滚动时增加
- resize 时根据 shift 调整

### 我们的 Rust 实现

```rust
// screen.rs - resize_with_reflow 结束时
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
self.first_row = self.active_transcript_rows % self.buffer.len();
```

**特点**：
- `active_transcript_rows` 通过**计算得出**
- 基于 `output_row`（实际写入行数）
- 每次 resize 都重新计算

---

## 潜在问题

### 问题 1：`first_row` 的计算

**Java 版本**：
```java
// resize() 快速路径
mScreenFirstRow += shiftDownOfTopRow;
mScreenFirstRow = (mScreenFirstRow < 0) 
    ? (mScreenFirstRow + mTotalRows) 
    : (mScreenFirstRow % mTotalRows);
```

**Rust 版本**：
```rust
self.first_row = self.active_transcript_rows % self.buffer.len();
```

**差异**：
- Java：`first_row` 通过 **shift 累加**
- Rust：`first_row` 通过 **active_transcript_rows 计算**

**可能导致的问题**：
- 如果 `active_transcript_rows` 计算不准确，`first_row` 也会错误
- 环形缓冲区的起始位置可能偏移

---

### 问题 2：`active_transcript_rows` 的维护

**Java 版本**：
```java
// scrollDownOneLine() - 每次滚动时
if (mActiveTranscriptRows < mTotalRows - mScreenRows) 
    mActiveTranscriptRows++;

// resize() - 根据 shift 调整
mActiveTranscriptRows = altScreen ? 0 : Math.max(0, mActiveTranscriptRows + shiftDownOfTopRow);
```

**Rust 版本**：
```rust
// resize_with_reflow 结束时
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
```

**差异**：
- Java：**增量维护**，每次滚动时更新
- Rust：**重新计算**，resize 时基于 `output_row`

**可能导致的问题**：
- `output_row` 可能不准确（例如跳过空行时）
- 重新计算可能丢失历史信息

---

### 问题 3：getSelectedText 的行范围检查

**Java 版本**：
```java
// getSelectedText()
if (selY1 < -getActiveTranscriptRows()) 
    selY1 = -getActiveTranscriptRows();
if (selY2 >= mScreenRows) 
    selY2 = mScreenRows - 1;

for (int row = selY1; row <= selY2; row++) {
    TerminalRow lineObject = mLines[externalToInternalRow(row)];
    // ...
}
```

**Rust 版本**：
```rust
// lib.rs - getSelectedTextFromRust
for row in y1..=y2 {
    let line = screen.get_row(row);
    // ...
}
```

**差异**：
- Java：有**边界检查**
- Rust：**依赖 get_row() 的 internal_row()**

**可能导致的问题**：
- 如果 `active_transcript_rows` 计算错误，Rust 可能访问无效行

---

## 测试验证

### 官方 Java 行为

```java
// 写入 100 行到 24 行屏幕
// mActiveTranscriptRows = 76 (100 - 24)
// mScreenFirstRow = 76 (假设从 0 开始)

// 读取行 -76（第一行历史）
externalToInternalRow(-76) = 76 + (-76) = 0
→ mLines[0] 应该是 "Line 1"

// 读取行 0（屏幕第一行）
externalToInternalRow(0) = 76 + 0 = 76
→ mLines[76] 应该是 "Line 77"

// 读取行 23（屏幕最后一行）
externalToInternalRow(23) = 76 + 23 = 99
→ mLines[99] 应该是 "Line 100"
```

### 我们的 Rust 行为

```rust
// 写入 100 行到 24 行屏幕
// active_transcript_rows = 76
// first_row = 76 % 1000 = 76

// 读取行 -76（第一行历史）
internal_row(-76) = ((76 + (-76)) % 1000 + 1000) % 1000 = 0
→ buffer[0] 应该是 "Line 1" ✓

// 读取行 0（屏幕第一行）
internal_row(0) = ((76 + 0) % 1000 + 1000) % 1000 = 76
→ buffer[76] 应该是 "Line 77" ✓

// 读取行 23（屏幕最后一行）
internal_row(23) = ((76 + 23) % 1000 + 1000) % 1000 = 99
→ buffer[99] 应该是 "Line 100" ✓
```

**数学上等价**！✅

---

## 结论

### Rust 实现 vs Java 实现

| 功能 | Java | Rust | 状态 |
|------|------|------|------|
| 坐标转换公式 | `first_row + row` | `first_row + row` | ✅ 等价 |
| active_transcript 维护 | 增量更新 | 重新计算 | ⚠️ 可能不同 |
| first_row 计算 | shift 累加 | active_transcript 推导 | ⚠️ 可能不同 |
| 边界检查 | 有 | 依赖 internal_row | ⚠️ 可能不同 |

### 核心问题

**`active_transcript_rows` 的计算可能不准确**

Java 通过**增量维护**确保准确性：
```java
// 每次滚动时
if (mActiveTranscriptRows < mTotalRows - mScreenRows) 
    mActiveTranscriptRows++;
```

Rust 通过**重新计算**：
```rust
// resize 时
self.active_transcript_rows = (output_row + 1).saturating_sub(new_rows);
```

**如果 `output_row` 追踪不准确，就会导致问题！**

---

## 建议修复

### 方案 1：增量维护 `active_transcript_rows`

```rust
// scroll_up() 中
pub fn scroll_up(&mut self, top: i32, bottom: i32, style: u64) {
    if top == 0 && bottom == self.rows {
        self.first_row = (self.first_row + 1) % self.buffer.len();
        // 增量维护！
        if self.active_transcript_rows < self.buffer.len() - self.rows as usize { 
            self.active_transcript_rows += 1; 
        }
        self.get_row_mut(self.rows - 1).clear(0, c, style);
    }
    // ...
}
```

### 方案 2：resize 时使用 Java 的 shift 逻辑

```rust
pub fn resize_rows_only(&mut self, new_rows: i32, ...) {
    let shift = self.rows - new_rows;
    // Java 式计算
    self.active_transcript_rows = (self.active_transcript_rows as i32 + shift).max(0) as usize;
    self.first_row = ((self.first_row as i32 + shift) % self.buffer.len() as i32 + 
                      self.buffer.len() as i32) as usize % self.buffer.len();
    self.rows = new_rows;
}
```

### 方案 3：添加边界检查

```rust
// get_row() 中添加检查
pub fn get_row(&self, row: i32) -> &TerminalRow { 
    let max_history = -(self.active_transcript_rows as i32);
    let row = row.max(max_history).min(self.rows as i32 - 1);
    &self.buffer[self.internal_row(row)] 
}
```

---

## 下一步

1. **添加日志**验证 `active_transcript_rows` 的值
2. **对比 Java 和 Rust**在相同操作后的值
3. **考虑增量维护**而不是重新计算
4. **添加边界检查**防止访问无效行
