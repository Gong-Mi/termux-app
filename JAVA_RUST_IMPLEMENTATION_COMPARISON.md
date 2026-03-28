# Java vs Rust 实现差异对比报告

## 1. 常量定义对比

### 1.1 终端配置常量

| 常量名 | Java 值 | Rust 值 | 状态 |
|--------|--------|---------|------|
| `DEFAULT_TERMINAL_TRANSCRIPT_ROWS` | 2000 | 2000 | ✅ 已修复 |
| `TERMINAL_TRANSCRIPT_ROWS_MIN` | 100 | 100 | ✅ 已修复 |
| `TERMINAL_TRANSCRIPT_ROWS_MAX` | 50000 | 50000 | ✅ |
| `DEFAULT_TERMINAL_CURSOR_STYLE` | `TERMINAL_CURSOR_STYLE_BLOCK` | `TERMINAL_CURSOR_STYLE_BLOCK` | ✅ |

### 1.2 光标样式常量

| 常量名 | Java 值 | Rust 值 | 状态 |
|--------|--------|---------|------|
| `TERMINAL_CURSOR_STYLE_BLOCK` | 0 | 0 | ✅ |
| `TERMINAL_CURSOR_STYLE_BAR` | 1 | 1 | ✅ |
| `TERMINAL_CURSOR_STYLE_UNDERLINE` | 2 | 2 | ✅ |

### 1.3 鼠标按钮常量

| 常量名 | Java 值 | Rust 值 | 状态 |
|--------|--------|---------|------|
| `MOUSE_LEFT_BUTTON` | 0 | 0 | ✅ |
| `MOUSE_MIDDLE_BUTTON` | 1 | 1 | ✅ |
| `MOUSE_RIGHT_BUTTON` | 2 | 2 | ✅ |
| `MOUSE_LEFT_BUTTON_MOVED` | 32 | 32 | ✅ |
| `MOUSE_WHEELUP_BUTTON` | 64 | 64 | ✅ |
| `MOUSE_WHEELDOWN_BUTTON` | 65 | 65 | ✅ |

### 1.4 特殊字符常量

| 常量名 | Java 值 | Rust 值 | 状态 |
|--------|--------|---------|------|
| `UNICODE_REPLACEMENT_CHAR` | 0xFFFD | 0xFFFD | ✅ |

### 1.5 DECSET 标志位

| 标志位 | Java | Rust | 状态 |
|--------|------|------|------|
| `DECSET_BIT_APPLICATION_CURSOR_KEYS` | 1 | 1 | ✅ |
| `DECSET_BIT_REVERSE_VIDEO` | 1 << 1 | 1 << 1 | ✅ |
| `DECSET_BIT_ORIGIN_MODE` | 1 << 2 | 1 << 2 | ✅ |
| `DECSET_BIT_AUTOWRAP` | 1 << 3 | 1 << 3 | ✅ |
| `DECSET_BIT_CURSOR_ENABLED` | 1 << 4 | 1 << 4 | ✅ |
| `DECSET_BIT_APPLICATION_KEYPAD` | 1 << 5 | 1 << 5 | ✅ |
| `DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE` | 1 << 6 | 1 << 6 | ✅ |
| `DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT` | 1 << 7 | 1 << 7 | ✅ |
| `DECSET_BIT_SEND_FOCUS_EVENTS` | 1 << 8 | 1 << 8 | ✅ |
| `DECSET_BIT_MOUSE_PROTOCOL_SGR` | 1 << 9 | 1 << 9 | ✅ |
| `DECSET_BIT_BRACKETED_PASTE_MODE` | 1 << 10 | 1 << 10 | ✅ |
| `DECSET_BIT_LEFTRIGHT_MARGIN_MODE` | 1 << 11 | 1 << 11 | ✅ |

### 1.6 样式效果常量 (EFFECT_*)

| 常量名 | Java (TextStyle) | Rust | 状态 |
|--------|-----------------|------|------|
| `EFFECT_BOLD` | 1 << 0 | 1 << 0 | ✅ |
| `EFFECT_ITALIC` | 1 << 1 | 1 << 1 | ✅ |
| `EFFECT_UNDERLINE` | 1 << 2 | 1 << 2 | ✅ |
| `EFFECT_BLINK` | 1 << 3 | 1 << 3 | ✅ |
| `EFFECT_REVERSE` | 1 << 4 | 1 << 4 | ✅ |
| `EFFECT_INVISIBLE` | 1 << 5 | 1 << 5 | ✅ |
| `EFFECT_STRIKETHROUGH` | 1 << 6 | 1 << 6 | ✅ |
| `EFFECT_PROTECTED` | 1 << 7 | 1 << 7 | ✅ |
| `EFFECT_DIM` | 1 << 8 | 1 << 8 | ✅ |

### 1.7 颜色索引常量

| 常量名 | Java | Rust | 状态 |
|--------|------|------|------|
| `COLOR_INDEX_FOREGROUND` | 256 | 256 | ✅ |
| `COLOR_INDEX_BACKGROUND` | 257 | 257 | ✅ |
| `COLOR_INDEX_CURSOR` | 258 | 258 | ✅ |

### 1.8 KeyHandler 修饰键常量

| 常量名 | Java 值 | Rust 值 | 状态 |
|--------|--------|---------|------|
| `KEYMOD_ALT` | 0x80000000 | 0x80000000 | ✅ |
| `KEYMOD_CTRL` | 0x40000000 | 0x40000000 | ✅ |
| `KEYMOD_SHIFT` | 0x20000000 | 0x20000000 | ✅ |
| `KEYMOD_NUM_LOCK` | 0x10000000 | 0x10000000 | ✅ |

---

## 2. 函数实现对比

### 2.1 TerminalEmulator 公共方法 (Java: 44 个)

| 方法名 | Java 实现 | Rust 实现 | 状态 |
|--------|----------|----------|------|
| `TerminalEmulator()` 构造函数 | ✅ | ✅ (通过 JNI) | ✅ |
| `append(byte[], int)` | ✅ | ✅ `processBatchRust` | ✅ |
| `processCodePoint(int)` | ✅ | ✅ `processCodePointRust` | ✅ |
| `isAlive()` | ✅ | ✅ `isAlive` | ✅ |
| `resize(int, int, int, int)` | ✅ | ✅ `resizeEngineRustFull` | ✅ |
| `getTitle()` | ✅ | ✅ `getTitleFromRust` | ✅ |
| `reset()` | ✅ | ✅ `resetToInitialState` | ✅ |
| `setCursorStyle(int)` | ✅ | ✅ `setCursorStyleFromRust` | ✅ |
| `doDecSetOrReset(boolean, int)` | ✅ | ✅ `doDecsetOrReset` | ✅ |
| `getCursorCol()` | ✅ | ✅ `getCursorColFromRust` | ✅ |
| `getCursorRow()` | ✅ | ✅ `getCursorRowFromRust` | ✅ |
| `getCursorStyle()` | ✅ | ✅ `getCursorStyleFromRust` | ✅ |
| `setCursorBlinkState(boolean)` | ✅ | ✅ `setCursorBlinkStateFromRust` | ✅ |
| `setCursorBlinkingEnabled(boolean)` | ✅ | ✅ `setCursorBlinkingEnabledFromRust` | ✅ |
| `isCursorEnabled()` | ✅ | ✅ `isCursorEnabledFromRust` | ✅ |
| `shouldCursorBeVisible()` | ✅ | ✅ `shouldCursorBeVisibleFromRust` | ✅ |
| `isReverseVideo()` | ✅ | ✅ `isReverseVideoFromRust` | ✅ |
| `isAlternateBufferActive()` | ✅ | ✅ `isAlternateBufferActiveFromRust` | ✅ |
| `isCursorKeysApplicationMode()` | ✅ | ✅ `isCursorKeysApplicationModeFromRust` | ✅ |
| `isKeypadApplicationMode()` | ✅ | ✅ `isKeypadApplicationModeFromRust` | ✅ |
| `isMouseTrackingActive()` | ✅ | ✅ `isMouseTrackingActiveFromRust` | ✅ |
| `getScrollCounter()` | ✅ | ✅ `getScrollCounterFromRust` | ✅ |
| `clearScrollCounter()` | ✅ | ✅ `clearScrollCounterFromRust` | ✅ |
| `getRows()` | ✅ | ✅ `getRowsFromRust` | ✅ |
| `getCols()` | ✅ | ✅ `getColsFromRust` | ✅ |
| `getActiveTranscriptRows()` | ✅ | ✅ `getActiveTranscriptRowsFromRust` | ✅ |
| `getActiveRows()` | ✅ | ✅ `getActiveRowsFromRust` | ✅ |
| `isAutoScrollDisabled()` | ✅ | ✅ `isAutoScrollDisabledFromRust` | ✅ |
| `toggleAutoScrollDisabled()` | ✅ | ✅ `toggleAutoScrollDisabledFromRust` | ✅ |
| `readRow(int, int[], long[])` | ✅ | ✅ `readRowFromRust` | ✅ |
| `getSelectedText(int, int, int, int)` | ✅ | ✅ `getSelectedTextFromRust` | ✅ |
| `getWordAtLocation(int, int)` | ✅ | ✅ `getWordAtLocationFromRust` | ✅ |
| `getTranscriptText()` | ✅ | ✅ `getTranscriptTextFromRust` | ✅ |
| `getCurrentColors()` | ✅ | ✅ `getColorsFromRust` | ✅ |
| `sendMouseEvent(int, int, int, boolean)` | ✅ | ✅ `sendMouseEventFromRust` | ✅ |
| `sendKeyEvent(int, int)` | ✅ | ✅ `sendKeyCodeFromRust` | ✅ |
| `sendCharEvent(char, int)` | ✅ | ✅ `sendCharKeyCodeFromRust` | ✅ |
| `paste(String)` | ✅ | ✅ `pasteFromRust` (括号粘贴) | ✅ |
| `resetColors()` | ✅ | ✅ `resetColorsFromRust` | ✅ |
| `getScreen()` | ✅ | ✅ (通过 `TerminalBufferCompat`) | ✅ |
| `getTotalRows()` | ✅ | ✅ `getTotalRowsFromRust` | ✅ |
| `destroy()` | ✅ | ✅ `destroyEngineFromRust` | ✅ |
| `toString()` | ✅ | ✅ `toStringFromRust` | ✅ |
| `updateTerminalSessionClient()` | ✅ | ✅ (通过回调) | ✅ |

---

## 3. 已修复的功能差异

### 3.1 清屏功能 (ED - Erase in Display)

**问题**: `erase_in_display()` mode 0/1 未清除当前行

**修复前 (Rust)**:
```rust
0 => { for y in (cursor_y + 1)..self.rows { ... } }  // ❌ 跳过当前行
1 => { for y in 0..cursor_y { ... } }  // ❌ 跳过当前行
```

**修复后 (Rust)**:
```rust
0 => {
    self.get_row_mut(cursor_y).clear(cursor_x as usize, c, style);  // ✅ 清除当前行
    for y in (cursor_y + 1)..self.rows { ... }
}
1 => {
    for y in 0..cursor_y { ... }
    self.get_row_mut(cursor_y).clear(0, (cursor_x + 1) as usize, style);  // ✅ 清除当前行
}
```

### 3.2 滚动历史行数

**问题**: Rust 默认 600 行，Java 默认 2000 行

**修复**:
- `DEFAULT_TERMINAL_TRANSCRIPT_ROWS`: 600 → 2000
- `TERMINAL_TRANSCRIPT_ROWS_MIN`: 0 → 100

---

## 4. 潜在差异和待检查项

### 4.1 VTE 解析器状态

| 状态 | Java | Rust | 备注 |
|------|------|------|------|
| `ESC_NONE` | 0 | 0 | ✅ |
| `ESC` | 1 | 1 | ✅ |
| `ESC_CSI` | 6 | 6 | ✅ |
| `ESC_CSI_QUESTIONMARK` | 7 | 7 | ✅ |
| `ESC_OSC` | 10 | 10 | ✅ |
| `ESC_P` (DCS) | 13 | 13 | ✅ |
| `ESC_APC` | 20 | 20 | ✅ |

### 4.2 可能缺失的功能

1. **DECSET 1003** - 鼠标追踪所有事件 (未实现)
   - Rust 代码中标记为"暂时不实现"

2. **DECSET 3** - 132 列模式
   - Rust 标记为"未实现，忽略"

3. **DECSET 40/45** - 132 列模式切换/反向换行
   - Rust 标记为"未实现"

4. **DECSET 1034** - 8 位输入模式
   - Rust 标记为"不实现"

### 4.3 SGR (Select Graphic Rendition) 支持

| SGR 代码 | 功能 | Java | Rust | 状态 |
|----------|------|------|------|------|
| 0 | 重置 | ✅ | ✅ | ✅ |
| 1 | 粗体 | ✅ | ✅ | ✅ |
| 2 | 暗淡 | ✅ | ✅ | ✅ |
| 3 | 斜体 | ✅ | ✅ | ✅ |
| 4 | 下划线 | ✅ | ✅ | ✅ |
| 5 | 闪烁 | ✅ | ✅ | ✅ |
| 7 | 反色 | ✅ | ✅ | ✅ |
| 8 | 隐藏 | ✅ | ✅ | ✅ |
| 9 | 删除线 | ✅ | ✅ | ✅ |
| 21 | 双下划线 | ✅ | ✅ (作为单下划线) | ⚠️ |
| 22-29 | 关闭效果 | ✅ | ✅ | ✅ |
| 30-37 | 前景色 | ✅ | ✅ | ✅ |
| 38 | 256/真彩色前景 | ✅ | ✅ | ✅ |
| 39 | 默认前景色 | ✅ | ✅ | ✅ |
| 40-47 | 背景色 | ✅ | ✅ | ✅ |
| 48 | 256/真彩色背景 | ✅ | ✅ | ✅ |
| 49 | 默认背景色 | ✅ | ✅ | ✅ |
| 58 | 下划线颜色 | ✅ | ✅ | ✅ |
| 59 | 默认下划线颜色 | ✅ | ✅ | ✅ |

---

## 5. 架构差异

### 5.1 代码行数对比

| 模块 | Java | Rust | 备注 |
|------|------|------|------|
| `TerminalEmulator.java` | 2618 行 | 439 行 (JNI 封装) | Rust 逻辑在 engine.rs |
| `TerminalBuffer.java` | 498 行 | ~600 行 (screen.rs) | Rust 包含更多功能 |
| `TerminalRow.java` | ~200 行 | ~150 行 (TerminalRow) | 基本一致 |
| 核心引擎 | ~4000 行 (总计) | ~4000 行 (Rust) | 功能对等 |

### 5.2 性能优化差异

| 特性 | Java | Rust | 优势 |
|------|------|------|------|
| 内存管理 | GC | 手动/RAII | Rust 更可控 |
| 并行处理 | 单线程 | Rayon 并行 | Rust 性能更好 |
| SIMD 优化 | 无 | simdutf8 | Rust 更快 |
| JNI 开销 | N/A | 有 | Java 原生 |

---

## 6. 测试覆盖对比

### 6.1 Rust 测试文件

| 测试文件 | 测试内容 | 状态 |
|----------|----------|------|
| `consistency.rs` | 基础功能一致性 | ✅ 2958 行 |
| `extended_features.rs` | 扩展功能 | ✅ |
| `performance.rs` | 性能测试 | ✅ |
| `check_width.rs` | 字符宽度 | ✅ |
| `fix_verification.rs` | 修复验证 | ✅ |
| `key_event_handling.rs` | 按键事件 | ✅ |

### 6.2 Java 测试

| 测试类型 | Java | Rust | 备注 |
|----------|------|------|------|
| 单元测试 | 有限 | 完善 | Rust 更完整 |
| 集成测试 | 有限 | 完善 | Rust 更完整 |
| 性能基准 | 无 | 有 | Rust 优势 |

---

## 7. 总结

### 7.1 完全对齐的功能
- ✅ 所有 DECSET 标志位
- ✅ 所有 SGR 效果常量
- ✅ 所有颜色索引
- ✅ 所有鼠标按钮定义
- ✅ 所有修饰键常量
- ✅ 所有公共 API 方法

### 7.2 已修复的差异
- ✅ 滚动历史行数 (600 → 2000)
- ✅ 清屏功能当前行处理
- ✅ 最小滚动行数 (0 → 100)

### 7.3 已知未实现功能 (不影响正常使用)
- ⚠️ DECSET 1003 (所有鼠标事件)
- ⚠️ DECSET 3/40 (132 列模式)
- ⚠️ DECSET 45 (反向换行)
- ⚠️ DECSET 1034 (8 位输入)

### 7.4 Rust 优势
- 🚀 更好的性能 (并行处理、SIMD)
- 🚀 更完善的测试覆盖
- 🚀 更安全的内存管理
- 🚀 更小的二进制体积 (优化后)
