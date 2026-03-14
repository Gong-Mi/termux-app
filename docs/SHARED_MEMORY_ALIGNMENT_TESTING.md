# 共享内存对齐测试方法论

## 概述

本文档描述了 Termux Rust 终端引擎中 Java 与 Rust 之间共享内存对齐的测试方法论。通过系统化的方法确保两个语言之间的内存布局完全一致，避免因偏移量计算错误导致的崩溃或数据显示错误。

---

## 背景

### 架构设计

Rust 终端引擎通过共享内存与 Java 层通信：

```
┌─────────────────────────────────────────────────────────────┐
│                    Shared Memory Buffer                      │
├──────────────┬────────────────────────┬──────────────────────┤
│   Header     │     Text Data          │    Style Data        │
│  (16 bytes)  │  (u16[cell_count])     │   (u64[cell_count])  │
│              │   @ offset 16          │  @ offset 16+text    │
└──────────────┴────────────────────────┴──────────────────────┘
```

### Header 布局 (16 字节固定)

| 偏移 | 字段 | 类型 | 说明 |
|------|------|------|------|
| 0 | version | u8 | 版本标志/同步锁 |
| 1-3 | padding | [u8; 3] | 对齐填充 |
| 4 | cols | u32 | 列数 |
| 8 | rows | u32 | 行数 |
| 12 | style_offset | u32 | 样式数据起始偏移 |

---

## 问题历史

### 症状

在调试过程中发现：
- Java 侧在偏移 16014 找到了样式数据
- Rust 侧声称写入偏移 16016
- 2 字节偏差导致 `ArrayIndexOutOfBoundsException`

### 根本原因分析

1. **`size_of` 不确定性**: Rust 使用 `std::mem::size_of::<SharedScreenBuffer>()` 计算 Header 大小，但该值可能因编译器优化而变化
2. **公式不一致**: `create_shared_buffer` 和 `sync_to_shared` 使用了不同的计算方式
3. **repr(C) 误解**: 虽然结构体标记为 `#[repr(C)]`，但零大小数组 `[u16; 0]` 的布局可能不明确

---

## 测试方法论

### 第一阶段：静态验证

#### 1.1 结构体布局验证

```rust
// 在编译时验证结构体布局
#[test]
fn test_shared_screen_buffer_layout() {
    assert_eq!(std::mem::offset_of!(SharedScreenBuffer, cols), 4);
    assert_eq!(std::mem::offset_of!(SharedScreenBuffer, rows), 8);
    assert_eq!(std::mem::offset_of!(SharedScreenBuffer, style_offset), 12);
    assert_eq!(std::mem::size_of::<SharedScreenBuffer>(), 16);
}
```

#### 1.2 内存大小计算验证

```rust
#[test]
fn test_required_size_calculation() {
    let cols = 80;
    let rows = 24;
    let expected = 16 + (cols * rows * 2).next_multiple_of(8) + (cols * rows * 8);
    assert_eq!(SharedScreenBuffer::required_size(cols, rows), expected);
}
```

### 第二阶段：动态验证

#### 2.1 标记值注入测试

在 Rust 侧写入独特的标记值，在 Java 侧验证位置：

```rust
// Rust 侧
std::ptr::write(base_ptr.add(0) as *mut u8, 0xABu8);  // 独特 version 值
std::ptr::write(base_ptr.add(2) as *mut u16, 0xDEADu16);  // 独特标记
```

```java
// Java 侧
byte version = shared.get(0);
short marker = shared.getShort(2);
assert version == (byte)0xAB;
assert marker == (short)0xDEAD;
```

#### 2.2 样式值扫描测试

```java
// Java 侧扫描验证
long expectedStyle = 0x1000001010000L;  // 默认样式值
long actualStyle = shared.getLong(styleOffsetFromHeader);
assert actualStyle == expectedStyle;
```

### 第三阶段：端到端验证

#### 3.1 完整数据流测试

```java
public static void main(String[] args) {
    // 1. 创建引擎
    long enginePtr = createEngineRustWithCallback(80, 24, 100, 10, 20, callback);
    
    // 2. 写入测试数据
    processEngineRust(enginePtr, "\u001b[31mRED".getBytes(), 0, 5);
    
    // 3. 同步到共享内存
    syncToSharedBufferRust(enginePtr);
    
    // 4. 获取并验证
    ByteBuffer shared = createSharedBufferRust(enginePtr);
    shared.order(ByteOrder.LITTLE_ENDIAN);
    
    // 5. 验证 Header
    int styleOffset = shared.getInt(12);
    assert styleOffset == 16016;
    
    // 6. 验证样式数据位置
    long firstStyle = shared.getLong(styleOffset);
    assert firstStyle == 0x1000001010000L;
    
    System.out.println("✓ Physical alignment verified!");
}
```

#### 3.2 Resize 后验证

```java
// 调整大小后再次验证
resizeEngineRustFull(enginePtr, 40, 24);
syncToSharedBufferRust(enginePtr);

// 重新获取并验证
ByteBuffer shared2 = createSharedBufferRust(enginePtr);
int newStyleOffset = shared2.getInt(12);
long newFirstStyle = shared2.getLong(newStyleOffset);
assert newFirstStyle == 0x1000001010000L;
```

---

## 最佳实践

### 1. 固定 Header 大小

**不要**依赖 `size_of`：
```rust
// ❌ 错误：可能因编译器而异
let header_size = std::mem::size_of::<SharedScreenBuffer>();
```

**要**使用硬编码值：
```rust
// ✅ 正确：明确且一致
const HEADER_SIZE: u32 = 16;
let style_offset = HEADER_SIZE + aligned_text_size;
```

### 2. 手动寻址写入

**不要**通过结构体字段访问：
```rust
// ❌ 错误：可能跳过 padding 或使用错误偏移
shared.cols = self.cols as u32;
```

**要**使用显式指针写入：
```rust
// ✅ 正确：精确控制物理位置
std::ptr::write(base_ptr.add(4) as *mut u32, self.cols as u32);
```

### 3. 字节序一致性

确保 Rust 和 Java 使用相同的字节序：
```rust
// Rust 默认使用小端序 (Little Endian)
let style_bytes = style_val.to_le_bytes();
```

```java
// Java 显式设置小端序
shared.order(ByteOrder.LITTLE_ENDIAN);
```

### 4. 对齐要求

```rust
// 确保 8 字节对齐用于 u64 数组
let layout = std::alloc::Layout::from_size_align(buffer_size, 8).unwrap();
let ptr = unsafe { std::alloc::alloc(layout) };
```

---

## 调试技巧

### 1. 逐字节打印

```rust
// Rust 侧打印内存内容
for i in 0..32 {
    let byte = unsafe { *base_ptr.add(i) };
    eprint!("{:02X ", byte);
}
eprintln!();
```

```java
// Java 侧打印内存内容
for (int i = 0; i < 32; i++) {
    System.out.printf("%02X ", shared.get(i));
}
System.out.println();
```

### 2. 独特标记值

在关键位置写入独特值用于追踪：
- `0xAB` - version 字段
- `0xDEAD` - padding 区域
- `0x1000001010000` - 默认样式值

### 3. 偏移计算表

| 字段 | Rust 计算 | Java 验证 | 预期值 |
|------|----------|----------|--------|
| Header | 固定 16 | `getInt(12)` 读取 | 16016 |
| Text | `16` | `getChar(16)` | 0x0020 (空格) |
| Style | `16 + aligned_text` | `getLong(offset)` | 0x1000001010000 |

---

## 测试用例模板

### 基础对齐测试

```rust
#[test]
fn test_java_rust_memory_alignment() {
    let cols = 80;
    let rows = 24;
    let mut buffer = FlatScreenBuffer::new(cols, rows);
    
    // 创建共享缓冲区
    let shared_ptr = buffer.create_shared_buffer();
    
    // 验证 Header 写入
    unsafe {
        assert_eq!(*(shared_ptr as *const u8).add(4) as *const u32, 80);
        assert_eq!(*(shared_ptr as *const u8).add(8) as *const u32, 24);
        assert_eq!(*(shared_ptr as *const u8).add(12) as *const u32, 16016);
    }
}
```

### 样式数据位置测试

```java
@Test
public void testStyleDataPosition() {
    ByteBuffer shared = createSharedBufferRust(enginePtr);
    shared.order(ByteOrder.LITTLE_ENDIAN);
    
    int styleOffset = shared.getInt(12);
    long expectedOffset = 16 + (80 * 24 * 2 + 7) & !7;  // 16016
    
    assertEquals(expectedOffset, styleOffset);
    
    // 验证第一个样式值在预期位置
    long firstStyle = shared.getLong(styleOffset);
    assertEquals(0x1000001010000L, firstStyle);
}
```

---

## 故障排查流程

```
1. 发现偏差
   └─> Java 读取的值与 Rust 写入的值不匹配

2. 检查 Header 布局
   └─> 验证 cols/rows/style_offset 是否在预期偏移

3. 检查标记值
   └─> 写入独特值 (0xAB, 0xDEAD) 并验证位置

4. 逐字节对比
   └─> Rust 和 Java 分别打印同一段内存的字节

5. 定位偏移差异
   └─> 计算实际偏移与预期偏移的差值

6. 修复并重新验证
   └─> 更新计算公式，运行完整测试套件
```

---

## 结论

通过系统化的测试方法论，我们成功定位并修复了 Rust 与 Java 之间的共享内存对齐问题。关键要点：

1. **固定 Header 大小**：使用硬编码的 16 字节，不依赖 `size_of`
2. **手动寻址**：显式指定每个字段的物理偏移
3. **端到端验证**：从 Rust 写入到 Java 读取的完整数据流验证
4. **独特标记**：使用独特值追踪内存布局

此方法论可应用于任何跨语言共享内存场景，确保内存布局的一致性。
