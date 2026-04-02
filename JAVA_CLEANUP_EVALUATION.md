# Java 类清理评估报告

## 待清理 Java 类概览

| 文件 | 行数 | Rust 替代 | 使用次数 | 清理难度 |
|------|------|----------|----------|----------|
| `WcWidth.java` | 573 | ✅ `unicode-width` crate | 31 次 | ⭐ 简单 |
| `TerminalBuffer.java` | 497 | ✅ `screen.rs` | 待检查 | ⭐⭐ 中等 |
| `TerminalRow.java` | 201 | ✅ `screen.rs` | 待检查 | ⭐⭐ 中等 |
| `TextStyle.java` | 90 | ✅ `style.rs` | 待检查 | ⭐ 简单 |
| `ByteQueue.java` | 108 | ✅ `pty.rs` | 待检查 | ⭐ 简单 |
| `Logger.java` | 80 | ✅ `log` crate | 待检查 | ⭐ 简单 |

---

## 1. WcWidth.java 评估

### 当前状态
- **Java 文件**: 573 行
- **Rust 替代**: `unicode-width` crate (已集成)
- **JNI 接口**: ✅ 已实现 `widthRust()`

### 使用位置 (31 处)

#### 源文件使用 (3 处)
1. `TerminalBuffer.java:305` - 计算代码点显示宽度
2. `TerminalRow.java:41,61,68,84,104,145,147,182` - 行文本宽度计算
3. `terminal-view/TerminalRenderer.java:128,165` - 渲染时宽度计算
4. `terminal-view/TextSelectionCursorController.java:325,327` - 文本选择光标

#### 测试文件使用 (28 处)
1. `TerminalRowTest.java` - 11 次
2. `TerminalTest.java` - 1 次
3. `TerminalTestCase.java` - 1 次
4. `WcWidthTest.java` - 1 次

### Rust 实现状态

**lib.rs**:
```rust
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_WcWidth_widthRust(
    _env: JNIEnv, _class: JClass, ucs: jint
) -> jint {
    crate::utils::get_char_width(ucs as u32) as jint
}
```

**utils.rs**:
```rust
pub fn get_char_width(ucs: u32) -> usize {
    if ucs == 0 { return 0; }
    use unicode_width::UnicodeWidthChar;
    if (ucs < 32) || (ucs >= 0x7F && ucs < 0xA0) { return 0; }
    c.width().unwrap_or(1)
}
```

### 清理步骤

#### 第一步：修改 WcWidth.java 委托给 Rust
```java
public static int width(int ucs) {
    if (JNI.sNativeLibrariesLoaded) {
        return widthRust(ucs);
    }
    // Fallback removed after cleanup
}
```

#### 第二步：修改测试
- `WcWidthTest.java` - 删除或迁移到 Rust
- `TerminalRowTest.java` - 更新测试调用

#### 第三步：删除 WcWidth.java
- 保留 JNI 接口声明
- 删除 Java 实现

### 清理难度：⭐ 简单

**原因**:
- ✅ Rust 实现已完成
- ✅ JNI 接口已存在
- ✅ 功能单一（只计算字符宽度）
- ⚠️ 需要修改测试文件

---

## 2. Logger.java 评估

### 当前状态
- **Java 文件**: 80 行
- **Rust 替代**: `log` crate + `android_logger`
- **使用**: 待检查

### 清理步骤
1. 检查所有 Java 日志调用
2. 迁移到 Android Log API
3. 删除 Logger.java

### 清理难度：⭐ 简单

---

## 3. TextStyle.java 评估

### 当前状态
- **Java 文件**: 90 行
- **Rust 替代**: `style.rs`
- **功能**: 样式编码（前景色、背景色、效果）

### Rust 实现
```rust
pub const STYLE_NORMAL: u64 = encode_style(COLOR_INDEX_FOREGROUND, COLOR_INDEX_BACKGROUND, 0);
pub fn encode_style(fore_color: u64, back_color: u64, effect: u64) -> u64 { ... }
pub fn decode_fore_color(style: u64) -> u64 { ... }
pub fn decode_back_color(style: u64) -> u64 { ... }
```

### 清理难度：⭐ 简单

---

## 4. ByteQueue.java 评估

### 当前状态
- **Java 文件**: 108 行
- **Rust 替代**: `pty.rs`
- **功能**: PTY 字节队列

### Rust 实现
```rust
// pty.rs - PTY 处理
```

### 清理难度：⭐ 简单

---

## 5. TerminalBuffer.java 评估

### 当前状态
- **Java 文件**: 497 行
- **Rust 替代**: `screen.rs`
- **功能**: 终端环形缓冲区

### 使用位置
- 待检查

### 清理难度：⭐⭐ 中等

**原因**:
- 文件较大
- 可能被多处引用
- 需要仔细验证

---

## 6. TerminalRow.java 评估

### 当前状态
- **Java 文件**: 201 行
- **Rust 替代**: `screen.rs`
- **功能**: 终端行数据结构

### 使用位置
- WcWidth.java 被它使用

### 清理难度：⭐⭐ 中等

---

## 推荐清理顺序

### 阶段 1：简单独立类（可立即清理）

1. **Logger.java** (80 行)
   - 最简单
   - 依赖少
   - 风险低

2. **WcWidth.java** (573 行)
   - Rust 实现完整
   - JNI 接口已存在
   - 需要修改测试

3. **TextStyle.java** (90 行)
   - 功能单一
   - Rust 已实现

4. **ByteQueue.java** (108 行)
   - 功能单一
   - Rust 已实现

### 阶段 2：核心数据结构（需要更多测试）

5. **TerminalRow.java** (201 行)
   - 需要验证 screen.rs 完整性

6. **TerminalBuffer.java** (497 行)
   - 最大文件
   - 需要最多验证

---

## 下一步行动

### 建议从 WcWidth.java 开始

**原因**:
1. ✅ Rust 实现最完整
2. ✅ JNI 接口已存在并工作
3. ✅ 功能单一，风险可控
4. ✅ 删除后收益大（573 行）

**步骤**:
1. 修改 WcWidth.java 完全委托给 Rust
2. 修改/删除 WcWidthTest.java
3. 更新其他测试文件
4. 删除 WcWidth.java

---

## 预估工作量

| 阶段 | 文件数 | 代码行数 | 预计时间 |
|------|--------|----------|----------|
| 阶段 1 | 4 个 | 851 行 | 2-3 小时 |
| 阶段 2 | 2 个 | 698 行 | 4-6 小时 |
| **总计** | **6 个** | **1549 行** | **6-9 小时** |
