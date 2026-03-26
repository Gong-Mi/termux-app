# 屏幕外内容丢失问题分析

**分析日期**: 2026-03-26  
**问题描述**: 屏幕外内容会丢失行内容，屏幕高度扩大时显示之后的行但已丢失的不会显示

---

## 问题现象

1. **屏幕外内容丢失** - 滚动历史中的某些行内容丢失
2. **扩大屏幕时显示异常** - 屏幕高度扩大时显示"之后的行"，但已丢失的内容不显示

---

## 可能的原因

### 原因 1: `active_transcript_rows` 计算不准确 ⚠️

**Rust 实现**:
```rust
// screen.rs - resize_with_reflow 结束时
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
self.first_row = self.active_transcript_rows % self.buffer.len();
```

**Java 实现**:
```java
// TerminalBuffer.java - resize() 快速路径
mActiveTranscriptRows = altScreen ? 0 : Math.max(0, mActiveTranscriptRows + shiftDownOfTopRow);
```

**差异**:
- Rust 通过 `output_row` **重新计算**
- Java **增量维护** `active_transcript_rows`

**可能导致的问题**:
- `output_row` 追踪不准确时，`active_transcript_rows` 会错误
- 特别是跳过空行时，`output_row` 可能没有正确反映实际写入的行数

---

### 原因 2: `resize_rows_only` 中的边界条件 ⚠️

**Rust 实现**:
```rust
// screen.rs - resize_rows_only()
// Update active_transcript_rows
let shift_i32 = shift_down_of_top_row;
self.active_transcript_rows = if shift_i32 > 0 {
    // Shrinking: increase transcript rows
    self.active_transcript_rows + shift_i32 as usize
} else {
    // Expanding: decrease transcript rows
    self.active_transcript_rows.saturating_sub((-shift_i32) as usize)
};
```

**Java 实现**:
```java
// TerminalBuffer.java - resize() 快速路径
mActiveTranscriptRows = altScreen ? 0 : Math.max(0, mActiveTranscriptRows + shiftDownOfTopRow);
```

**对比**:
- Rust: `active_transcript_rows + shift` (shrinking)
- Java: `active_transcript_rows + shiftDownOfTopRow`

**看起来一致** ✅，但需要验证边界条件。

---

### 原因 3: `get_transcript_text` 遍历范围错误 ⚠️

**Rust 实现**:
```rust
// screen.rs
pub fn get_transcript_text(&self) -> String {
    let mut res = String::new();
    let first_y = -(self.active_transcript_rows as i32);
    for y in first_y..self.rows {
        let row = self.get_row(y);
        res.push_str(&row.get_selected_text(0, row.get_space_used()));
        if !row.line_wrap && y < self.rows - 1 { res.push('\n'); }
    }
    res
}
```

**Java 实现**:
```java
// TerminalBuffer.java
public String getTranscriptText() {
    return getSelectedText(0, -getActiveTranscriptRows(), mColumns, mScreenRows).trim();
}

public String getSelectedText(int selX1, int selY1, int selX2, int selY2) {
    // ...
    if (selY1 < -getActiveTranscriptRows()) selY1 = -getActiveTranscriptRows();
    if (selY2 >= mScreenRows) selY2 = mScreenRows - 1;
    
    for (int row = selY1; row <= selY2; row++) {
        TerminalRow lineObject = mLines[externalToInternalRow(row)];
        // ...
    }
}
```

**对比**:
- Rust: `for y in first_y..self.rows` → 范围 `[-active_transcript_rows, rows-1]`
- Java: `for row = selY1 to selY2` → 范围 `[-active_transcript_rows, mScreenRows-1]`

**一致** ✅

---

### 原因 4: `resize_with_reflow` 中 `output_row` 追踪错误 🔴

**问题分析**:

```rust
// screen.rs - resize_with_reflow
let mut output_row: usize = 0;
let mut output_col: usize = 0;

for external_old_row in start_row..end_row {
    // ...
    
    // Insert skipped blank lines
    if skipped_blank_lines > 0 {
        for _ in 0..skipped_blank_lines {
            if output_row >= old_total - 1 {
                // Scroll...
            } else {
                output_row += 1;  // ← 这里增加 output_row
            }
            output_col = 0;
        }
        skipped_blank_lines = 0;
    }
    
    // Process characters...
    for i in 0..last_non_space_index {
        // ...
        if output_col + display_width as usize > n_cols {
            // Line wrap
            if output_row < new_buffer.len() {
                new_buffer[output_row].line_wrap = true;
            }
            if output_row >= old_total - 1 {
                // Scroll...
            } else {
                output_row += 1;  // ← 这里增加 output_row
            }
            output_col = 0;
        }
        // ...
    }
    
    // 行结束后增加 output_row
    if external_old_row != (old_rows as i32 - 1) && !old_line.line_wrap {
        if output_row >= old_total - 1 {
            // Scroll...
        } else {
            output_row += 1;  // ← 这里增加 output_row
        }
        output_col = 0;
    }
}

// 计算 active_transcript_rows
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
```

**潜在问题**:

1. **`output_row` 可能超过 `new_rows`** - 当内容很多时，`output_row` 会一直增加
2. **`total_written = output_row + 1` 可能不准确** - 因为 `output_row` 是从 0 开始的索引，不是实际写入的行数
3. **跳过空行逻辑可能导致 `output_row` 过大** - 每次插入跳过的空行都会增加 `output_row`

---

### 原因 5: `resize_rows_only` 中 expanding 场景处理错误 🔴

**Rust 实现**:
```rust
// screen.rs - resize_rows_only()
} else if shift_down_of_top_row < 0 {
    // Expanding: only move screen up if there's transcript to show
    let actual_shift = std::cmp::max(shift_down_of_top_row, -(self.active_transcript_rows as i32));

    if shift_down_of_top_row != actual_shift {
        // The new lines revealed by resizing are not all from transcript
        // Blank the below ones
        let blank_count = actual_shift - shift_down_of_top_row;
        for i in 0..blank_count {
            let row_to_clear = (self.first_row + old_rows + i as usize) % self.buffer.len();
            self.buffer[row_to_clear].clear_all(current_style);
        }
        shift_down_of_top_row = actual_shift;
    }
}
```

**Java 实现**:
```java
// TerminalBuffer.java - resize() 快速路径
} else if (shiftDownOfTopRow < 0) {
    // Negative shift down = expanding.
    // Only move screen up if there is transcript to show:
    int actualShift = Math.max(shiftDownOfTopRow, -mActiveTranscriptRows);
    if (shiftDownOfTopRow != actualShift) {
        // The new lines revealed by the resizing are not all from the transcript.
        // Blank the below ones.
        for (int i = 0; i < actualShift - shiftDownOfTopRow; i++)
            allocateFullLineIfNecessary((mScreenFirstRow + mScreenRows + i) % mTotalRows).clear(currentStyle);
        shiftDownOfTopRow = actualShift;
    }
}
```

**对比**:
- Rust: `row_to_clear = (first_row + old_rows + i) % buffer.len()`
- Java: `(mScreenFirstRow + mScreenRows + i) % mTotalRows`

**问题**: 
- Rust 使用 `old_rows` (旧的可见行数)
- Java 使用 `mScreenRows` (也是旧的可见行数)
- **但 Rust 在 expanding 后已经更新了 `self.rows = new_rows`**，所以应该用新的行数！

---

## 测试验证

### 测试场景 1: 写入内容后缩小再扩大

```rust
// 1. 写入 100 行到 24 行屏幕
for i in 1..=100 {
    println!("Line {}", i);
}
// 此时候：active_transcript_rows = 76, first_row = 76

// 2. 缩小到 12 行
// Java: shift = 24 - 12 = 12
//       active_transcript_rows = 76 + 12 = 88
//       first_row = (76 + 12) % 1000 = 88
// Rust: 应该相同

// 3. 扩大到 48 行
// Java: shift = 12 - 48 = -36
//       actual_shift = max(-36, -88) = -36
//       active_transcript_rows = 88 - 36 = 52
//       first_row = (88 - 36) % 1000 = 52
// Rust: 应该相同
```

### 测试场景 2: 边界条件验证

```rust
// 1. 写入 50 行到 24 行屏幕
// active_transcript_rows = 26

// 2. 扩大到 48 行
// shift = 24 - 48 = -24
// actual_shift = max(-24, -26) = -24
// active_transcript_rows = 26 - 24 = 2
// first_row = (26 - 24) % 1000 = 2

// 3. 读取历史行 -1 和 -2
// 应该还能看到内容
```

---

## 修复建议

### 修复 1: 修正 `resize_rows_only` 中的行清除逻辑

```rust
// 当前代码（错误）
for i in 0..blank_count {
    let row_to_clear = (self.first_row + old_rows + i as usize) % self.buffer.len();
    self.buffer[row_to_clear].clear_all(current_style);
}

// 修复后
for i in 0..blank_count {
    // 使用 old_rows 而不是 self.rows，因为 self.rows 已经更新为 new_rows
    let row_to_clear = (self.first_row + new_rows as usize - blank_count as usize + i) % self.buffer.len();
    self.buffer[row_to_clear].clear_all(current_style);
}
```

### 修复 2: 验证 `output_row` 计算

添加调试日志：
```rust
// resize_with_reflow 结束时
println!("DEBUG: output_row={}, new_rows={}, total_written={}, active_transcript_rows={}", 
         output_row, new_rows, total_written, self.active_transcript_rows);
```

### 修复 3: 添加边界检查

```rust
// get_row() 中添加检查
pub fn get_row(&self, row: i32) -> &TerminalRow {
    let max_history = -(self.active_transcript_rows as i32);
    let row = row.max(max_history).min(self.rows as i32 - 1);
    &self.buffer[self.internal_row(row)]
}
```

---

## 下一步行动

1. **添加调试日志** - 验证 `active_transcript_rows` 和 `first_row` 的值
2. **编写复现测试** - 创建具体的测试用例复现问题
3. **对比 Java 行为** - 在相同操作下对比 Java 和 Rust 的值
4. **修复并验证** - 根据发现的问题修复并验证
