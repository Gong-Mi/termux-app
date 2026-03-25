# 滚动问题最终分析

## 关键发现

### Java 侧滚动逻辑 ✅ 正确

```java
// TerminalView.java - doScroll() 方法
void doScroll(MotionEvent event, int rowsDown) {
    boolean up = rowsDown < 0;
    int amount = Math.abs(rowsDown);
    
    for (int i = 0; i < amount; i++) {
        if (mEmulator.isMouseTrackingActive()) {
            // 发送鼠标滚轮事件
        } else if (mEmulator.isAlternateBufferActive()) {
            // 备用缓冲区（如 vim）
        } else {
            // ← 正常终端模式的滚动
            mTopRow = Math.min(0, Math.max(-(mEmulator.getActiveTranscriptRows()), 
                                           mTopRow + (up ? -1 : 1)));
            if (!awakenScrollBars()) invalidate();
        }
    }
}
```

**逻辑分析**：
```java
mTopRow = Math.min(0,                    // 最大值为 0（屏幕顶部）
                   Math.max(-(activeTranscriptRows),  // 最小值为 -历史行数
                            mTopRow + (up ? -1 : 1))); // 向上滑动 -1，向下滑动 +1
```

**示例**：
- `activeTranscriptRows = 77`（有 77 行历史）
- 当前 `mTopRow = 0`（在底部）
- 向上滑动：`mTopRow = max(-77, 0 + (-1)) = -1`
- 继续滑动到顶：`mTopRow = -77`
- 向下滑动：`mTopRow = min(0, -77 + 1) = -76`

**结论**：Java 侧滚动逻辑**正确**！

---

## 完整滚动流程

### 1. 用户向上滑动

```
用户手指上滑
    ↓
GestureDetector.onScroll()
    ↓
TerminalView.onScroll(distanceX, distanceY)
    ↓
deltaRows = distanceY / fontLineSpacing
    ↓
doScroll(event, deltaRows)
    ↓
mTopRow += -1 (向上)
    ↓
invalidate()  // 重新渲染
```

### 2. 渲染时使用 mTopRow

```java
// TerminalRenderer.render()
for (int row = topRow; row < endRow; row++) {
    // topRow = mTopRow (可能是负数)
    // 例如：mTopRow = -50, rows = 24
    // 渲染行：-50, -49, ..., -27
    mEmulator.readRow(row, ...);
}
```

### 3. Rust 侧的 internal_row 转换

```rust
// screen.rs
pub fn internal_row(&self, row: i32) -> usize {
    let t = self.buffer.len() as i64;
    (((self.first_row as i64 + row as i64) % t + t) % t) as usize
}

// 示例：first_row = 77, row = -50, buffer.len() = 1000
// internal_row = ((77 + (-50)) % 1000 + 1000) % 1000
//              = (27 % 1000 + 1000) % 1000
//              = 27
// 读取 buffer[27] 的内容
```

---

## 测试验证

### Rust 侧测试 ✅

```rust
// content_overflow_test.rs
After writing 100 lines to 24-row screen:
  active_transcript = 77  ✓
  Line 1 at row -77  ✓
  Line 100 at row 22  ✓
```

### Java 侧逻辑 ✅

```java
// doScroll()
mTopRow = Math.min(0, Math.max(-77, mTopRow + (up ? -1 : 1)));
// 正确限制在 [-77, 0] 范围内
```

### 渲染逻辑 ✅

```java
// render()
for (row = mTopRow; row < mTopRow + 24; row++) {
    readRow(row);  // 支持负数行号
}
```

---

## 为什么用户觉得"不能滚动"？

### 可能原因

1. **滚动条不显示**
   - Android 的滚动条可能需要特定条件才显示
   - 检查 `setVerticalScrollBarEnabled(true)` 是否生效

2. **自动滚动太激进**
   - `onScreenUpdated()` 总是重置 `mTopRow = 0`
   - 用户刚滚上去，新内容一来又回到底部

3. **触摸灵敏度问题**
   - `distanceY / fontLineSpacing` 可能太小
   - 需要滑动很大距离才能滚动一行

4. **鼠标滚轮 vs 触摸**
   - 鼠标滚轮：`doScroll(event, ±3)` 一次滚动 3 行
   - 触摸：`distanceY / fontLineSpacing` 可能小于 1

---

## 诊断方法

### 1. 添加日志

```java
// TerminalView.java
void doScroll(MotionEvent event, int rowsDown) {
    int oldTopRow = mTopRow;
    mTopRow = Math.min(0, Math.max(-(mEmulator.getActiveTranscriptRows()), 
                                   mTopRow + (up ? -1 : 1)));
    Log.d("Termux-Scroll", "doScroll: rowsDown=" + rowsDown + 
          ", oldTopRow=" + oldTopRow + ", newTopRow=" + mTopRow +
          ", activeTranscript=" + mEmulator.getActiveTranscriptRows());
    if (!awakenScrollBars()) invalidate();
}

// onScreenUpdated()
public void onScreenUpdated(boolean skipScrolling) {
    int rowsInHistory = mEmulator.getActiveTranscriptRows();
    Log.d("Termux-Scroll", "onScreenUpdated: rowsInHistory=" + rowsInHistory +
          ", mTopRow=" + mTopRow + ", skipScrolling=" + skipScrolling);
    // ...
}
```

### 2. 测试命令

```bash
# 1. 产生大量输出
seq 1 200

# 2. 尝试向上滑动
# 观察日志输出

# 3. 检查滚动条
adb shell dumpsys view | grep -A 5 "ScrollBar"

# 4. 检查 mTopRow
adb shell dumpsys view | grep mTopRow
```

---

## 解决方案

### 方案 1：禁用自动滚动（推荐）

```java
// 在 Termux 设置中添加选项
public void toggleAutoScrollDisabled() {
    if (mEnginePtr != 0) toggleAutoScrollDisabledFromRust(mEnginePtr);
}

// onScreenUpdated() 中
if (mEmulator.isAutoScrollDisabled() && mTopRow != 0) {
    // 用户手动滚动后，保持位置
    skipScrolling = true;
}
```

### 方案 2：优化触摸灵敏度

```java
// onScroll()
int deltaRows = (int) (distanceY / mRenderer.getFontLineSpacing());
// 改为：
float rowHeight = mRenderer.getFontLineSpacing();
int deltaRows = (int) (distanceY / rowHeight);
if (Math.abs(deltaRows) < 1 && Math.abs(distanceY) > rowHeight / 2) {
    deltaRows = (int) Math.signum(distanceY);  // 至少滚动 1 行
}
```

### 方案 3：显示滚动条

```java
// attachSession() 或初始化时
setVerticalScrollBarEnabled(true);
setHorizontalScrollBarEnabled(false);

// onScreenUpdated()
if (mTopRow < -3) {
    awakenScrollBars();  // 已经调用了
}
```

---

## 结论

**Rust 和 Java 侧的滚动逻辑都是正确的！**

问题可能是：
1. **用户体验问题** - 自动滚动太激进
2. **滚动条显示问题** - Android 系统可能隐藏滚动条
3. **触摸灵敏度** - 需要更大滑动距离

**建议**：
1. 添加日志确认 `mTopRow` 是否变化
2. 测试禁用自动滚动
3. 检查滚动条是否正确配置
