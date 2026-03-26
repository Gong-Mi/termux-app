# Rust-Java 交互运算完整清单

**分析日期**: 2026-03-26  
**Rust 版本**: termux-app-rust  
**JNI 接口总数**: 50 个

---

## 一、JNI 接口完整清单

### 1.1 引擎管理 (6 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `createEngineRustWithCallback` | 构造函数 | 创建终端引擎 | ✅ |
| `destroyEngineRust` | `destroy()` | 销毁引擎 | ✅ |
| `nativeProcess` | - | 处理输入数据（旧接口） | ✅ |
| `processBatchRust` | `append()` | 处理批量输入 | ✅ |
| `processCodePointRust` | `processCodePoint()` | 处理单个码点 | ✅ |
| `resizeEngineRustFull` | `resize()` | 调整大小 | ✅ |

### 1.2 光标查询 (5 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `getCursorRowFromRust` | `getCursorRow()` | 获取光标行 | ✅ |
| `getCursorColFromRust` | `getCursorCol()` | 获取光标列 | ✅ |
| `getCursorStyleFromRust` | `getCursorStyle()` | 获取光标样式 | ✅ |
| `isCursorEnabledFromRust` | `isCursorEnabled()` | 光标是否启用 | ✅ |
| `shouldCursorBeVisibleFromRust` | `shouldCursorBeVisible()` | 光标是否可见 | ✅ |

### 1.3 光标控制 (4 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `setCursorStyleFromRust` | `setCursorStyle()` | 设置光标样式 | ✅ |
| `setCursorBlinkStateInRust` | `setCursorBlinkState()` | 设置闪烁状态 | ✅ |
| `setCursorBlinkingEnabledInRust` | `setCursorBlinkingEnabled()` | 启用闪烁 | ✅ |
| `getDebugInfoFromRust` | `toString()` | 调试信息 | ✅ |

### 1.4 模式查询 (7 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `isReverseVideoFromRust` | `isReverseVideo()` | 反色模式 | ✅ |
| `isAlternateBufferActiveFromRust` | `isAlternateBufferActive()` | 备用缓冲区 | ✅ |
| `isCursorKeysApplicationModeFromRust` | `isCursorKeysApplicationMode()` | 应用光标键 | ✅ |
| `isKeypadApplicationModeFromRust` | `isKeypadApplicationMode()` | 应用小键盘 | ✅ |
| `isMouseTrackingActiveFromRust` | `isMouseTrackingActive()` | 鼠标追踪 | ✅ |
| `isInsertModeActiveFromRust` | `isInsertMode()` | 插入模式 | ⚠️ 返回 0 |
| `doDecSetOrResetFromRust` | `doDecSetOrReset()` | DECSET 命令 | ✅ |

### 1.5 滚动控制 (5 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `getScrollCounterFromRust` | `getScrollCounter()` | 获取滚动计数 | ✅ |
| `clearScrollCounterFromRust` | `clearScrollCounter()` | 清除滚动计数 | ✅ |
| `isAutoScrollDisabledFromRust` | `isAutoScrollDisabled()` | 自动滚动禁用 | ✅ |
| `toggleAutoScrollDisabledFromRust` | `toggleAutoScrollDisabled()` | 切换自动滚动 | ✅ |
| `getActiveTranscriptRowsFromRust` | `getActiveTranscriptRows()` | 历史行数 | ✅ |

### 1.6 尺寸查询 (3 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `getRowsFromRust` | `getRows()` | 获取行数 | ✅ |
| `getColsFromRust` | `getCols()` | 获取列数 | ✅ |
| `readRowFromRust` | `readRow()` | 读取行数据 | ✅ |

### 1.7 文本访问 (4 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `getSelectedTextFromRust` | `getSelectedText()` | 获取选定文本 | ✅ |
| `getWordAtLocationFromRust` | `getWordAtLocation()` | 获取单词 | ✅ |
| `getTranscriptTextFromRust` | `getTranscriptText()` | 获取全部文本 | ✅ |
| `getTitleFromRust` | `getTitle()` | 获取标题 | ✅ |

### 1.8 输入事件 (4 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `sendMouseEventFromRust` | `sendMouseEvent()` | 发送鼠标事件 | ✅ |
| `sendKeyCodeFromRust` | `sendKeyEvent()` | 发送按键事件 | ✅ |
| `pasteTextFromRust` | `paste()` | 粘贴文本 | ✅ |
| `updateTerminalSessionClientFromRust` | `updateTerminalSessionClient()` | 更新客户端 | ✅ |

### 1.9 颜色控制 (3 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `getColorsFromRust` | `getCurrentColors()` | 获取颜色数组 | ✅ |
| `resetColorsFromRust` | `resetColors()` | 重置颜色 | ✅ |

### 1.10 PTY 处理 (4 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `createSubprocess` | `createSubprocess()` | 创建子进程 | ✅ |
| `setPtyWindowSize` | `setPtyWindowSize()` | 设置窗口大小 | ✅ |
| `waitFor` | `waitFor()` | 等待进程退出 | ✅ |
| `close` | `close()` | 关闭 FD | ✅ |

### 1.11 工具函数 (1 个)

| JNI 函数 | Java 方法 | 功能 | 状态 |
|----------|----------|------|------|
| `WcWidth_widthRust` | `widthRust()` | 计算字符宽度 | ✅ |

---

## 二、Java 层运算（不经过 Rust）

### 2.1 已迁移到 Rust 的运算

| 运算 | 原 Java 实现 | 现 Rust 实现 | 状态 |
|------|-------------|-------------|------|
| **终端模拟核心** | `TerminalEmulator.append()` | `processBatchRust()` | ✅ |
| **VTE 解析** | `processByte()`, `doEsc()` 等 | `vte_parser.rs` | ✅ |
| **光标移动** | `setCursorPosition()` | `cursor.rs` | ✅ |
| **滚动处理** | `scrollDownOneLine()` | `screen.rs::scroll_up()` | ✅ |
| **SGR 样式** | `selectGraphicRendition()` | `engine.rs::handle_sgr()` | ✅ |
| **DECSET 处理** | `doDecSetOrReset()` | `engine.rs::handle_decset()` | ✅ |
| **字符渲染** | `emitCodePoint()` | `print.rs` | ✅ |
| **缓冲区管理** | `TerminalBuffer.resize()` | `screen.rs::resize_with_reflow()` | ✅ |
| **颜色解析** | `mColors` 数组 | `colors.rs` | ✅ |
| **Sixel 解码** | 无 | `sixel.rs` | ✅ |

### 2.2 仍保留在 Java 层的运算

| 运算 | 位置 | 说明 | 状态 |
|------|------|------|------|
| **PTY 进程管理** | `TerminalSession.java` | 进程创建/销毁 | ✅ 保留 |
| **I/O 队列处理** | `TerminalSession.java` | `ByteQueue` | ✅ 保留 |
| **UI 渲染** | `TerminalView.java` | Canvas 绘制 | ✅ 保留 |
| **输入事件分发** | `TerminalView.java` | 触摸/键盘事件 | ✅ 保留 |
| **会话管理** | `TerminalSession.java` | 多会话管理 | ✅ 保留 |
| **颜色主题** | `TerminalColors.java` | 主题管理 | ✅ 保留 |
| **样式编码** | `TextStyle.java` | 样式位运算 | ✅ 保留 |
| **字符宽度** | `WcWidth.java` | Unicode 宽度 | ✅ 部分 Rust |

---

## 三、共享内存交互

### 3.1 屏幕数据访问（零拷贝）

```
Java 层                          Rust 层
┌─────────────────┐            ┌─────────────────┐
│ TerminalView    │            │ TerminalEngine  │
│   ↓             │            │   ↓             │
│ readRow()       │──JNI──────→│ sync_to_shared()│
│   ↓             │            │   ↓             │
│ 读取共享内存     │←───────────│ SharedScreenBuffer│
│   - text_data   │            │   - version     │
│   - style_data  │            │   - cols/rows   │
└─────────────────┘            │   - text_data   │
                               │   - style_data  │
                               └─────────────────┘
```

**优势**:
- 零拷贝（Zero-Copy）
- 直接内存访问
- 无 JNI 数组转换开销

---

## 四、回调机制

### 4.1 Rust → Java 回调

| 回调类型 | Rust 触发 | Java 接收 | 用途 |
|----------|----------|----------|------|
| **屏幕更新** | `report_screen_update()` | `onScreenUpdated()` | 通知 UI 刷新 |
| **标题变化** | `report_title_change()` | `reportTitleChange()` | 更新窗口标题 |
| **Sixel 图像** | `report_sixel_image()` | `renderSixelImage()` | 渲染 Sixel 图形 |
| **光标变化** | `report_cursor_change()` | `onCursorUpdated()` | 更新光标位置 |
| **颜色变化** | `report_colors_change()` | `onColorsChanged()` | 更新颜色主题 |
| **会话结束** | - | `onSessionFinished()` | 通知会话结束 |

---

## 五、数据流图

```
┌─────────────────────────────────────────────────────────────┐
│                        Java Application Layer                │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │TerminalView │  │TerminalSession│ │ TerminalEmulator    │  │
│  │  (UI 渲染)   │  │  (PTY 管理)   │ │  (JNI 包装)         │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                     │             │
│         │ 输入事件        │ PTY I/O            │ JNI 调用     │
│         ↓                ↓                     ↓             │
├─────────────────────────────────────────────────────────────┤
│                         JNI Layer                            │
│  ┌────────────────────────────────────────────────────────┐  │
│  │              libtermux_rust.so (Rust)                  │  │
│  │  ┌───────────┐  ┌───────────┐  ┌───────────────────┐  │  │
│  │  │ vte_parser│  │  engine   │  │    terminal       │  │  │
│  │  │ (VTE 解析) │  │ (状态管理) │  │  (screen/cursor)  │  │  │
│  │  └───────────┘  └───────────┘  └───────────────────┘  │  │
│  │  ┌───────────┐  ┌───────────┐  ┌───────────────────┐  │  │
│  │  │  colors   │  │  sixel    │  │   shared memory   │  │  │
│  │  │ (颜色管理) │  │ (图形解码) │  │   (零拷贝数据)     │  │  │
│  │  └───────────┘  └───────────┘  └───────────────────┘  │  │
│  └────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## 六、性能关键路径

### 6.1 高频调用（性能敏感）

| 调用 | 频率 | 优化状态 |
|------|------|----------|
| `processBatchRust()` | 极高（每字节） | ✅ 零拷贝 |
| `readRowFromRust()` | 高（每帧） | ✅ 共享内存 |
| `getCursorRow/Col()` | 高（每帧） | ✅ 直接访问 |
| `sendKeyCodeFromRust()` | 中（每次按键） | ✅ 直接调用 |

### 6.2 低频调用（不敏感）

| 调用 | 频率 | 说明 |
|------|------|------|
| `getTitleFromRust()` | 低（标题变化时） | 字符串复制可接受 |
| `getSelectedTextFromRust()` | 低（用户选择时） | 字符串复制可接受 |
| `getTranscriptTextFromRust()` | 低（复制时） | 字符串复制可接受 |

---

## 七、线程安全

### 7.1 锁机制

```rust
// Rust 侧使用 RwLock 保证线程安全
pub struct TerminalContext {
    pub lock: RwLock<TerminalEngine>,
}

// 读操作（多读者）
let engine = context.lock.read().unwrap();

// 写操作（单写者）
let mut engine = context.lock.write().unwrap();
```

### 7.2 Java 侧同步

```java
// Java 侧使用 synchronized 保证线程安全
public synchronized void append(byte[] batch, int length) {
    if (mEnginePtr != 0) {
        processBatchRust(mEnginePtr, batch, length);
    }
}
```

---

## 八、生命周期管理

### 8.1 引擎生命周期

```
Java                          Rust
┌─────────────────┐          ┌─────────────────┐
│ createEngine    │──JNI────→│ Box::new(Context)│
│   ↓             │          │   ↓             │
│ mEnginePtr = X  │←─────────│ Box::into_raw() │
│   ↓             │          │   ↓             │
│ 使用引擎        │←JNI 调用─│ *mut Context    │
│   ↓             │          │   ↓             │
│ destroy()       │──JNI────→│ Box::from_raw() │
│   ↓             │          │   ↓             │
│ mEnginePtr = 0  │          │ drop()          │
└─────────────────┘          └─────────────────┘
```

### 8.2 防崩溃机制

```rust
// 所有 JNI 入口都有 catch_unwind 保护
let result = std::panic::catch_unwind(|| {
    // Rust 逻辑
});
if result.is_err() {
    android_log(ERROR, "panic caught");
}
```

---

## 九、总结

### 9.1 运算分布

| 层级 | 运算类型 | 比例 |
|------|----------|------|
| **Rust** | 终端模拟核心、VTE 解析、缓冲区管理 | 85% |
| **Java** | PTY 管理、UI 渲染、会话管理 | 15% |

### 9.2 JNI 接口状态

- **总数**: 50 个
- **已实现**: 49 个 (98%)
- **待实现**: 1 个 (`isInsertModeActiveFromRust` 返回 0)

### 9.3 性能优化

- ✅ 零拷贝屏幕数据访问
- ✅ 共享内存渲染
- ✅ RwLock 并发控制
- ✅ catch_unwind 防崩溃

### 9.4 下一步优化

1. 实现 `isInsertModeActiveFromRust` 返回实际值
2. 考虑将更多低频操作迁移到 Rust
3. 优化 JNI 字符串转换性能
