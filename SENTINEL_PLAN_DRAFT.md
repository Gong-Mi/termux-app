# Termux-Rust Sentinel: 特权态势感知与进程保护计划

## 1. 核心目标
通过 Native Rust 编写的特权守护进程（Sentinel Daemon），利用 Shizuku (shell UID) 权限，为 Termux 提供实时的系统级监控与进程保护。解决 Android 12+ 的 Phantom Process (信号 9) 杀进程问题，并实现任务的动态分片与降级。

## 2. 系统架构
- **Sentinel Daemon (Privileged)**: 运行在 `/data/local/tmp`，由 `rish` 引导，具备 `shell` 权限。
- **Sentinel Client (App side)**: 集成在 `termux-app-rust` 核心线程中。
- **Communication Channel**: 本地 TCP 环回流 (127.0.0.1:54321)，绕过 SELinux 对 Unix Domain Socket 的文件权限限制。
- **Data Source**: 直接读取 `/proc` 伪文件系统（高频、低耗）及 `dumpsys`（低频、深度）。

## 3. 功能模块
- **感知层**: 统计当前幻影进程总数、监控 CPU 占用率、读取 `device_config` 限制。
- **决策层**: 计算风险等级（SAFE/WARNING/CRITICAL）。
- **执行层**: 
    - **Slicing**: 拦截并排队超额的 PTY 创建请求。
    - **Throttling**: 对危险进程实施 `SIGSTOP`/`SIGCONT` 调度。
    - **Persistence**: 自动修复 `max_phantom_processes` 系统属性。

## 4. 实施阶段
1. **引导层**: 优化 `rish` 脚本，实现 Rust 二进制的零依赖启动。
2. **感知层**: 实现纯 Rust 的 `/proc` 扫描器。
3. **通信层**: 构建稳定的 TCP 高频推流协议。
4. **集成层**: 在 `coordinator.rs` 中植入基于感知数据的控制逻辑。

---

## 5. 当前架构潜在风险分析 (Architecture Review)

在目前的框架设计中，仍存在以下几个关键挑战：

### A. 端口冲突与安全性 (Port Collision & Security)
- **风险**: TCP 端口 `54321` 是全局可见的。设备上的恶意应用可以连接该端口窃取系统态势，甚至发送伪造数据欺骗主程序。
- **对策**: 
    - 引入 **简单的握手校验**：Client 连接后必须发送一个随机生成的 Token，Daemon 验证通过后才开始推流。
    - 使用 **动态端口**：Daemon 启动时寻找可用端口，并通过临时文件通知 Client。

### B. Android 14+ 的执行限制 (W^X Restrictions)
- **风险**: Android 14 对 `/data/local/tmp` 的执行权限控制越来越严（如 `noexec` 挂载）。
- **对策**: 
    - 将二进制文件写入应用的私有目录 (`/data/data/com.termux/files/...`) 并在该目录下赋予权限，由 `rish` 在该路径下启动。

### C. 资源争用死锁 (Resource Deadlock)
- **风险**: 如果感知线程为了获取 `dumpsys` 结果而被阻塞，而此时主线程正在等待感知结果来决定是否派生进程，可能导致死锁。
- **对策**: 
    - **非阻塞设计**：主线程永远只读取原子变量中的“最后一帧”数据，绝不主动触发同步查询。

### D. 进程树断裂 (Process Orphanage)
- **风险**: 如果 `sentinel` 守护进程因为 OOM 被系统杀掉，主程序可能因为失去雷达而盲目 fork，导致被系统批量清理。
- **对策**: 
    - **失效安全 (Fail-safe)**：一旦 Client 失去 TCP 连接，主程序立即进入“最保守模式”，大幅限制进程派生频率，直到重新建立连接。

### E. 耗电量与 CPU 唤醒次数 (Battery Impact)
- **风险**: 50ms 的汇报频率在后台可能会阻止 CPU 进入深度睡眠。
- **对策**: 
    - **动态频率调节**：当 Termux 处于后台且无活动任务时，将汇报频率降至 1s 或进入休眠模式；当有活跃 PTY 时，切换回 50ms 高频模式。
