# Rust 终端模拟器优化计划

## 当前架构分析

### Full Takeover 模式现状
```
[Java TerminalView] - 渲染层
    ↓
[Java TerminalEmulator] - JNI 壳 + 状态查询
    ↓ (JNI 边界)
[Rust TerminalEngine] - 完整解析器 + 状态机
    ↓
[Rust ScreenState] - 双缓冲区 (main/alt) + 回调
```

### 存在的问题

1. **双重缓冲区冗余**
   - Java `TerminalBuffer` 仍存储完整屏幕数据
   - Rust `ScreenState` 维护独立的 `main_buffer`/`alt_buffer`
   - 内存占用翻倍，同步开销大

2. **JNI 调用频率过高**
   - 每次渲染调用 `readRowFromRust` 逐行拷贝
   - 每行都需要单独的 JNI 调用和数组分配
   - 大量 `GetPrimitiveArrayCritical` / `ReleasePrimitiveArrayCritical` 开销

3. **缺少批量操作优化**
   - 全屏刷新需要 N 次 JNI 调用（N=行数）
   - 每次调用都涉及 Java 数组分配和数据拷贝

4. **Sixel 图形未实现**
   - DCS 序列解析框架已完成
   - Sixel 图像解码和渲染逻辑缺失

---

## 优化方案

### 阶段 1: 减少 JNI 调用开销 (高优先级)

#### 1.1 批量行读取
**当前**: 每行单独调用 `readRowFromRust`
```rust
// Java 侧循环调用
for (int row = 0; row < mRows; row++) {
    readRowFromRust(mRustEnginePtr, row, textArray, styleArray);
}
```

**优化**: 单次 JNI 调用获取整个屏幕
```rust
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_terminal_TerminalEmulator_readScreenBatchFromRust(
    env_ptr: *mut *const JNINativeInterface_,
    _class: jclass,
    engine_ptr: jlong,
    dest_text: jobjectArray,    // 2D char array
    dest_style: jobjectArray,   // 2D long array
    start_row: jint,
    num_rows: jint,
) {
    // 一次性拷贝多行数据
}
```

**预期收益**: 减少 24x JNI 调用（24 行屏幕 → 1 次调用）

#### 1.2 直接内存映射 (零拷贝)
**目标**: 使用 JNI `DirectByteBuffer` 共享内存
```rust
// Rust 分配直接内存
let direct_buffer = env.new_direct_byte_buffer(&mut screen_data)?;

// Java 直接访问，无需拷贝
ByteBuffer screenBuffer = readScreenDirect(enginePtr);
char[] text = screenBuffer.asCharBuffer();
```

**预期收益**: 消除所有数据拷贝，仅保留 JNI 边界开销

---

### 阶段 2: 优化 Java TerminalBuffer (中优先级)

#### 2.1 瘦身 Java TerminalBuffer
**当前**: `TerminalBuffer` 存储完整的 `TerminalRow[]`
```java
public final class TerminalBuffer {
    TerminalRow[] mLines;  // 完整存储屏幕数据
    int mScreenRows, mColumns;
    // ...
}
```

**优化**: 转换为 Rust 缓冲区的视图层
```java
public final class TerminalBuffer {
    private final long mRustEnginePtr;  // Rust 引擎指针
    
    // 仅作为访问接口，不存储数据
    public void getRow(int row, char[] text, long[] style) {
        TerminalEmulator.readRowFromRust(mRustEnginePtr, row, text, style);
    }
    
    public String getSelectedText(...) {
        // 按需从 Rust 同步数据
        syncVisibleRows();
        // ...
    }
}
```

**预期收益**: 减少 50% 内存占用（移除重复存储）

#### 2.2 延迟同步策略
```java
// 仅在需要时同步行
private final BitSet mDirtyRows = new BitSet();

public void syncRowIfNeeded(int row) {
    if (mDirtyRows.get(row)) {
        readRowFromRust(mRustEnginePtr, row, mTextBuffer, mStyleBuffer);
        mDirtyRows.clear(row);
    }
}

public void markAllRowsDirty() {
    mDirtyRows.set(0, mRows);
}
```

---

### 阶段 3: 实现 Sixel 图形支持 (中优先级)

#### 3.1 Sixel 状态机
```rust
pub struct SixelDecoder {
    state: SixelState,
    params: Vec<i32>,
    pixel_data: Vec<u8>,
    current_color: u32,
    width: i32,
    height: i32,
}

enum SixelState {
    Ground,
    Param,
    Pixel,
    Macro,
}
```

#### 3.2 DCS 序列集成
```rust
fn dcs_dispatch(&mut self, data: &[u8]) {
    if data.starts_with(b"q") {
        // Sixel 图像数据
        self.state.sixel_decoder.process(&data[1..]);
    }
}
```

#### 3.3 图像渲染回调
```rust
fn report_sixel_image(&self, image: &SixelImage) {
    // 通过 JNI 发送图像到 Java 渲染
    // Java 侧使用 Bitmap 显示
}
```

---

### 阶段 4: 性能基准测试 (低优先级)

#### 4.1 添加性能测试
```rust
#[test]
fn benchmark_screen_refresh() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 填充屏幕内容
    for i in 0..24 {
        engine.process_bytes(format!("\r\x1b[{};1HLine {}", i + 1, i).as_bytes());
    }
    
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        // 模拟全屏刷新
        for row in 0..24 {
            let mut text = [0u16; 80];
            engine.state.copy_row_text(row, &mut text);
        }
    }
    println!("1000 次全屏刷新耗时：{:?}", start.elapsed());
}
```

---

## 实施时间表

| 阶段 | 任务 | 预计工时 | 优先级 |
|------|------|----------|--------|
| 1.1 | 批量行读取 JNI | 2 小时 | 🔴 高 |
| 1.2 | DirectByteBuffer 零拷贝 | 4 小时 | 🟡 中 |
| 2.1 | 瘦身 Java TerminalBuffer | 3 小时 | 🟡 中 |
| 3.1 | Sixel 状态机 | 8 小时 | 🟡 中 |
| 3.2 | Sixel 渲染集成 | 4 小时 | 🟢 低 |
| 4.1 | 性能基准测试 | 2 小时 | 🟢 低 |

---

## 预期成果

1. **内存优化**: 减少 40-50% 内存占用
2. **性能提升**: 屏幕刷新速度提升 3-5x
3. **功能完整**: 支持 Sixel 图形显示
4. **代码质量**: 更清晰的 Rust/Java 职责分离
