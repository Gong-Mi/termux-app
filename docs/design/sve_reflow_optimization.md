# 技术设计文档：Termux SVE 加速与增量重排 (Incremental Reflow) 优化

## 1. 背景与目标
目前 Termux 的终端重排（Reflow）逻辑为同步全量重排。当缓冲区行数较大（如 10,000+ 行）且 CPU 资源受限（如有后台高负载任务）时，旋转屏幕或滑动触发的重排会导致主线程（UI 线程）卡顿。

**优化目标：**
- **主线程解耦**：将 Reflow 计算量从 $O(\text{TotalRows})$ 降低到 $O(\text{ViewportRows})$。
- **性能飞跃**：利用 AArch64 SVE (Scalable Vector Extension) 指令集加速字符宽度计算。
- **内存优化**：引入逻辑视图缓存，减少因全量重排导致的频繁内存分配。

---

## 2. 核心设计架构

### 2.1 虚拟缓冲区模型 (Virtual Buffer Model)
将 `TerminalBuffer` 拆分为两层：
1.  **物理行存储 (Physical Store)**：存储原始输入的逻辑行。
2.  **重排缓存 (Reflow Cache)**：一个 LRU (Least Recently Used) 缓存，仅存储当前可视区域 $\pm 50$ 行重排后的渲染数据。

### 2.2 SVE 加速逻辑
在 AArch64 SVE 支持的设备上，利用变长向量并行处理：
- **向量载入**：批量载入 UTF-32 编码的字符。
- **并行宽度判定**：
    - `0x00..0x7F` -> 1 (ASCII)
    - `0x4E00..0x9FFF` -> 2 (CJK)
- **谓词掩码 (Predicates)**：利用 SVE 谓词处理非 128 位对齐的剩余字符，消除传统 NEON 的补齐开销。

---

## 3. 工程验证步骤 (Engineering Validation)

为确保该优化在工程上是稳定且有效的，必须遵循以下验证流程：

### 阶段一：硬件与编译环境验证
- **步骤 1：SVE 特性探测**
    - 执行 `adb shell cat /proc/cpuinfo` 确认 `Features` 包含 `sve`。
    - 编写简单的 Rust 探测程序，使用 `std::arch::is_aarch64_feature_detected!("sve")` 进行运行时确认。
- **步骤 2：工具链支持确认**
    - 验证 NDK 编译器版本，确保支持 `-C target-feature=+sve`。
    - 运行 `cargo build` 测试是否能正确生成含有 SVE 指令的 `.so`。

### 阶段二：基准性能验证 (Micro-benchmarking)
- **步骤 3：核心算法对比**
    - 在 Rust 层编写 Benchmark 测试（使用 `criterion`）。
    - 比较 **标量逻辑** vs **SVE 向量化逻辑** 在计算 100,000 个随机字符宽度时的纯耗时（Time per char）。
- **步骤 4：缓存命中率测试**
    - 模拟连续快速滚动，记录 `Reflow Cache` 的命中率和缓存失效导致的重排延迟。

### 阶段三：集成与压力验证 (System Testing)
- **步骤 5：高负载下的 UI 响应度**
    - 模拟用户当前场景：后台运行 `npm install` (PID 5622 类似负载)。
    - 使用 `adb shell dumpsys gfxinfo` 记录滑动过程中的 `Janky frames` 和 `High input latency`。
    - **通过标准**：优化后在 CPU 负载 90%+ 的情况下，滑动掉帧率应低于 5%。
- **步骤 6：正确性回归 (Fuzzing)**
    - 使用含有大量 Emoji、组合字符、CJK 混合的复杂文本流进行填充。
    - 验证增量重排后的字符位置与全量重排结果是否完全一致（字节级比对）。

---

## 4. 风险评估
- **硬件兼容性**：SVE 仅在较新的 ARM 内核（如 Cortex-X2/A710+）支持，老旧设备需自动回退到 NEON 或标量。
- **索引复杂性**：增量重排在快速跳转（如 `G` 到底部）时，可能需要稀疏索引来快速定位物理行偏移。
