# Java 与 Rust 终端实现不等价分析

## 概述

本文档详细分析 Termux 终端模拟器从 Java 迁移到 Rust 实现过程中，**尚未完全等价**的功能和潜在问题。

---

## 1. 环形缓冲区实现差异

### Java 实现 (`TerminalBuffer.java`)

```java
// 环形缓冲区索引转换
public int externalToInternalRow(int externalRow) {
    if (externalRow < -mActiveTranscriptRows || externalRow > mScreenRows)
        throw new IllegalArgumentException(...);
    final int internalRow = mScreenFirstRow + externalRow;
    return (internalRow < 0) ? (mTotalRows + internalRow) : (internalRow % mTotalRows);
}
```

**特点**：
- `externalRow` 范围：`[-mActiveTranscriptRows, mScreenRows-1]`
- 屏幕行 0 对应内部 `mScreenFirstRow`
- 历史行 -1, -2, ... 对应 `mScreenFirstRow-1, mScreenFirstRow-2, ...`

### Rust 实现 (`screen.rs`)

```rust
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

**特点**：
- 数学上等价，但使用双重取模处理负数
- `row` 范围：`[-active_transcript_rows, rows-1]`

### ⚠️ 潜在问题

**`first_row` 设置逻辑可能不一致**：

```rust
// Rust resize_with_reflow 结束时
self.first_row = self.active_transcript_rows % self.buffer.len();
```

Java 的 `mScreenFirstRow` 在 resize 时通过复杂逻辑计算，而 Rust 简单设置为 `active_transcript_rows`。

**影响**：在某些边界情况下，历史行的索引可能偏移。

---

## 2. resize_with_reflow 滚动逻辑

### Java 实现

```java
// TerminalBuffer.resize() - 快速路径（仅行数变化）
if (newColumns == mColumns && newRows <= mTotalRows) {
    int shiftDownOfTopRow = mScreenRows - newRows;
    // ... 计算实际的 shift
    mScreenFirstRow += shiftDownOfTopRow;
    mScreenFirstRow = (mScreenFirstRow < 0) ? (mScreenFirstRow + mTotalRows) 
                                             : (mScreenFirstRow % mTotalRows);
    mTotalRows = newTotalRows;
    mActiveTranscriptRows = altScreen ? 0 : Math.max(0, mActiveTranscriptRows + shiftDownOfTopRow);
    cursor[1] -= shiftDownOfTopRow;
    mScreenRows = newRows;
}
```

**特点**：
- 快速路径只调整指针，不移动数据
- `mActiveTranscriptRows` 通过 `shiftDownOfTopRow` 计算

### Rust 实现

```rust
// screen.rs - resize_with_reflow
// 总是重建整个缓冲区
let mut new_buffer: Vec<TerminalRow> = Vec::with_capacity(old_total);
for _ in 0..old_total {
    new_buffer.push(TerminalRow::new(n_cols));
}

// 逐字符复制并处理重排
for external_old_row in start_row..end_row {
    // ... 逐字符处理
}

// 计算 active_transcript_rows
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
self.first_row = self.active_transcript_rows % self.buffer.len();
```

**特点**：
- 总是重建缓冲区（慢路径）
- `active_transcript_rows` 通过 `output_row` 计算

### ⚠️ 潜在问题

1. **性能差异**：Rust 版本总是 O(n) 复制，Java 有 O(1) 快速路径
2. **计算方式不同**：Java 用 `shift`，Rust 用 `output_row`，极端情况下结果可能不同
3. **内容丢失风险**：Rust 的 `output_row` 追踪可能不准确

---

## 3. 滚动实现差异

### Java 实现

```java
// TerminalBuffer.scrollDownOneLine()
public void scrollDownOneLine(int topMargin, int bottomMargin, long style) {
    // 复制固定顶部行
    blockCopyLinesDown(mScreenFirstRow, topMargin);
    // 复制固定底部行
    blockCopyLinesDown(externalToInternalRow(bottomMargin), mScreenRows - bottomMargin);
    
    // 更新屏幕位置
    mScreenFirstRow = (mScreenFirstRow + 1) % mTotalRows;
    
    // 增加历史记录
    if (mActiveTranscriptRows < mTotalRows - mScreenRows) 
        mActiveTranscriptRows++;
    
    // 清空新暴露的行
    int blankRow = externalToInternalRow(bottomMargin - 1);
    mLines[blankRow].clear(style);
}
```

**特点**：
- 使用 `blockCopyLinesDown` 在环形缓冲区中复制
- 只移动 `mScreenFirstRow` 指针
- 不移动实际数据

### Rust 实现

```rust
// screen.rs - scroll_up
pub fn scroll_up(&mut self, top: i32, bottom: i32, style: u64) {
    let c = self.cols as usize;
    if top == 0 && bottom == self.rows {
        // 全屏滚动：移动 first_row 指针
        self.first_row = (self.first_row + 1) % self.buffer.len();
        if self.active_transcript_rows < self.buffer.len() - self.rows as usize { 
            self.active_transcript_rows += 1; 
        }
        self.get_row_mut(self.rows - 1).clear(0, c, style);
    } else {
        // 部分滚动：实际移动数据
        for i in top..(bottom - 1) {
            let s = self.internal_row(i + 1);
            let d = self.internal_row(i);
            self.buffer[d] = self.buffer[s].clone();
        }
        self.get_row_mut(bottom - 1).clear(0, c, style);
    }
}
```

**特点**：
- 全屏滚动时与 Java 等价
- 部分滚动时实际移动数据（Java 也用类似逻辑）

### ✅ 基本等价

全屏滚动逻辑基本等价，但部分滚动的边界条件可能需要验证。

---

## 4. 字符宽度计算

### Java 实现

```java
// WcWidth.java
public static int width(int codePoint) {
    if (codePoint < 0x20 || (codePoint >= 0x7f && codePoint < 0xa0)) 
        return -1;
    if (codePoint < 0x7f) 
        return 1;
    // ... 复杂的双宽度字符表查找
    return wcwidth(codePoint);
}
```

### Rust 实现

```rust
// screen.rs
#[inline]
fn local_get_width(code_point: u32) -> i32 {
    if code_point < 0x20 || (code_point >= 0x7f && code_point < 0xa0) { 
        return -1; 
    }
    if code_point < 0x7f { 
        return 1; 
    }
    // 使用 unicode-width crate
    unicode_width::UnicodeWidthChar::width(code_point).unwrap_or(0) as i32
}
```

### ⚠️ 潜在问题

1. **算法差异**：Java 使用自定义表，Rust 使用 `unicode-width` crate
2. **边界情况**：某些罕见 Unicode 字符可能计算结果不同
3. **组合字符**：处理方式可能不同

---

## 5. 样式处理

### Java 实现

```java
// TerminalRow.java
public char[] mText;      // 字符数组
public long[] mStyle;     // 样式数组（并行数组）

public void setChar(int x, char c, long style) {
    mText[x] = c;
    mStyle[x] = style;
}
```

### Rust 实现

```rust
// screen.rs - TerminalRow
pub struct TerminalRow {
    pub text: Vec<char>,
    pub styles: Vec<u64>,
    pub line_wrap: bool,
}

// 并行数组，与 Java 等价
```

### ✅ 基本等价

结构和处理方式基本相同。

---

## 6. 换行符处理

### Java 实现

```java
// TerminalBuffer.resize()
if (cursorAtThisRow || oldLine.mLineWrap) {
    lastNonSpaceIndex = oldLine.getSpaceUsed();
} else {
    // 找到最后一个非空格字符
    for (int i = 0; i < oldLine.getSpaceUsed(); i++)
        if (oldLine.mText[i] != ' ' || oldLine.mStyle[i] != currentStyle)
            lastNonSpaceIndex = i + 1;
}
```

### Rust 实现

```rust
// screen.rs
let last_non_space_index = if cursor_at_this_row || old_line.line_wrap {
    old_line.text.len()
} else {
    old_line.get_space_used()
};
```

### ⚠️ 潜在问题

Rust 版本简化了逻辑，**没有检查样式变化**。这可能导致：
- 尾部空格被错误保留
- 样式变化处的截断不准确

---

## 7. 组合字符处理

### Java 实现

```java
// TerminalBuffer.resize()
int offsetDueToCombiningChar = ((displayWidth <= 0 && currentOutputExternalColumn > 0) ? 1 : 0);
int outputColumn = currentOutputExternalColumn - offsetDueToCombiningChar;
setChar(outputColumn, currentOutputExternalRow, codePoint, styleAtCol);
```

### Rust 实现

```rust
// screen.rs
let offset = if display_width <= 0 && output_col > 0 { 1 } else { 0 };
let output_column = output_col.saturating_sub(offset);

if output_column < n_cols && output_row < new_buffer.len() {
    new_buffer[output_row].text[output_column] = c;
    new_buffer[output_row].styles[output_column] = style_at_col;
}
```

### ✅ 基本等价

逻辑相同，但 Rust 使用 `saturating_sub` 更安全。

---

## 8. 光标处理

### Java 实现

```java
// TerminalBuffer.resize()
if (cursorAtThisRow && currentOldCol == oldCursorColumn) {
    newCursorColumn = currentOutputExternalColumn;
    newCursorRow = currentOutputExternalRow;
    newCursorPlaced = true;
}

// 最后检查
if (newCursorColumn < 0 || newCursorRow < 0) 
    newCursorColumn = newCursorRow = 0;
```

### Rust 实现

```rust
// screen.rs
if cursor_at_this_row && current_old_col == cursor_x as usize && !cursor_placed {
    new_cursor_x = output_col as i32;
    new_cursor_y = output_row as i32;
    cursor_placed = true;
}

// 最后检查
if !cursor_placed || new_cursor_x < 0 || new_cursor_y < 0 {
    new_cursor_x = 0;
    new_cursor_y = 0;
}
```

### ⚠️ 潜在问题

Rust 多了 `!cursor_placed` 检查，这可能导致：
- 光标位置重置过于激进
- 在某些边界情况下光标跳到 (0,0)

---

## 9. 空行跳过逻辑

### Java 实现

```java
// TerminalBuffer.resize()
if (oldLine == null || (!(!newCursorPlaced && cursorAtThisRow)) && oldLine.isBlank()) {
    skippedBlankLines++;
    continue;
} else if (skippedBlankLines > 0) {
    // 遇到非空行，插入跳过的空行
    for (int i = 0; i < skippedBlankLines; i++) {
        if (currentOutputExternalRow == mScreenRows - 1) {
            scrollDownOneLine(0, mScreenRows, currentStyle);
        } else {
            currentOutputExternalRow++;
        }
        currentOutputExternalColumn = 0;
    }
    skippedBlankLines = 0;
}
```

### Rust 实现

```rust
// screen.rs
let is_blank = {
    let used = old_line.get_space_used();
    used == 0 || (0..used).all(|i| old_line.text[i] == ' ')
};

if is_blank && !cursor_at_this_row {
    skipped_blank_lines += 1;
    continue;
}

if skipped_blank_lines > 0 {
    for _ in 0..skipped_blank_lines {
        if output_row >= old_total - 1 {
            // 滚动...
        } else {
            output_row += 1;
        }
        output_col = 0;
    }
    skipped_blank_lines = 0;
}
```

### ⚠️ 潜在问题

1. **空行判断不同**：Java 检查 `oldLine == null`，Rust 不检查
2. **滚动阈值不同**：Java 用 `mScreenRows - 1`，Rust 用 `old_total - 1`
3. **滚动行为不同**：Java 调用 `scrollDownOneLine`（移动指针），Rust 移动数据

---

## 10. 测试覆盖差异

### Java 测试

- 经过多年生产环境验证
- 大量用户实际使用
- 边界情况已被发现并修复

### Rust 测试

```bash
# 当前测试覆盖
cargo test --lib                    # 4 个基础测试
cargo test --test consistency       # 121 个一致性测试
cargo test --test fix_verification  # 15 个修复验证测试
cargo test --test reflow_600_lines  # 1 个 600 行重排测试
```

**缺失的测试**：
- 长时间运行稳定性
- 极端 Unicode 字符处理
- 真实用户交互场景
- 与其他应用的互操作性

---

## 总结：关键不等价点

| 功能 | Java | Rust | 状态 |
|------|------|------|------|
| 环形缓冲区索引 | `externalToInternalRow` | `internal_row` | ✅ 等价 |
| resize 快速路径 | 有（仅调整指针） | 无（总是重建） | ⚠️ 性能差异 |
| 滚动（全屏） | 移动指针 | 移动指针 | ✅ 等价 |
| 滚动（部分） | 移动数据 | 移动数据 | ✅ 等价 |
| 字符宽度 | 自定义表 | unicode-width crate | ⚠️ 可能差异 |
| 样式处理 | 并行数组 | 并行数组 | ✅ 等价 |
| 换行符处理 | 检查样式变化 | 不检查样式 | ⚠️ 可能差异 |
| 组合字符 | saturating_sub | saturating_sub | ✅ 等价 |
| 光标处理 | 基本检查 | 额外检查 | ⚠️ 可能过于激进 |
| 空行判断 | 检查 null | 不检查 null | ⚠️ 可能差异 |
| 滚动阈值 | `mScreenRows - 1` | `old_total - 1` | ⚠️ 可能差异 |

---

## 建议的验证步骤

### 1. 自动化测试

```bash
# 运行所有 Rust 测试
cargo test

# 运行 Java 测试（如果有）
./gradlew test

# 对比结果
```

### 2. 手动测试

```bash
# 测试 1：基本滚动
seq 1 1000
# 向上滚动查看第 1 行

# 测试 2：resize
seq 1 500
# 调整窗口大小
# 确认内容不丢失

# 测试 3：Unicode
echo "测试テスト테스트🔥"
# 确认显示正确
```

### 3. 日志分析

```bash
# 查看 Rust 日志
logcat -s Termux:* Rust:*

# 对比 Java 版本日志
```

---

## 修复优先级

### 高优先级（影响正常使用）

1. **`first_row` 计算逻辑** - 可能导致历史行索引错误
2. **空行跳过滚动阈值** - 可能导致内容丢失
3. **光标处理** - 可能导致光标位置错误

### 中优先级（边界情况）

1. **字符宽度计算** - 罕见字符可能显示错误
2. **换行符样式检查** - 尾部空格处理可能不同

### 低优先级（性能优化）

1. **resize 快速路径** - 可以添加性能优化
2. **测试覆盖** - 增加更多边界情况测试

---

## 结论

Rust 实现在**核心逻辑**上与 Java 等价，但在**边界条件**和**细节处理**上存在差异。这些差异在大多数使用场景下不会显现，但在极端情况下可能导致显示问题。

**建议**：
1. 优先修复高优先级问题
2. 增加真实用户场景的测试
3. 在生产环境中逐步验证
