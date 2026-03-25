# Java Resize 逻辑复刻指南

## Java 快速路径逻辑

Java 的 `TerminalBuffer.resize()` 有一个**快速路径**，当只有行数变化时（列数不变），只调整指针而不移动数据：

```java
// Java TerminalBuffer.java line 204
if (newColumns == mColumns && newRows <= mTotalRows) {
    // Fast resize where just the rows changed.
    int shiftDownOfTopRow = mScreenRows - newRows;
    
    if (shiftDownOfTopRow > 0 && shiftDownOfTopRow < mScreenRows) {
        // Shrinking: 检查底部空行
        for (int i = mScreenRows - 1; i > 0; i--) {
            if (cursor[1] >= i) break;
            int r = externalToInternalRow(i);
            if (mLines[r] == null || mLines[r].isBlank()) {
                if (--shiftDownOfTopRow == 0) break;
            }
        }
    } else if (shiftDownOfTopRow < 0) {
        // Expanding: 只有当有历史记录时才移动
        int actualShift = Math.max(shiftDownOfTopRow, -mActiveTranscriptRows);
        if (shiftDownOfTopRow != actualShift) {
            // 清空新暴露的行
            for (int i = 0; i < actualShift - shiftDownOfTopRow; i++)
                allocateFullLineIfNecessary(
                    (mScreenFirstRow + mScreenRows + i) % mTotalRows
                ).clear(currentStyle);
            shiftDownOfTopRow = actualShift;
        }
    }
    
    // 应用 shift
    mScreenFirstRow += shiftDownOfTopRow;
    mScreenFirstRow = (mScreenFirstRow < 0) ? (mScreenFirstRow + mTotalRows) 
                                             : (mScreenFirstRow % mTotalRows);
    mActiveTranscriptRows = altScreen ? 0 : Math.max(0, mActiveTranscriptRows + shiftDownOfTopRow);
    cursor[1] -= shiftDownOfTopRow;
    mScreenRows = newRows;
}
```

## 关键公式

### 1. shiftDownOfTopRow 计算

```
shiftDownOfTopRow = oldScreenRows - newScreenRows
```

- **正值**：屏幕缩小，顶部行需要下移
- **负值**：屏幕扩大，顶部行可以上移

### 2. 缩小优化（空行跳过）

```java
for (int i = oldScreenRows - 1; i > 0; i--) {
    if (cursorRow >= i) break;  // 不能跳过光标所在行及以下
    if (line[i] is blank) {
        shiftDownOfTopRow--;  // 可以少移动一行
        if (shiftDownOfTopRow == 0) break;
    }
}
```

**目的**：如果底部有空行，可以减少滚动，保留更多历史内容。

### 3. 扩大限制（历史记录限制）

```java
actualShift = max(shiftDownOfTopRow, -activeTranscriptRows)
```

**目的**：不能上移超过历史记录的起始位置。

### 4. first_row 更新

```java
mScreenFirstRow += shiftDownOfTopRow;
mScreenFirstRow = (mScreenFirstRow < 0) 
    ? (mScreenFirstRow + mTotalRows)  // 处理负数
    : (mScreenFirstRow % mTotalRows);  // 环形缓冲区
```

### 5. active_transcript_rows 更新

```java
mActiveTranscriptRows = altScreen 
    ? 0  // 备用屏幕没有历史
    : max(0, mActiveTranscriptRows + shiftDownOfTopRow);
```

**逻辑**：
- 缩小（shift > 0）：历史记录增加
- 扩大（shift < 0）：历史记录减少

### 6. 光标调整

```java
cursor[1] -= shiftDownOfTopRow;
```

**逻辑**：光标随行数变化同步移动。

---

## Rust 当前实现 vs Java 实现

### Rust 当前实现（简化版）

```rust
// 当前 Rust 实现
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
self.first_row = self.active_transcript_rows % self.buffer.len();
```

**问题**：
1. 没有快速路径，总是重建缓冲区
2. `active_transcript_rows` 计算方式不同
3. 没有空行跳过优化
4. 没有扩大限制检查

### Java 实现（完整逻辑）

```java
// Java 快速路径
int shift = oldRows - newRows;  // 核心公式

// 空行优化（缩小）
if (shift > 0) {
    for (i = oldRows - 1; i > 0 && cursorRow < i; i--) {
        if (line[i] is blank) shift--;
    }
}

// 历史记录限制（扩大）
if (shift < 0) {
    shift = max(shift, -activeTranscriptRows);
}

// 应用
firstRow = (firstRow + shift) % totalRows;
activeTranscriptRows = max(0, activeTranscriptRows + shift);
cursorY -= shift;
```

---

## 复刻版本

已创建 `screen_java_style.rs`，包含完整的 Java 风格实现：

```rust
pub fn resize_with_reflow_java_style(
    &mut self,
    new_cols: i32,
    new_rows: i32,
    new_total_rows: usize,
    current_style: u64,
    cursor_x: i32,
    cursor_y: i32,
    alt_screen: bool,
) -> (i32, i32)
```

### 关键特性

1. **快速路径**：当列数不变时，只调整指针
2. **空行优化**：缩小检查底部空行
3. **历史限制**：扩大不超过历史记录
4. **环形缓冲区**：正确处理 `first_row`  wraparound
5. **光标追踪**：同步调整光标位置

---

## 测试对比

### 测试用例 1：缩小（80x24 → 80x12）

**Java 行为**：
```
shift = 24 - 12 = 12
检查底部 12 行是否有空行
如果有 5 个空行：shift = 12 - 5 = 7
first_row += 7
active_transcript_rows += 7
cursor_y -= 7
```

**Rust 当前行为**：
```
重建整个缓冲区
output_row = 实际写入行数
active_transcript_rows = output_row - 12
first_row = active_transcript_rows % total
```

### 测试用例 2：扩大（80x12 → 80x24）

**Java 行为**：
```
shift = 12 - 24 = -12
actual_shift = max(-12, -active_transcript_rows)
如果 active_transcript_rows = 100:
  actual_shift = -12
如果 active_transcript_rows = 5:
  actual_shift = -5  (不能上移超过历史记录)
first_row += actual_shift
active_transcript_rows += actual_shift
cursor_y -= actual_shift
```

**Rust 当前行为**：
```
重建整个缓冲区
output_row = 实际写入行数
active_transcript_rows = output_row - 24
```

---

## 建议

### 短期修复

1. **添加快速路径**：
   ```rust
   if new_cols == old_cols {
       return self.resize_fast_java_style(new_rows, cursor_x, cursor_y, alt_screen);
   }
   ```

2. **修正 first_row 计算**：
   ```rust
   // 当前
   self.first_row = self.active_transcript_rows % self.buffer.len();
   
   // 应该
   self.first_row = (self.first_row as i32 + shift) % total_rows;
   ```

3. **添加空行优化**：
   ```rust
   for i in (0..old_rows).rev() {
       if cursor_y >= i { break; }
       if self.get_row(i as i32).is_blank() {
           shift -= 1;
       }
   }
   ```

### 长期方案

完全替换为 Java 风格实现：
```rust
// 在 screen.rs 中
pub fn resize(&mut self, new_cols: i32, new_rows: i32, new_total_rows: usize) {
    let style = self.current_style;
    let cx = self.cursor.x;
    let cy = self.cursor.y;
    let alt = self.use_alternate_buffer;
    
    let (new_cx, new_cy) = self.resize_with_reflow_java_style(
        new_cols, new_rows, new_total_rows, style, cx, cy, alt
    );
    
    self.cursor.x = new_cx;
    self.cursor.y = new_cy;
}
```

---

## 验证步骤

1. **单元测试**：
   ```bash
   cargo test resize
   ```

2. **对比测试**：
   ```bash
   # 运行 Java 版本
   ./gradlew test --tests "*Resize*"
   
   # 运行 Rust 版本
   cargo test --test consistency test_resize
   ```

3. **手动测试**：
   ```bash
   # 产生多行输出
   seq 1 500
   
   # 调整窗口大小
   # 观察内容是否丢失
   ```
