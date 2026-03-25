# 滚动显示问题分析报告

## 问题描述

用户报告："内容不会显示到屏幕外面"

## 测试结果

### Rust 侧测试 ✅ 通过

```rust
// tests/content_overflow_test.rs
After writing 100 lines:
  active_transcript=77  ✓ 正确（100-24=76，约等于 77）

Screen content (rows 0-23):
  [0]: Line 78
  [1]: Line 79
  ...
  [22]: Line 100  ✓ Line 100 在屏幕上
  [23]: (empty)

History content (sample):
  [-77]: Line 1  ✓ Line 1 在历史中
  [-67]: Line 11
  ...
```

**结论**：Rust 侧**正确**地将超出屏幕的内容写入历史缓冲区。

---

## Java 侧逻辑分析

### 1. `onScreenUpdated()` 行为

```java
public void onScreenUpdated(boolean skipScrolling) {
    int rowsInHistory = mEmulator.getActiveTranscriptRows();
    
    if (isSelectingText() || mEmulator.isAutoScrollDisabled()) {
        // 选择文本或禁用自动滚动时：保持当前位置
        skipScrolling = true;
    }
    
    if (!skipScrolling && mTopRow != 0) {
        // 自动滚动到底部
        if (mTopRow < -3) {
            awakenScrollBars();  // 显示滚动条动画
        }
        mTopRow = 0;  // ← 重置到底部
    }
    
    invalidate();  // 重新渲染
}
```

**行为**：
- 每次屏幕更新后，**自动滚动到底部**（`mTopRow = 0`）
- 除非用户正在选择文本或禁用了自动滚动

### 2. 渲染逻辑

```java
public final void render(TerminalEmulator mEmulator, Canvas canvas, int topRow, ...) {
    final int rows = mEmulator.getRows();
    final int endRow = topRow + rows;
    
    for (int row = topRow; row < endRow; row++) {
        mEmulator.readRow(row, ...);  // 读取并渲染
    }
}
```

**渲染范围**：`[topRow, topRow + rows)`

- 当 `topRow = 0`：渲染屏幕行 0-23
- 当 `topRow = -50`：渲染历史行 -50 到屏幕行 -27

### 3. 滚动条计算

```java
@Override
protected int computeVerticalScrollRange() {
    return mEmulator.getActiveRows();  // 历史 + 屏幕
}

@Override
protected int computeVerticalScrollOffset() {
    return mEmulator.getActiveRows() + mTopRow - mEmulator.getRows();
}
```

**示例**：
- `active_rows = 100`, `rows = 24`, `mTopRow = 0`
  - `scroll_range = 100`
  - `scroll_offset = 100 + 0 - 24 = 76`（在底部）
- `mTopRow = -50`（向上滚动 50 行）
  - `scroll_offset = 100 + (-50) - 24 = 26`（在中间）

---

## 问题定位

### 可能的原因

1. **滚动条不显示**
   - 可能 Android 系统认为内容不需要滚动
   - 检查 `computeVerticalScrollRange()` 和 `computeVerticalScrollExtent()`

2. **`mTopRow` 无法手动修改**
   - 用户向上滑动时，`mTopRow` 应该变为负值
   - 检查触摸事件处理

3. **`active_transcript_rows` 返回 0**
   - Rust 侧计算错误
   - 但测试显示是正确的

### 需要检查的地方

1. **`TerminalView` 的触摸处理**
   ```java
   @Override
   public boolean onTouchEvent(MotionEvent event) {
       // 检查是否处理了垂直滑动
       // 是否更新了 mTopRow
   }
   ```

2. **`doScroll()` 方法**
   ```java
   void doScroll(MotionEvent event, int rowsDown) {
       if (mEmulator.isMouseTrackingActive()) {
           // 发送鼠标事件
       } else if (mEmulator.isAlternateBufferActive()) {
           // 备用缓冲区
       } else {
           mTopRow = Math.min(0, Math.max(-(mEmulator.getScreen().getActiveTranscriptRows()), 
                                          mTopRow + (up ? -1 : 1)));
           // ← 这里应该能更新 mTopRow
       }
   }
   ```

3. **`computeVerticalScrollExtent()`**
   ```java
   @Override
   protected int computeVerticalScrollExtent() {
       return mEmulator == null ? 1 : mEmulator.mRows;
   }
   ```
   - 返回屏幕行数
   - 如果 `scroll_extent >= scroll_range`，滚动条会隐藏

---

## 诊断步骤

### 1. 检查滚动条是否启用

```java
// TerminalView.java
setVerticalScrollBarEnabled(true);  // 应该启用
```

### 2. 添加日志

```java
// onScreenUpdated()
Log.d("Termux-Scroll", "rowsInHistory=" + rowsInHistory + 
      ", mTopRow=" + mTopRow + ", skipScrolling=" + skipScrolling);

// doScroll()
Log.d("Termux-Scroll", "doScroll: rowsDown=" + rowsDown + 
      ", new mTopRow=" + mTopRow);
```

### 3. 测试手动滚动

```bash
# 在 Termux 中
adb shell input swipe 500 800 500 200  # 向上滑动
adb shell dumpsys view | grep mTopRow  # 检查 mTopRow 是否变化
```

---

## 临时解决方案

### 方案 1：禁用自动滚动

```java
// 在 Termux 设置中添加选项
mEmulator.toggleAutoScrollDisabled();
```

### 方案 2：保留用户滚动位置

```java
// 修改 onScreenUpdated()
if (!skipScrolling && mTopRow != 0) {
    // 不要总是重置为 0
    // 只有当新内容超出当前视图时才滚动
    if (newContentRow > mTopRow + rows) {
        mTopRow = 0;
    }
    // 否则保持用户的位置
}
```

---

## 结论

**Rust 侧逻辑正确**，内容确实写入历史缓冲区。

问题可能在 **Java 侧的 UI 交互**：
1. 滚动条可能没有正确显示
2. 触摸事件可能没有正确更新 `mTopRow`
3. 自动滚动逻辑过于激进

**建议**：
1. 添加日志诊断具体问题
2. 检查 `computeVerticalScrollRange()` 返回值
3. 测试手动滑动是否更新 `mTopRow`
4. 考虑添加"禁用自动滚动"选项
