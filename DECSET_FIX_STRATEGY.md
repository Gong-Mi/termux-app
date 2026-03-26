# DECSET 处理方案对比分析

## 现状

### 两个 DECSET 处理函数

1. **`do_decset_or_reset(setting, mode)`** (engine.rs:240)
   - 被 JNI 函数 `doDecSetOrResetFromRust()` 调用
   - Java 侧通过 `TerminalEmulator.doDecSetOrReset()` 调用
   - 处理 20+ 个模式

2. **`handle_decset(params, set)`** (engine.rs:662)
   - 被 CSI 处理器调用 (csi.rs)
   - 处理转义序列中的 DECSET 命令
   - 处理 12 个模式

### 问题

**12 个模式重复处理**，可能导致状态不一致。

---

## 方案 1：扩大 Rust 侧 `handle_decset()` 功能

### 实现

**删除 `do_decset_or_reset()`，统一使用 `handle_decset()`**

```rust
// engine.rs - 修改 handle_decset() 处理所有模式
pub fn handle_decset(&mut self, params: &Params, set: bool) {
    for param in params.iter() {
        for &p in param.iter() {
            match p {
                // 添加所有模式处理
                1 => { /* DECCKM */ }
                3 => { /* DECCOLM */ }
                4 => { /* DECSCLM */ }
                // ... 所有模式
            }
        }
    }
}

// lib.rs - 修改 JNI 函数
#[unsafe(no_mangle)]
pub extern "system" fn Java_..._doDecSetOrResetFromRust(
    env: JNIEnv, _class: JClass, ptr: jlong, setting: jboolean, mode: jint,
) {
    if ptr == 0 { return; }
    let context = unsafe { &*(ptr as *const TerminalContext) };
    let mut engine = context.lock.write().unwrap();
    
    // 使用 handle_decset() 处理
    let params = Params::new(&[mode as i64]);
    engine.state.handle_decset(&params, setting != 0);
}
```

### 优点

✅ **代码统一** - 只有一个 DECSET 处理函数
✅ **状态一致** - 不会有不同步问题
✅ **易于维护** - 修改一处即可
✅ **符合架构** - DECSET 处理逻辑集中在 CSI 层

### 缺点

❌ **需要修改现有代码** - 迁移所有模式处理
❌ **测试工作量大** - 需要验证所有模式
❌ **API 变更** - JNI 接口参数变化

### 适用场景

- 长期维护
- 代码质量优先
- 有充足测试资源

---

## 方案 2：让 `do_decset_or_reset()` 暴露给 Java（保持现状但修复同步）

### 实现

**保留两个函数，但让 `do_decset_or_reset()` 调用 `handle_decset()`**

```rust
// engine.rs - 修改 do_decset_or_reset()
pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
    // 统一使用 handle_decset() 处理，避免重复代码
    let params = Params::new(&[mode as i64]);
    self.handle_decset(&params, setting);
}
```

### 优点

✅ **改动最小** - 只需修改一个函数
✅ **保持 API 稳定** - Java 侧不需要修改
✅ **快速修复** - 立即解决重复问题
✅ **向后兼容** - 不影响现有调用

### 缺点

❌ **保留冗余函数** - 代码结构不够优雅
❌ **性能开销** - 多一层函数调用（可忽略）
❌ **技术债务** - 未来可能需要清理

### 适用场景

- 快速修复
- 稳定优先
- 测试资源有限

---

## 方案 3：混合方案（推荐）

### 实现

**短期：方案 2（快速修复）**
**长期：方案 1（代码优化）**

```rust
// 第一阶段：快速修复（当前）
pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
    let params = Params::new(&[mode as i64]);
    self.handle_decset(&params, setting);
}

// 第二阶段：清理冗余（未来）
// 删除 do_decset_or_reset()，直接修改 JNI 调用
```

### 优点

✅ **立即解决问题** - 状态不一致风险消除
✅ **风险可控** - 小改动，易测试
✅ **有优化路径** - 未来可以清理代码
✅ **保持灵活性** - 可以根据实际情况调整

---

## 推荐方案

### 立即执行：方案 2（最小改动修复）

```rust
// 在 engine.rs 中
pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
    // 委托给 handle_decset() 处理，避免代码重复和状态不一致
    let params = Params::new(&[mode as i64]);
    self.handle_decset(&params, setting);
}
```

**理由**：
1. **改动最小** - 只修改一个函数
2. **风险最低** - 不影响现有 API
3. **立即生效** - 解决状态不一致问题
4. **易于回滚** - 如果发现问题可快速恢复

### 未来优化：方案 1（代码清理）

在测试覆盖充足后：
1. 删除 `do_decset_or_reset()`
2. 修改 JNI 直接调用 `handle_decset()`
3. 清理冗余的模式处理代码

---

## DECSET 标志位同步修复

### 问题

```rust
// 当前代码 - 可能不同步
1 => {
    if setting { self.modes.set(DECSET_BIT_APPLICATION_CURSOR_KEYS); }
    else { self.modes.reset(DECSET_BIT_APPLICATION_CURSOR_KEYS); }
    self.application_cursor_keys = setting;  // ← 独立字段
}
```

### 修复

**移除独立字段，统一使用 `modes.flags`**

```rust
// 1. 删除 application_cursor_keys 字段
// 2. 添加访问方法
pub fn application_cursor_keys(&self) -> bool {
    self.modes.is_enabled(DECSET_BIT_APPLICATION_CURSOR_KEYS)
}

// 3. 修改所有使用位置
// 旧代码：if self.application_cursor_keys { ... }
// 新代码：if self.application_cursor_keys() { ... }
```

---

## 测试验证清单

修复后需要验证：

### DECSET 模式测试
- [ ] DECCKM (模式 1) - 光标键模式切换
- [ ] DECCOLM (模式 3) - 132 列模式
- [ ] DECSCNM (模式 5) - 反色显示
- [ ] DECOM (模式 6) - 原点模式
- [ ] DECAWM (模式 7) - 自动换行
- [ ] DECTCEM (模式 25) - 光标显示/隐藏
- [ ] DECLRMM (模式 69) - 左右边距模式
- [ ] 鼠标追踪模式 (1000/1002/1003/1006)
- [ ] 焦点事件 (1004)
- [ ] 备用屏幕 (1047/1048/1049)
- [ ] 括号粘贴 (2004)

### 状态同步测试
- [ ] 设置 DECSET 后查询状态正确
- [ ] 设置 DECRST 后查询状态正确
- [ ] 多次切换状态正确
- [ ] 保存/恢复光标时 DECSET 状态正确

### 性能测试
- [ ] 大量 DECSET 命令处理性能
- [ ] 内存使用无异常增长

---

## 结论

**推荐立即执行方案 2**：
```rust
pub fn do_decset_or_reset(&mut self, setting: bool, mode: u32) {
    let params = Params::new(&[mode as i64]);
    self.handle_decset(&params, setting);
}
```

**理由**：改动最小，风险最低，立即解决问题。
