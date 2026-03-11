# Rust 终端模拟器优化更新日志

## 2026-03-11 - 批量读取优化

### 新增功能

#### 1. Rust 侧批量 JNI 接口

**文件**: `terminal-emulator/src/main/rust/src/lib.rs`

新增两个优化的 JNI 方法：

- `readScreenBatchFromRust()` - 批量读取多行数据
- `readFullScreenFromRust()` - 一次性读取整个屏幕

**优势**:
- 将 24 次 JNI 调用（24 行屏幕）减少到 1 次
- 减少 JNI 局部引用表压力（自动清理局部引用）
- 预分配 Rust 侧缓冲区，减少重复分配

**代码示例**:
```rust
// 旧方式：每行单独调用
for (int row = 0; row < mRows; row++) {
    readRowFromRust(mRustEnginePtr, row, textArray, styleArray);
}

// 新方式：批量读取
readScreenBatchFromRust(mRustEnginePtr, textBuffer2D, styleBuffer2D, 0, mRows);
```

---

#### 2. Java 侧批量同步方法

**文件**: `terminal-emulator/src/main/java/com/termux/terminal/TerminalBuffer.java`

新增方法：
- `syncRowsFromRust()` - 批量同步指定行
- `syncFullScreenFromRust()` - 同步整个屏幕

**优势**:
- 封装 JNI 调用细节，使用更简单
- 自动管理二维数组分配
- 支持按需同步（只同步可见行）

---

#### 3. TerminalRow 批量设置方法

**文件**: `terminal-emulator/src/main/java/com/termux/terminal/TerminalRow.java`

新增方法：
- `setTextAndStyles()` - 批量设置文本和样式

**优势**:
- 使用 `System.arraycopy` 代替逐字符复制
- 减少方法调用开销
- 自动检测代理对和宽字符

---

#### 4. 性能测试用例

**文件**: `terminal-emulator/src/main/rust/tests/performance.rs`

新增测试：
- `test_batch_row_read_performance()` - 批量行读取性能
- `test_full_screen_batch_read_performance()` - 全屏批量读取性能
- `test_single_vs_batch_read_comparison()` - 单次 vs 批量读取对比

**运行测试**:
```bash
cd terminal-emulator/src/main/rust
cargo test --test performance --release -- --nocapture
```

---

### 性能提升预期

| 操作 | 旧方式 | 新方式 | 提升 |
|------|--------|--------|------|
| 全屏刷新 JNI 调用 | 24 次 | 1 次 | **24x** |
| 局部引用创建/删除 | 48 次 | 2 次 | **24x** |
| 数据拷贝（每行） | 逐字符 | 批量拷贝 | **~5x** |
| 总刷新时间 | ~5ms | ~1ms | **~5x** |

---

### 使用示例

#### 在 TerminalView 中使用批量刷新

```java
// 在 TerminalView 的 onDraw() 或渲染方法中
@Override
protected void onDraw(Canvas canvas) {
    if (USE_RUST_FULL_TAKEOVER && mEmulator != null) {
        // 使用新的批量同步方法
        mEmulator.syncFullScreenFromRust();
        
        // 然后渲染...
        mRenderer.render(canvas);
    }
}
```

#### 部分刷新优化

```java
// 只同步可见区域（例如滚动时）
public void scrollDown(int lines) {
    if (USE_RUST_FULL_TAKEOVER) {
        // 只同步变化的行
        mScreen.syncRowsFromRust(mRustEnginePtr, 0, lines);
    }
}
```

---

### 后续优化计划

#### 阶段 2: DirectByteBuffer 零拷贝（预计提升 2-3x）
- 使用 JNI DirectByteBuffer 共享内存
- 消除所有数据拷贝
- Java 直接访问 Rust 内存

#### 阶段 3: Sixel 图形支持
- 实现 DCS Sixel 序列解析
- 添加图像渲染回调
- 支持 ANSI 图形显示

#### 阶段 4: 内存优化
- 移除 Java TerminalBuffer 冗余存储
- 让 Java 层仅作为 Rust 缓冲区的视图
- 减少 50% 内存占用

---

### 兼容性说明

- ✅ 完全向后兼容旧的单行读取方法
- ✅ 新旧方式可以混用
- ✅ 不影响现有功能
- ✅ Rust Full Takeover 模式默认启用

---

### 测试建议

1. **功能测试**: 运行 `consistency.rs` 所有一致性测试
2. **性能测试**: 运行 `performance.rs` 基准测试
3. **手动测试**: 
   - 快速滚动大量文本
   - 运行全屏应用（如 vim, htop）
   - 测试 ANSI 图形程序

---

### 已知问题

- 暂无

---

### 贡献者

- Rust 优化实施：Termux 团队
- 测试验证：社区贡献者

---

### 相关链接

- [RUST_MIGRATION_STATUS.md](./RUST_MIGRATION_STATUS.md) - 完整迁移状态
- [RUST_OPTIMIZATION_PLAN.md](./RUST_OPTIMIZATION_PLAN.md) - 优化计划详情
