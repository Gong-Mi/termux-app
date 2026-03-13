# Java vs Rust 性能对比方案

## 📋 概述

本文档说明了如何对比 Java 和 Rust 两个终端引擎实现的性能差异。

## 🎯 对比目标

1. **公平对比** - 使用相同的测试数据和场景
2. **可重复性** - 使用固定 seed 生成测试数据
3. **全面覆盖** - 覆盖多个关键性能指标
4. **自动化** - CI/CD 自动运行和生成报告

## 📊 对比指标

| 指标 | 说明 | 单位 |
|------|------|------|
| Raw Text Throughput | 纯文本处理吞吐量 | MB/s |
| ANSI Escape Throughput | ANSI 转义序列处理吞吐量 | MB/s |
| Cursor Movement | 光标移动操作频率 | K ops/s |
| Scrolling | 滚动操作频率 | K lines/s |
| Wide Char Processing | 宽字符（中文）处理频率 | K chars/s |
| Small Batch Calls | 小批量高频调用频率 | K calls/s |

## 🔧 测试文件

### Java 测试
- **文件**: `terminal-emulator/src/test/java/com/termux/terminal/JavaRustPerformanceComparisonTest.java`
- **运行方式**:
  ```bash
  ./gradlew :terminal-emulator:test --tests com.termux.terminal.JavaRustPerformanceComparisonTest --info
  ```

### Rust 测试
- **文件**: `terminal-emulator/src/main/rust/tests/performance.rs`
- **运行方式**:
  ```bash
  cd terminal-emulator/src/main/rust
  cargo test --test performance --release -- --nocapture
  ```

## 📁 输出格式

两个测试使用相同的输出格式，便于自动化解析：

```
[JAVA|RUST]_RAW_TEXT_MBPS=123.45
[JAVA|RUST]_ANSI_MBPS=67.89
[JAVA|RUST]_CURSOR_OPS=123.45
[JAVA|RUST]_SCROLL_LINES=67.89
[JAVA|RUST]_WIDECHAR_OPS=12.34
[JAVA|RUST]_SMALLBATCH_OPS=56.78
```

## 🚀 本地运行对比

```bash
# 使用对比脚本
./scripts/compare-performance.sh

# 或手动运行
# 1. 运行 Java 测试
./gradlew :terminal-emulator:test --tests com.termux.terminal.JavaRustPerformanceComparisonTest --info

# 2. 运行 Rust 测试
cd terminal-emulator/src/main/rust && cargo test --test performance --release -- --nocapture

# 3. 对比结果
```

## 🤖 CI/CD 自动化

GitHub Actions 工作流会自动：
1. 在 Java (Master) 分支运行基准测试
2. 在当前分支（Rust）运行测试
3. 解析日志提取指标
4. 生成对比报告到 GitHub Step Summary

### 报告示例

```markdown
### ⚡ Comprehensive Engine Comparison (Java vs Rust)

#### 📊 Throughput Comparison

| Metric | Java (Master) | Rust (Current) | Speedup | Status |
|--------|---------------|----------------|---------|--------|
| Raw Text | 50.00 MB/s | 150.00 MB/s | 3.00x | 🚀 +200% |
| ANSI Escape | 30.00 MB/s | 75.00 MB/s | 2.50x | 🚀 +150% |
| Cursor Movement | 500.00 K ops/s | 800.00 K ops/s | 1.60x | ✅ +60% |
```

## 📝 测试数据生成

两个测试使用**完全相同**的随机数生成算法和 seed (42)，确保公平对比：

```java
// Java
long seed = 42L;
seed = Long.multiplyUnsigned(seed, 6364136223846793005L) + 1L;
```

```rust
// Rust
let mut seed = 42u64;
seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
```

## 📈 性能优化建议

### Rust 优势场景
- **批量数据处理** - Rust 的零拷贝和内存布局控制
- **复杂 ANSI 解析** - Rust 的模式匹配和枚举
- **高频小批量调用** - Rust 的低开销

### Java 优势场景
- **JIT 优化后的热路径** - 长时间运行后可能接近 Rust
- **GC 友好的场景** - 短生命周期对象

## 🔍 结果解读

- **Speedup > 1.5x** 🚀 - Rust 显著优势
- **Speedup >= 1.0x** ✅ - Rust 更快或持平
- **Speedup < 1.0x** ⚠️ - Java 更快

## 📚 相关文件

- `.github/workflows/compare-engines.yml` - CI 工作流配置
- `scripts/compare-performance.sh` - 本地对比脚本
- `terminal-emulator/src/test/java/.../JavaRustPerformanceComparisonTest.java` - Java 测试
- `terminal-emulator/src/main/rust/tests/performance.rs` - Rust 测试
