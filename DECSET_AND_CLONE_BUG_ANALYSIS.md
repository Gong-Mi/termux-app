# DECSET 标志位误用和深克隆逻辑分析

## 问题 1：DECSET 处理函数重复

### 现状

代码中有**两个**DECSET 处理函数：

1. **`do_decset_or_reset()`** (engine.rs:240)
   - 被 JNI 函数 `Java_..._doDecSetOrResetFromRust()` 调用 (lib.rs:211)
   - 处理模式：1, 3, 4, 5, 6, 7, 12, 25, 40, 45, 66, 69, 1000, 1002, 1003, 1004, 1006, 1034, 1048, 47/1047/1049, 2004

2. **`handle_decset()`** (engine.rs:662)
   - 被 CSI 处理器调用 (csi.rs:128, 132)
   - 处理模式：1, 5, 6, 7, 25, 69, 1000, 1002, 1004, 1006, 1047/1048/1049, 2004

### 重叠的模式

| 模式 | 功能 | do_decset_or_reset | handle_decset | 问题 |
|------|------|-------------------|---------------|------|
| 1 | DECCKM (光标键) | ✅ | ✅ | 重复处理 |
| 5 | DECSCNM (反色) | ✅ | ✅ | 重复处理 |
| 6 | DECOM (原点模式) | ✅ | ✅ | 重复处理 |
| 7 | DECAWM (自动换行) | ✅ | ✅ | 重复处理 |
| 25 | DECTCEM (光标显示) | ✅ | ✅ | 重复处理 |
| 69 | DECLRMM (左右边距) | ✅ | ✅ | 重复处理 |
| 1000 | 鼠标追踪 | ✅ | ✅ | 重复处理 |
| 1002 | 鼠标按钮事件 | ✅ | ✅ | 重复处理 |
| 1004 | 焦点事件 | ✅ | ✅ | 重复处理 |
| 1006 | SGR 鼠标 | ✅ | ✅ | 重复处理 |
| 1047/1048/1049 | 备用屏幕 | ✅ | ✅ | 重复处理 |
| 2004 | 括号粘贴 | ✅ | ✅ | 重复处理 |

### 潜在问题

1. **状态不一致** - 同一个 DECSET 命令可能被两个函数处理，导致状态不同步
2. **标志位冲突** - `modes.flags` 和独立字段（如 `application_cursor_keys`）可能不一致
3. **维护困难** - 修改一个函数时需要同时修改另一个

### 解决方案

**方案 1：统一使用 `handle_decset()`**

```rust
// 删除 do_decset_or_reset()，所有 DECSET 处理都通过 handle_decset()
// 修改 JNI 调用：
#[unsafe(no_mangle)]
pub extern "system" fn Java_..._doDecSetOrResetFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, setting: jboolean, mode: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    
    // 使用 handle_decset() 而不是 do_decset_or_reset()
    let params = Params::new(&[mode as i64]);
    engine.state.handle_decset(&params, setting != 0);
}
```

**方案 2：让 `do_decset_or_reset()` 调用 `handle_decset()`**

```rust
pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
    // 统一使用 handle_decset() 处理
    let params = Params::new(&[mode as i64]);
    self.handle_decset(&params, setting);
}
```

---

## 问题 2：深克隆逻辑

### 现状

```rust
#[derive(Clone)]
pub struct TerminalRow {
    pub text: Vec<char>,
    pub styles: Vec<u64>,
    pub line_wrap: bool,
}

// 滚动时使用 clone()
self.buffer[i] = self.buffer[i + 1].clone();
```

### 分析

`#[derive(Clone)]` 对 `Vec<T>` 会进行**深克隆**：
- `Vec<char>` - 深克隆（复制所有字符）
- `Vec<u64>` - 深克隆（复制所有样式）
- `line_wrap: bool` - 复制值

**这是正确的行为** ✅

### 性能考虑

每次滚动都会克隆整行数据（字符 + 样式），可能影响性能。

**优化方案**：

1. **使用 Arc 共享数据**（只读场景）
2. **使用写时复制（Copy-on-Write）**
3. **使用环形缓冲区指针**（Java 方案）

但当前实现功能正确，性能优化可以后续进行。

---

## 问题 3：DECSET 标志位同步

### 现状

```rust
// do_decset_or_reset() 中
1 => {
    if setting { self.modes.set(DECSET_BIT_APPLICATION_CURSOR_KEYS); }
    else { self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS); }
    self.application_cursor_keys = setting;  // ← 独立字段
}
```

### 问题

`modes.flags` 和 `application_cursor_keys` 字段**可能不同步**：
- 如果只修改 `modes.flags`，`application_cursor_keys` 不会更新
- 如果只修改 `application_cursor_keys`，`modes.flags` 不会更新

### 解决方案

**方案 1：移除独立字段，统一使用 `modes.flags`**

```rust
// 删除 application_cursor_keys 字段
pub fn application_cursor_keys(&self) -> bool {
    self.modes.is_enabled(DECSET_BIT_APPLICATION_CURSOR_KEYS)
}
```

**方案 2：添加同步检查**

```rust
// 在设置时同步
self.application_cursor_keys = setting;
if setting {
    self.modes.set(DECSET_BIT_APPLICATION_CURSOR_KEYS);
} else {
    self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS);
}
```

---

## 建议修复优先级

### 高优先级（功能正确性）

1. **统一 DECSET 处理函数** - 避免状态不一致
2. **同步 DECSET 标志位** - 避免字段不同步

### 中优先级（代码质量）

3. **移除冗余字段** - 简化代码结构
4. **添加注释** - 说明 DECSET 处理逻辑

### 低优先级（性能优化）

5. **优化深克隆** - 考虑使用更高效的内存管理

---

## 测试验证

修复后需要验证：
1. DECSET/DECRST 命令正确响应
2. 光标键模式（DECCKM）正确切换
3. 自动换行（DECAWM）正确工作
4. 鼠标追踪模式正确设置
5. 备用屏幕切换正常
