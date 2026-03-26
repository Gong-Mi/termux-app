# active_transcript_rows 计算错误修复

**分析日期**: 2026-03-26  
**问题**: `resize_with_reflow` 中 `active_transcript_rows` 计算错误

---

## 问题根源

### 错误的计算逻辑

```rust
// screen.rs - resize_with_reflow (slow path, line 494)
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
```

**问题**：`output_row` 是**当前输出位置的索引**，不是**实际写入的总行数**！

### 场景分析

**场景**：80x10 写入 20 行，然后 resize 到 80x18

**写入过程**：
```
行 0-9:  输出到 output_row 0-9 (可见区域)
行 10:   滚动，output_row 保持在 9 (环形缓冲区)
行 11-19: 输出到 output_row 0-8 (覆盖旧数据)

最终：output_row = 8 (最后一次写入的位置)
```

**resize 计算**：
```rust
total_written = output_row + 1 = 9  // ❌ 错误！实际写入了 20 行
active_transcript_rows = 9 - 18 = 0  // ❌ 应该是 2！
```

**正确计算**：
```
实际写入 20 行，可见 18 行
active_transcript_rows = 20 - 18 = 2
```

---

## 修复方案

### 方案 1：追踪实际写入行数

```rust
// resize_with_reflow 开始时
let mut total_chars_written = 0;

// 每次写入字符时
total_chars_written += 1;

// 计算 active_transcript_rows
let total_lines_written = (total_chars_written / new_cols as usize) + 1;
self.active_transcript_rows = total_lines_written.saturating_sub(new_rows as usize);
```

### 方案 2：使用 Java 的增量维护方式

```rust
// 不在 resize 时重新计算
// 而是在每次 scroll_up 时增量维护
pub fn scroll_up(&mut self, top: i32, bottom: i32, style: u64) {
    if top == 0 && bottom == self.rows {
        self.first_row = (self.first_row + 1) % self.buffer.len();
        // ✅ 增量维护
        if self.active_transcript_rows < self.buffer.len() - self.rows as usize {
            self.active_transcript_rows += 1;
        }
        // ...
    }
    // ...
}
```

### 方案 3：resize 时重建 active_transcript_rows

```rust
// resize_with_reflow 结束时
// 从缓冲区中实际计算有多少行有内容
let mut last_non_empty_row = 0;
for (i, row) in self.buffer.iter().enumerate() {
    if row.get_space_used() > 0 {
        last_non_empty_row = i;
    }
}

// 计算 active_transcript_rows
let total_lines = /* 从 first_row 到 last_non_empty_row 的距离 */;
self.active_transcript_rows = total_lines.saturating_sub(new_rows as usize);
```

---

## 推荐修复

**使用方案 2**：增量维护，与 Java 行为一致

### 修改点

1. **移除 `resize_with_reflow` 中的重新计算**
2. **确保 `scroll_up` 正确增量维护**
3. **初始化时设置正确的值**

### 代码修改

```rust
// screen.rs - resize_with_reflow (slow path)
// 删除这段代码：
/*
let total_written = output_row + 1;
self.active_transcript_rows = total_written.saturating_sub(new_rows as usize);
self.first_row = self.active_transcript_rows % self.buffer.len();
*/

// 替换为：
// active_transcript_rows 已经在 scroll_up 中增量维护
// resize 时只需要调整 first_row
let shift = old_rows as i32 - new_rows as i32;
self.first_row = ((self.first_row as i32 + shift) % self.buffer.len() as i32 + 
                  self.buffer.len() as i32) as usize % self.buffer.len();
if shift > 0 {
    // Shrinking: increase transcript rows
    self.active_transcript_rows = (self.active_transcript_rows as i32 + shift) as usize;
} else {
    // Expanding: decrease transcript rows
    self.active_transcript_rows = self.active_transcript_rows.saturating_sub((-shift) as usize);
}
```

---

## 验证测试

```rust
#[test]
fn test_active_transcript_rows_calculation() {
    // 1. 创建 80x10 屏幕
    let mut engine = TerminalEngine::new(80, 10, 1000, 10, 20);
    
    // 2. 写入 20 行
    for i in 1..=20 {
        engine.process_bytes(format!("Line {}\r\n", i).as_bytes());
    }
    
    // 3. 验证 active_transcript_rows
    assert_eq!(engine.state.main_screen.active_transcript_rows, 11);  // 20 - 10 + 1 (cursor row)
    
    // 4. 扩大到 80x18
    engine.state.main_screen.resize_with_reflow(80, 18, 0, 0, 9);
    engine.state.main_screen.rows = 18;
    
    // 5. 验证 active_transcript_rows
    assert_eq!(engine.state.main_screen.active_transcript_rows, 2);  // 20 - 18 = 2
}
```

---

## 结论

**根本原因**：`resize_with_reflow` 使用 `output_row` 计算 `active_transcript_rows`，但 `output_row` 是索引不是计数。

**修复方法**：使用增量维护，与 Java 行为一致。

**影响范围**：
- 只影响 `resize_with_reflow` (slow path)
- `resize_rows_only` (fast path) 已经正确
