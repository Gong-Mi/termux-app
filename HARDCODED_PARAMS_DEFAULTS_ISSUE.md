# Rust 硬编码参数默认值问题报告

## 问题概述

Rust 版本使用 `unwrap_or(1)` 和 `unwrap_or(0)` 硬编码参数默认值，而 Java upstream 使用 `getArg0(defaultValue)` 方法，支持**可配置的默认值**。

## 关键差异

### Java 参数处理逻辑

```java
// TerminalEmulator.java
private int getArg0(int defaultValue) {
    return getArg(0, defaultValue, true);
}

private int getArg(int index, int defaultValue, boolean treatZeroAsDefault) {
    int result = mArgs[index];
    if (result < 0 || (result == 0 && treatZeroAsDefault)) {
        result = defaultValue;  // ✅ 使用传入的默认值
    }
    return result;
}
```

**关键点**:
- `getArg0(0)` - 默认值为 0
- `getArg0(1)` - 默认值为 1
- `getArg0(-1)` - 默认值为 -1（表示未指定）
- `treatZeroAsDefault=true` - 0 被视为默认值

### Rust 参数处理逻辑

```rust
// csi.rs - 硬编码默认值
let n = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0) as i32;
```

**问题**:
- ❌ 默认值硬编码为 1 或 0
- ❌ 无法区分"未指定"和"显式为 0"
- ❌ 某些命令的默认值可能不正确

## 详细对比表

### CSI 命令参数默认值

| CSI 命令 | 功能 | Java 默认值 | Rust 默认值 | 状态 |
|----------|------|------------|------------|------|
| `@` | ICH - 插入字符 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `A` | CUU - 光标上移 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `B` | CUD - 光标下移 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `C` / `a` | CUF - 光标右移 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `D` | CUB - 光标左移 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `E` | CNL - 下一行 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `F` | CPL - 上一行 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `G` / `\`` | CHA - 水平绝对 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `H` / `f` | CUP - 光标位置 | `getArg1(1)` = 1, `getArg0(1)` = 1 | `unwrap_or(1)`, `unwrap_or(1)` | ✅ |
| `I` | CHT - 水平制表 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `J` | ED - 清屏 | `getArg0(0)` = 0 | `unwrap_or(0)` = 0 | ✅ |
| `K` | EL - 清行 | `getArg0(0)` = 0 | `unwrap_or(0)` = 0 | ✅ |
| `L` | IL - 插入行 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `M` | DL - 删除行 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `P` | DCH - 删除字符 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `S` | SU - 上滚 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `T` | SD - 下滚 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `X` | ECH - 擦除字符 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `Z` | CBT - 后制表 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `b` | REP - 重复 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `c` | DA - 设备属性 | N/A (无参数) | N/A | ✅ |
| `d` | VPA - 垂直绝对 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `e` | VPR - 垂直相对 | `getArg0(1)` = 1 | `unwrap_or(1)` = 1 | ✅ |
| `g` | TBC - 清除制表 | `getArg0(0)` = 0 | `unwrap_or(0)` = 0 | ✅ |
| `h` | SM - 设置模式 | 遍历所有参数 | 遍历所有参数 | ✅ |
| `l` | RM - 重置模式 | 遍历所有参数 | 遍历所有参数 | ✅ |
| `m` | SGR - 图形渲染 | 特殊处理 | 特殊处理 | ✅ |
| `n` | DSR - 设备状态 | `getArg0(-1)` = -1 | `unwrap_or(0)` = 0 | ⚠️ |
| `p` | DECSTR - 软重置 | N/A (带 `!`) | N/A (带 `!`) | ✅ |
| `r` | DECSTBM - 滚动区域 | `getArg1(rows)` = rows, `getArg0(1)` = 1 | `unwrap_or(state.rows)`, `unwrap_or(1)` | ⚠️ |
| `s` | 保存/左右边距 | `getArg1(cols)` = cols, `getArg0(1)` = 1 | `unwrap_or(state.cols)`, `unwrap_or(1)` | ⚠️ |
| `u` | RC - 恢复光标 | N/A | N/A | ✅ |
| `x` | DECREQTPARM - 请求参数 | N/A | N/A | ✅ |

### 潜在问题命令

#### 1. DSR (Device Status Report) - `CSI n`

**Java**:
```java
switch (getArg0(-1)) {
    case 6:  // 只有显式为 6 时才报告光标位置
        reportCursorPosition();
}
```

**Rust**:
```rust
let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0) as i32;
match mode {
    5 => report("\x1b[0n"),
    6 => report_cursor(),  // ⚠️ 默认 0 可能意外触发
    _ => {}
}
```

**问题**: Java 使用 `-1` 作为默认值，只有显式参数才会匹配。Rust 使用 `0` 作为默认值。

#### 2. DECSTBM (Set Top and Bottom Margins) - `CSI r`

**Java**:
```java
int top = getArg0(1);           // 默认 1
int bottom = getArg1(mRows);    // 默认屏幕行数
setMargins(top, bottom);
```

**Rust**:
```rust
let top = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
let bottom = iter.next().and_then(|p| p.first()).copied().unwrap_or(state.rows as i32) as i32;
```

**状态**: ✅ 默认值正确

#### 3. DECSLRM (Set Left and Right Margins) - `CSI s`

**Java**:
```java
if (leftright_margin_mode) {
    int left = getArg0(1);       // 默认 1
    int right = getArg1(mCols);  // 默认屏幕列数
    setLeftRightMargins(left, right);
}
```

**Rust**:
```rust
if state.leftright_margin_mode() {
    let left = iter.next().and_then(|p| p.first()).copied().unwrap_or(1) as i32;
    let right = iter.next().and_then(|p| p.first()).copied().unwrap_or(state.cols as i32) as i32;
}
```

**状态**: ✅ 默认值正确

## 需要修复的问题

### 1. DSR 命令默认值

**当前**:
```rust
let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0) as i32;
```

**应改为**:
```rust
// DSR: 默认 -1 表示无参数，只有显式值才处理
let mode = if params.len == 0 { -1 } else { params.get(0, 0) };
```

### 2. 参数处理辅助函数

建议添加类似 Java 的辅助函数：

```rust
impl Params {
    /// Get parameter with default value (treats 0 as default)
    pub fn get_with_default(&self, index: usize, default: i32, treat_zero_as_default: bool) -> i32 {
        if index < self.len {
            let val = self.values[index];
            if val < 0 || (val == 0 && treat_zero_as_default) {
                default
            } else {
                val
            }
        } else {
            default
        }
    }
}
```

## 统计

| 类别 | 数量 |
|------|------|
| 完全一致的命令 | 24 |
| 默认值正确但实现不同 | 3 |
| 潜在问题命令 | 1 (DSR) |
| 硬编码 `unwrap_or(1)` | 22 处 |
| 硬编码 `unwrap_or(0)` | 5 处 |

## 建议

1. **添加参数辅助函数** - 模仿 Java 的 `getArg()` 行为
2. **修复 DSR 命令** - 使用 `-1` 作为默认值
3. **添加单元测试** - 验证各命令的参数默认值行为
4. **文档化默认值** - 在每个 CSI 处理函数中注释默认值

## 参考

- VT100/ANSI 标准：https://vt100.net/docs/vt510-rm/
- Java 实现：`TerminalEmulator.java:2269-2283`
- Rust 实现：`terminal/handlers/csi.rs`
