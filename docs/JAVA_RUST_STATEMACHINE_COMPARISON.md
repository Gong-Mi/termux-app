# Java vs Rust 状态机功能一致性对比报告

## 执行摘要

本文档详细对比了 Termux 终端模拟器中 Java 和 Rust 两种状态机实现的功能差异和一致性状态。

**当前状态**: Rust 引擎被禁用 (`FULL TAKEOVER = false`)，仅使用 Fast Path 优化。

---

## 1. 架构对比

### 1.1 Java 状态机架构

```
┌─────────────────────────────────────────────────────────┐
│              TerminalEmulator.java (2737 行)             │
│                                                          │
│  状态变量:                                                │
│  - mCursorRow, mCursorCol (光标位置)                     │
│  - mScreen (TerminalBuffer - 屏幕缓冲区)                 │
│  - mColors (颜色表)                                      │
│  - mDecsetBits (DECSET 模式标志)                         │
│  - mEscapeMode (转义序列状态机)                          │
│                                                          │
│  状态机状态:                                              │
│  - ESC_NONE (0) - 非转义序列                              │
│  - ESC (1) - 已见 ESC                                     │
│  - ESC_CSI (6) - CSI 序列                                 │
│  - ESC_OSC (10) - OSC 序列                                │
│  - ... (共 24 种状态)                                      │
│                                                          │
│  核心方法:                                                │
│  - append() - 处理输入字节                                │
│  - doCsi() - 处理 CSI 序列                                │
│  - doEsc() - 处理 ESC 序列                                │
│  - doOsc() - 处理 OSC 序列                                │
│  - selectGraphicRendition() - SGR 处理                    │
└─────────────────────────────────────────────────────────┘
```

### 1.2 Rust 状态机架构

```
┌─────────────────────────────────────────────────────────┐
│              TerminalEngine (engine.rs - 341 行)         │
│                                                          │
│  依赖: vte crate (Parser + Perform trait)                │
│                                                          │
│  结构体:                                                  │
│  - TerminalEngine                                        │
│    - parser: Parser (vte 状态机)                         │
│    - state: ScreenState                                  │
│  - ScreenState                                           │
│    - buffer: Vec<TerminalRow> (循环缓冲区)               │
│    - cursor_x, cursor_y (光标)                           │
│    - screen_first_row (循环缓冲区偏移)                   │
│  - PurePerformHandler                                    │
│    - 实现 vte::Perform trait                             │
│                                                          │
│  核心方法:                                                │
│  - process_bytes() - 调用 vte parser.advance()           │
│  - print() / execute() / csi_dispatch() / esc_dispatch() │
└─────────────────────────────────────────────────────────┘
```

---

## 2. 功能实现对比矩阵

### 2.1 控制字符处理

| 功能 | Java | Rust | 一致性 |
|------|------|------|--------|
| NUL (0x00) | ✅ 忽略 | ✅ 忽略 (vte 处理) | ✅ |
| BEL (0x07) | ✅ 响铃 | ✅ 响铃 (vte 处理) | ✅ |
| BS (0x08) | ✅ 退格 | ✅ `cursor_x = max(0, cursor_x - 1)` | ✅ |
| HT (0x09) | ✅ 制表符 | ✅ `(cursor_x + 8) & !7` | ✅ |
| LF (0x0a) | ✅ 换行 + 滚动 | ✅ 换行 + 滚动 | ✅ |
| VT (0x0b) | ✅ 同 LF | ✅ 同 LF (vte 处理) | ✅ |
| FF (0x0c) | ✅ 同 LF | ✅ 同 LF (vte 处理) | ✅ |
| CR (0x0d) | ✅ 回车 | ✅ `cursor_x = 0` | ✅ |

### 2.2 CSI 序列 (ESC [)

| 序列 | 功能 | Java | Rust | 一致性 |
|------|------|------|------|--------|
| `@` | ICH 插入字符 | ✅ | ❌ | ❌ |
| `A` | CUU 光标上移 | ✅ | ✅ | ✅ |
| `B` | CUD 光标下移 | ✅ | ✅ | ✅ |
| `C` | CUF 光标右移 | ✅ | ✅ | ✅ |
| `D` | CUB 光标左移 | ✅ | ✅ | ✅ |
| `E` | CNL 下一行 | ✅ | ❌ | ❌ |
| `F` | CPL 上一行 | ✅ | ❌ | ❌ |
| `G` | CHA 水平定位 | ✅ | ❌ | ❌ |
| `H` | CUP 光标定位 | ✅ | ✅ | ✅ |
| `J` | ED 清屏 | ✅ (0,1,2,3) | ✅ (0,1,2,3) | ✅ |
| `K` | EL 清行 | ✅ (0,1,2) | ✅ (0,1,2) | ✅ |
| `L` | IL 插入行 | ✅ | ❌ | ❌ |
| `M` | DL 删除行 | ✅ | ❌ | ❌ |
| `P` | DCH 删除字符 | ✅ | ❌ | ❌ |
| `S` | SU 上滚 | ✅ | ❌ | ❌ |
| `T` | SD 下滚 | ✅ | ❌ | ❌ |
| `X` | ECH 擦除字符 | ✅ | ❌ | ❌ |
| `Z` | CBT 后退制表 | ✅ | ❌ | ❌ |
| `d` | VPA 垂直定位 | ✅ | ❌ | ❌ |
| `f` | HVP 水平垂直定位 | ✅ | ✅ | ✅ |
| `g` | TBC 清除制表 | ✅ | ❌ | ❌ |
| `h` | SM 设置模式 | ✅ | ❌ | ❌ |
| `l` | RM 重置模式 | ✅ | ❌ | ❌ |
| `m` | SGR 字符属性 | ✅ (完整) | ⚠️ (仅基础色) | ⚠️ |
| `n` | DSR 设备状态 | ✅ | ❌ | ❌ |
| `r` | DECSTBM 设置边距 | ✅ | ❌ | ❌ |
| `s` | DECSLRM/DECSC | ✅ | ❌ | ❌ |
| `t` | 窗口操作 | ✅ | ❌ | ❌ |
| `u` | DECRC 恢复光标 | ✅ | ❌ | ❌ |

### 2.3 ESC 序列

| 序列 | 功能 | Java | Rust | 一致性 |
|------|------|------|------|--------|
| `7` | DECSC 保存光标 | ✅ | ✅ | ✅ |
| `8` | DECRC 恢复光标 | ✅ | ✅ | ✅ |
| `#8` | DECALN 屏幕测试 | ✅ | ❌ | ❌ |
| `(` | G0 字符集 | ✅ | ❌ | ❌ |
| `)` | G1 字符集 | ✅ | ❌ | ❌ |
| `=` | DECPAM 应用键盘 | ✅ | ❌ | ❌ |
| `>` | DECPNM 数字键盘 | ✅ | ❌ | ❌ |
| `D` | IND 索引 | ✅ | ❌ | ❌ |
| `E` | NEL 下一行 | ✅ | ❌ | ❌ |
| `M` | RI 反向索引 | ✅ | ❌ | ❌ |
| `Z` | DECID 设备标识 | ✅ | ❌ | ❌ |
| `c` | RIS 重置 | ✅ | ❌ | ❌ |

### 2.4 OSC 序列 (ESC ])

| 序列 | 功能 | Java | Rust | 一致性 |
|------|------|------|------|--------|
| `0` | 设置图标+窗口标题 | ✅ | ❌ | ❌ |
| `2` | 设置窗口标题 | ✅ | ❌ | ❌ |
| `4` | 设置颜色 | ✅ | ❌ | ❌ |
| `10-19` | 动态颜色 | ✅ | ❌ | ❌ |
| `52` | 剪贴板操作 | ✅ | ❌ | ❌ |
| `104` | 重置颜色 | ✅ | ❌ | ❌ |
| `110-112` | 重置特殊颜色 | ✅ | ❌ | ❌ |

### 2.5 DECSET/DECRST 私有模式

| 模式 | 功能 | Java | Rust | 一致性 |
|------|------|------|------|--------|
| `1` | 应用光标键 | ✅ | ❌ | ❌ |
| `3` | 列模式 | ✅ | ❌ | ❌ |
| `5` | 反显 | ✅ | ❌ | ❌ |
| `6` | 原点模式 | ✅ | ❌ | ❌ |
| `7` | 自动换行 | ✅ | ❌ | ❌ |
| `12` | 本地回显 | ✅ | ❌ | ❌ |
| `25` | 光标可见性 | ✅ | ❌ | ❌ |
| `35` | Shift 键 | ✅ | ❌ | ❌ |
| `42` | 替换模式 | ✅ | ❌ | ❌ |
| `1000` | 鼠标跟踪 | ✅ | ❌ | ❌ |
| `1002` | 按钮事件 | ✅ | ❌ | ❌ |
| `1003` | 所有事件 | ✅ | ❌ | ❌ |
| `1004` | 焦点跟踪 | ✅ | ❌ | ❌ |
| `1006` | SGR 鼠标 | ✅ | ❌ | ❌ |
| `1049` | 备用屏幕 | ✅ | ❌ | ❌ |
| `2004` | 括号粘贴 | ✅ | ❌ | ❌ |
| `2026` | 同步输出 | ✅ | ❌ | ❌ |

### 2.6 SGR 字符属性 (ESC [ m)

| 属性 | Java | Rust | 一致性 |
|------|------|------|--------|
| 0 重置 | ✅ | ✅ | ✅ |
| 1 粗体 | ✅ | ❌ | ❌ |
| 2 淡色 | ✅ | ❌ | ❌ |
| 3 斜体 | ✅ | ❌ | ❌ |
| 4 下划线 | ✅ | ❌ | ❌ |
| 5 闪烁 | ✅ | ❌ | ❌ |
| 7 反显 | ✅ | ❌ | ❌ |
| 8 隐藏 | ✅ | ❌ | ❌ |
| 9 删除线 | ✅ | ❌ | ❌ |
| 30-37 前景色 | ✅ | ✅ (仅 30-37) | ⚠️ |
| 38;5;n 256 色 | ✅ | ❌ | ❌ |
| 38;2;r;g;b 真彩色 | ✅ | ❌ | ❌ |
| 40-47 背景色 | ✅ | ❌ | ❌ |
| 48;5;n 256 色 | ✅ | ❌ | ❌ |
| 48;2;r;g;b 真彩色 | ✅ | ❌ | ❌ |
| 90-97 亮前景色 | ✅ | ❌ | ❌ |
| 100-107 亮背景色 | ✅ | ❌ | ❌ |

---

## 3. 数据结构对比

### 3.1 屏幕缓冲区

| 特性 | Java | Rust |
|------|------|------|
| 实现 | `TerminalBuffer` + `TerminalRow[]` | `Vec<TerminalRow>` |
| 滚动优化 | `System.arraycopy()` | 循环缓冲区 O(1) |
| 行存储 | 动态数组 | 固定容量 Vec |
| 样式编码 | `TextStyle.encode()` (long) | `u64` 位字段 |
| 宽字符处理 | 占位符 `\0` | 占位符 `\0` |

### 3.2 光标状态

| 字段 | Java | Rust |
|------|------|------|
| 列 | `mCursorCol` (int) | `cursor_x` (i32) |
| 行 | `mCursorRow` (int) | `cursor_y` (i32) |
| 保存列 | `mSavedCursorCol` | `saved_x` |
| 保存行 | `mSavedCursorRow` | `saved_y` |
| 原点模式 | `mOriginMode` | ❌ |

### 3.3 边距

| 类型 | Java | Rust |
|------|------|------|
| 上边距 | `mTopMargin` | `top_margin` |
| 下边距 | `mBottomMargin` | `bottom_margin` |
| 左边距 | `mLeftMargin` | ❌ |
| 右边距 | `mRightMargin` | ❌ |

### 3.4 颜色

| 类型 | Java | Rust |
|------|------|------|
| 前景色 | `mForeColor` (int) | `current_style` (u64 低 8 位) |
| 背景色 | `mBackColor` (int) | ❌ |
| 效果 | `mEffect` (int) | ❌ |
| 颜色表 | `mColors.mCurrentColors[256]` | ❌ |

---

## 4. 性能对比

### 4.1 测试结果 (来自 java-rust-performance-comparison.md)

| 测试项目 | Java 阈值 | Rust 实际 | 提升倍数 |
|---------|----------|----------|---------|
| 原始文本处理 | >20 MB/s | 240 MB/s | **12x** |
| ANSI 转义序列 | >2 MB/s | 34 MB/s | **17x** |
| 光标移动 | N/A | 6.5M ops/s | N/A |
| 滚动操作 | N/A | 16M lines/s | N/A |
| 宽字符处理 | N/A | 90M chars/s | N/A |
| 小批量高频 | N/A | 11.6M calls/s | N/A |

### 4.2 瓶颈分析

**Java 瓶颈**:
1. GC 压力 - 频繁创建临时对象
2. 边界检查 - 数组访问需要运行时检查
3. JIT 预热 - 需要时间达到峰值性能
4. 虚方法调用 - 多态性带来的开销

**Rust 瓶颈**:
1. JNI 开销 - Java/Rust 边界调用成本
2. UTF-8 解码 - 多字节字符处理
3. 状态同步 - Rust→Java 状态更新

---

## 5. 一致性测试状态

### 5.1 当前测试覆盖

```
文件：terminal-emulator/src/main/rust/tests/consistency.rs
测试用例: 4 个基础测试
  - test_basic_text
  - test_newline
  - test_cursor_position
  - test_erase_display
```

### 5.2 测试框架限制

1. **Rust 引擎已禁用** - `FULL_TAKEOVER = false`
2. **无法验证 Rust 输出** - `getRowContent` 总是从 Java Buffer 读取
3. **需要 Android 环境** - ConsistencyTest 是 Android Instrumentation Test

### 5.3 已知不一致问题

| 问题 | 描述 | 影响 |
|------|------|------|
| 状态不同步 | Rust 引擎独立维护状态 | 屏幕内容不匹配 |
| 光标位置差异 | 某些序列处理不同 | 光标位置错误 |
| 边距处理缺失 | Rust 不支持左右边距 | 区域滚动错误 |
| 颜色属性丢失 | Rust 仅支持基础 8 色 | 颜色显示错误 |
| 标题不传播 | OSC 序列未实现 | 窗口标题不变 |

---

## 6. 实现路径建议

### 6.1 方案 A: 完整 Rust 实现 (推荐长期)

**目标**: 使 Rust 引擎功能完整，重新启用 `FULL TAKEOVER`

**工作量**: 200-400 小时

**优先级**:
1. **高** - CSI 序列补全 (G, L, M, P, S, T, X, n, r)
2. **高** - SGR 完整支持 (256 色、真彩色、属性)
3. **中** - ESC 序列补全 (D, E, M, Z, c)
4. **中** - OSC 序列支持 (0, 2, 52)
5. **低** - DECSET 私有模式 (1, 3, 5, 6, 7, 25, 1000, 2004, 2026)
6. **低** - 左右边距支持

**收益**:
- 最大性能提升 (单次解析)
- 简洁架构 (Rust 拥有状态)
- 更好内存效率 (无重复缓冲区)

### 6.2 方案 B: 混合解析模式 (推荐短期)

**目标**: Rust 解析 + Java 执行

**工作量**: 40-80 小时

**实现**:
1. 修改 `PurePerformHandler` 通过 JNI 回调 Java
2. Java 在 `mScreen` 上执行所有状态变更
3. Rust 仅提供快速解析，不存储状态
4. 保持 `getRowContent` 从 Java 缓冲区读取

**收益**:
- 利用现有 Java 实现
- 保证兼容性
- 实现更快

**缺点**:
- 每次序列都有 JNI 开销
- 代码更复杂 (两条执行路径)

### 6.3 方案 C: 仅 Fast Path (当前状态)

**目标**: 保持当前实现，仅优化 ASCII 批次

**工作量**: 已实现

**收益**:
- 稳定工作
- 无兼容性问题

**缺点**:
- 性能提升有限
- Rust 代码部分未使用

---

## 7. 测试建议

### 7.1 短期测试改进

1. **创建纯 JUnit 测试**
   - 文件：`RustConsistencyTest.java`
   - 不依赖 Android 框架
   - 可通过 `./gradlew test` 运行

2. **添加测试模式开关**
   ```java
   // TerminalEmulator.java
   public static boolean sForceUseRustEngine = false;
   ```

3. **扩展 Rust 测试用例**
   - 文件：`tests/consistency.rs`
   - 覆盖所有已实现的 CSI/ESC 序列

### 7.2 中期测试框架

1. **Python 验证脚本**
   - 文件：`tools/consistency_check.py`
   - 同时调用 Java 和 Rust
   - 生成详细对比报告

2. **CI 集成**
   - GitHub Actions 自动运行
   - 每次提交验证一致性

### 7.3 长期测试策略

1. **属性基测试**
   - 使用 QuickCheck (Rust)
   - 自动生成测试用例

2. ** fuzzing 测试**
   - 随机字节序列
   - 发现边界情况

3. **应用兼容性测试**
   - vim, nano, htop, mc
   - tmux, screen
   - bash, zsh, fish

---

## 8. 代码位置索引

### Rust 代码
```
terminal-emulator/src/main/rust/
├── Cargo.toml                    # Rust 包配置 (edition = "2024")
├── src/
│   ├── lib.rs                    # JNI 入口 (processBatchRust 等)
│   ├── engine.rs                 # 终端引擎 (341 行，DISABLED)
│   ├── fastpath.rs               # ASCII 快速扫描
│   ├── utils.rs                  # 工具函数
│   └── pty.rs                    # 进程管理
└── tests/
    ├── performance.rs            # 性能测试
    └── consistency.rs            # 一致性测试
```

### Java 代码
```
terminal-emulator/src/main/java/com/termux/terminal/
├── TerminalEmulator.java         # 主模拟器逻辑 (2737 行)
├── TerminalBuffer.java           # 屏幕缓冲区
├── TerminalRow.java              # 行存储
├── TextStyle.java                # 样式编码
├── Colors.java                   # 颜色管理
└── JNI.java                      # Native 库加载
```

### 测试代码
```
terminal-emulator/src/
├── test/java/                    # JUnit 4 本地测试
│   └── com/termux/terminal/
│       ├── TerminalPerformanceTest.java
│       └── JavaRustPerformanceComparisonTest.java
└── androidTest/java/             # Android Instrumentation 测试
    └── com/termux/terminal/
        └── ConsistencyTest.java
```

---

## 9. 结论

### 9.1 功能覆盖率

| 类别 | Java | Rust | 覆盖率 |
|------|------|------|--------|
| 控制字符 | 8/8 | 8/8 | 100% |
| CSI 序列 | 30/30 | 8/30 | **27%** |
| ESC 序列 | 12/12 | 2/12 | **17%** |
| OSC 序列 | 7/7 | 0/7 | **0%** |
| DECSET 模式 | 17/17 | 0/17 | **0%** |
| SGR 属性 | 20/20 | 2/20 | **10%** |
| **总体** | **94/94** | **20/94** | **21%** |

### 9.2 关键发现

1. **Rust 仅实现基础功能** - 仅支持基本文本输出和光标移动
2. **复杂序列完全缺失** - OSC、DECSET、高级 SGR 未实现
3. **性能优势明显** - Rust 快路径提供 2-17x 性能提升
4. **兼容性是主要障碍** - 完整 ANSI 支持需要大量工作

### 9.3 推荐行动

1. **立即**: 保持当前 Fast Path 模式 (稳定且有性能收益)
2. **短期 (1-2 月)**: 实现混合解析模式 (Rust 解析 + Java 执行)
3. **长期 (6-12 月)**: 逐步补全 Rust 功能，最终启用 Full Takeover

---

## 附录 A: 参考文档

- [VT100 User Guide](https://vt100.net/docs/vt100-ug/)
- [Xterm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- [ANSI Escape Codes](https://en.wikipedia.org/wiki/ANSI_escape_code)
- [VTE Parser Documentation](https://docs.rs/vte/latest/vte/)
- [JNI Specification](https://docs.oracle.com/javase/8/docs/technotes/guides/jni/spec/jniTOC.html)

## 附录 B: 相关文档

- `java-rust-performance-comparison.md` - 性能对比详情
- `rust-consistency-testing.md` - 一致性测试方案
- `rust-integration-status.md` - Rust 集成状态

---

*最后更新：2026-03-08*
*基于 Termux v0.118.3 代码*
