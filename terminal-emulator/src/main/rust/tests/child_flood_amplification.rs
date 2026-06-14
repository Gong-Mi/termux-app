// child_flood_amplification.rs
// 测量 resize → 子进程反馈回路的写放大系数
//
// 问题抽象:
//   resize(N, C)
//     → buffer 遍历 O(N×C) 重置列宽
//     → SIGWINCH → 子进程重绘全部历史
//       → flood 输出回到 PTY
//         → buffer.process_bytes(flood) 即 O(flood_size)
//
// 关键指标: 写放大系数 = flood_size / (一次正常屏幕内容量)
//   如果放大系数 > 1，反馈回路不收敛 → 必 ANR
//
// 内容模型: UI 和 Content 分层
//   UI: 固定宽度前缀 (`> `)、分隔线 (`───`)、状态栏
//   Content: 跨行长段落，宽度变化时自动换行
//
// 运行: cargo test --test child_flood_amplification --release -- --nocapture

use std::time::Instant;
use termux_rust::TerminalEngine;

// =========================================================================
// 配置（可以根据需要调整）
// =========================================================================

/// 模拟 N 轮 chat turn 后的 scrollback 量
const CHAT_TURNS: usize = 50;

/// 模拟 resize 目标宽度（pinch zoom 后的列数变化）
const TARGET_COLS: i64 = 60;

/// 子进程收到 SIGWINCH 后重绘的比例
const CHILD_REDRAW_RATIO: f64 = 1.0;

// =========================================================================
// 真实内容样本 + 子进程独立模型
// =========================================================================

/// 跨行长消息样本——模拟 AI 回复的段落
/// 这些是子进程"自己存的内容"，每次 redraw 时它按当前宽度重新排版
const SAMPLE_MESSAGES: &[&str] = &[
    "好的，我来解释一下这个问题。它涉及到终端 resize 时的缓冲区重排机制。当用户通过捏合手势改变字号时，终端模拟器需要重新计算每一行的换行位置。这是一个 O(N×C) 的操作，其中 N 是 scrollback 行数，C 是列数。在 Java 实现中，这个遍历会触发 ART 的 JIT OSR 机制，导致主线程卡顿超过 5 秒从而触发 ANR。",
    "Rust 版本的实现把 buffer 操作移到了本地代码。resize 的时候不再逐行调用 Java 的 setChar，而是直接在 Rust 层做内存级别的行重排。实测 10,000 行 scrollback 的 resize 只需要 12 毫秒。但是子进程收到 SIGWINCH 后的 flood 输出依然是个问题——不管 buffer 多快，子进程重绘全历史的数据量不会变小。",
    "这个问题的本质是正反馈回路。resize 触发了子进程的重绘，子进程的重绘产生了更多 buffer 操作，而这些 buffer 操作又可能触发新的 resize（比如内容行数变化）。在极端情况下，这个回路是发散的——每次迭代产生的数据量都比上一次大。写放大系数就是衡量这个发散程度的指标。",
    "我建议采取以下几个优化方向：第一，在 pinch zoom 期间抑制 SIGWINCH 的发送，只在手势结束时一次性提交。第二，对于子进程的输出，在 PTY 读取端做限速，避免一次性涌入过多数据。第三，buffer 的 resize 操作做增量处理——只重排可见区域的行，不可见的行打标记延迟处理。",
    "但是以上三种方案都有各自的代价。抑制 SIGWINCH 会导致子进程在 zoom 期间显示的尺寸不对，用户看到的内容是扭曲的。PTY 限速会导致子进程的输出被截断或延迟，对于交互式程序来说体验很差。增量处理则引入了数据一致性问题——不可见行和可见行的列宽可能不同步。",
    "所以真正的解法可能不在这些具体方案里。我们需要重新审视终端 resize 的语义：为什么每次 resize 都要全量重排整个 scrollback？如果 scrollback 只是一个线性日志，它其实不需要 reflow——只需要按新宽度重新渲染可见区域即可。scrollback 里的行只是历史记录，用户翻回去看的时候再按当前宽度渲染就够了。",
];

/// UI 前缀
const UI_PROMPT: &str = "> ";

/// UI 分隔线
const UI_SEPARATOR: &str = "────────────────────────────────────";

/// 子进程模型中的一条消息
struct ChildMessage {
    /// 发送者: "User" 或 "Assistant"
    sender: String,
    /// 原始文本（不包含换行，子进程自己在显示时排版）
    text: String,
}

/// 子进程的独立数据模型
/// 它不知道自己被连接到终端，它只知道自己有 N 条消息
struct ChildModel {
    messages: Vec<ChildMessage>,
    cols: i64,
    rows: i64,
}

impl ChildModel {
    fn new(cols: i64, rows: i64) -> Self {
        Self {
            messages: Vec::new(),
            cols,
            rows,
        }
    }

    fn add_turn(&mut self, turn: usize, msg_text: &str) {
        self.messages.push(ChildMessage {
            sender: format!("User"),
            text: format!("第 {} 轮的问题在这里", turn),
        });
        self.messages.push(ChildMessage {
            sender: format!("Assistant"),
            text: msg_text.to_string(),
        });
    }

    /// 子进程收到 SIGWINCH 后，用自己的模型按新宽度重新排版全文
    /// 输出 ANSI 序列（类似 vim 的 redraw 命令）
    fn redraw_all(&self, new_cols: i64) -> Vec<u8> {
        let mut output = Vec::with_capacity(1024 * 1024);

        // 子进程的完整重绘：清屏 → 重新绘制每行
        output.extend_from_slice(b"\x1b[2J\x1b[H");

        for msg in &self.messages {
            // UI: 发送者前缀（固定宽度）
            let prefix = if msg.sender == "User" {
                format!("{}你: ", UI_PROMPT)
            } else {
                format!("{}", UI_PROMPT)
            };
            let prefix_width = prefix.chars().count() as i64;

            // Content: 按当前新宽度重新排版文本
            let content_width = (new_cols - prefix_width).max(10) as usize;
            let mut pos = 0;
            let text_chars: Vec<char> = msg.text.chars().collect();

            while pos < text_chars.len() {
                // 找在当前宽度内能放下的前缀
                if pos == 0 {
                    output.extend_from_slice(prefix.as_bytes());
                } else {
                    // 续行：空格对齐
                    for _ in 0..prefix_width {
                        output.push(b' ');
                    }
                }

                // 取 content_width 个字符（宽字符处理简化版）
                let end = (pos + content_width).min(text_chars.len());
                let segment: String = text_chars[pos..end].iter().collect();
                output.extend_from_slice(segment.as_bytes());
                output.extend_from_slice(b"\r\n");

                pos = end;
            }

            // UI: 消息后的分隔线
            output.extend_from_slice(UI_SEPARATOR.as_bytes());
            output.extend_from_slice(b"\r\n");
        }

        output.truncate(1024 * 1024);
        output
    }
}

// =========================================================================
// 填充缓冲区
// =========================================================================

/// 填充终端 buffer + 子进程独立模型
/// 每次 turn：先写 UI 前缀，再写跨行 content（终端自动换行）
/// 同时往子进程模型里存原始文本
fn fill_chat_history(engine: &mut TerminalEngine, child: &mut ChildModel, turns: usize) {
    for t in 0..turns {
        let msg = SAMPLE_MESSAGES[t % SAMPLE_MESSAGES.len()];

        // 写入终端（模拟 AI 输出到终端的过程）
        let user_line = format!("\r\n{}你: 第 {} 轮的问题在这里\r\n", UI_PROMPT, t);
        engine.process_bytes(user_line.as_bytes());

        // Content 写入——终端会按当前列宽自动换行
        let content_line = format!("\r\n{}", msg);
        engine.process_bytes(content_line.as_bytes());

        // 分隔线
        engine.process_bytes(b"\r\n");
        engine.process_bytes(UI_SEPARATOR.as_bytes());
        engine.process_bytes(b"\r\n");

        // 子进程模型同时记录原始文本
        child.add_turn(t, msg);
    }
}

/// 子进程收到 SIGWINCH 后的重绘
/// 它不知道自己连接了什么终端，只知道自己有 N 条消息
/// 它用自己的模型重新排版全文，产生 ANSI 输出
/// 终端收到这些 ANSI 序列后，再解析写入 buffer
fn apply_child_redraw(
    engine: &mut TerminalEngine,
    child: &ChildModel,
) -> Vec<u8> {
    // 子进程按 PTY 当前列宽重新排版
    let new_cols = engine.state.cols;
    child.redraw_all(new_cols)
}

/// 模拟 Vulkan/Skia 渲染开销
fn simulate_render_overhead(engine: &TerminalEngine, _full_redraw: bool) -> std::time::Duration {
    let rows = engine.state.rows as usize;
    let cols = engine.state.cols as usize;

    // 模拟:
    //   1. flat_buffer → glyph cache lookup（每字符 ~50ns）
    //   2. Skia canvas 操作（每行 ~2μs）
    //   3. Vulkan command buffer build（固定 ~200μs）
    //   4. GPU submit + present（fixed ~500μs）
    let glyph_lookup_ns = (rows * cols) as f64 * 50.0;
    let canvas_ops_ns = rows as f64 * 2000.0;
    let vk_overhead_ns = 200.0;
    let gpu_submit_ns = 500.0;

    let total_ns = glyph_lookup_ns + canvas_ops_ns + vk_overhead_ns + gpu_submit_ns;
    std::time::Duration::from_nanos(total_ns as u64)
}

// =========================================================================
// 测试 1: 基线——纯 buffer resize 耗时
// =========================================================================

#[test]
fn test_baseline_resize_cost() {
    println!("\n=== 测试 1: 纯 buffer resize 基线 ===");
    println!("Chat turns: {}, 每条消息含 UI + 跨行 Content", CHAT_TURNS);
    println!("Scrollback ≈ {} 条消息 × 自动换行", CHAT_TURNS);

    let total_rows = 20000; // 留够换行空间
    let mut engine = TerminalEngine::new(80, 24, total_rows, 10, 20);
    let mut child = ChildModel::new(80, 24);
    fill_chat_history(&mut engine, &mut child, CHAT_TURNS);

    let actual_rows = engine.state.main_screen.active_transcript_rows;
    println!("Actual scrollback rows: {}", actual_rows);

    let start = Instant::now();
    engine.state.resize(TARGET_COLS, 24);
    let dur = start.elapsed();

    println!("Resize time: {:?} ({:.2} ms)", dur, dur.as_secs_f64() * 1000.0);

    assert!(
        dur.as_millis() < 1000,
        "Baseline resize should complete within 1s for {} rows",
        actual_rows
    );
}

// =========================================================================
// 测试 2: 反馈回路——resize + child flood 累计耗时
// =========================================================================

#[test]
fn test_amplification_loop_cost() {
    println!("\n=== 测试 2: 反馈回路写放大 ===");
    println!("Chat turns: {}, Child redraw ratio: {}", CHAT_TURNS, CHILD_REDRAW_RATIO);

    let total_rows = 20000;
    let mut engine = TerminalEngine::new(80, 24, total_rows, 10, 20);
    let mut child = ChildModel::new(80, 24);
    fill_chat_history(&mut engine, &mut child, CHAT_TURNS);

    let actual_rows = engine.state.main_screen.active_transcript_rows;
    println!("Actual scrollback rows: {}", actual_rows);

    // 模拟一次 pinch zoom (10 次 MOVE → 10 次 resize)
    let move_count = 10;
    let mut total_resize_time = std::time::Duration::ZERO;
    let mut total_flood_time = std::time::Duration::ZERO;
    let mut total_render_time = std::time::Duration::ZERO;
    let mut total_input_bytes: usize = 0;

    for i in 0..move_count {
        let cols = if i % 2 == 0 { TARGET_COLS } else { 80 };

        // 1. resize（buffer 层）
        let start = Instant::now();
        engine.state.resize(cols, 24);
        total_resize_time += start.elapsed();

        // 2. 渲染第一帧（resize 后画面）
        total_render_time += simulate_render_overhead(&engine, true);

        // 3. child flood — 子进程用自己的模型按新宽度重新排版
        //    它不知道终端 buffer 刚刚被 reflow 过
        let flood = apply_child_redraw(&mut engine, &child);
        let start = Instant::now();
        engine.process_bytes(&flood);
        total_flood_time += start.elapsed();
        total_input_bytes += flood.len();

        // 4. 渲染第二帧（flood 后画面）
        total_render_time += simulate_render_overhead(&engine, true);
    }

    let total_time = total_resize_time + total_flood_time + total_render_time;

    println!("\n--- 耗时 ---");
    println!("Resize total:   {:>8.2}ms", total_resize_time.as_secs_f64() * 1000.0);
    println!("Flood total:    {:>8.2}ms", total_flood_time.as_secs_f64() * 1000.0);
    println!("Render total:   {:>8.2}ms", total_render_time.as_secs_f64() * 1000.0);
    println!("Combined:       {:>8.2}ms", total_time.as_secs_f64() * 1000.0);
    println!("ANR threshold:   5000.00ms");

    if total_time.as_millis() > 5000 {
        println!("⚠️  ANR RISK");
    } else {
        println!("✅  Below ANR threshold");
    }

    println!("\n--- 放大 ---");
    let screen_bytes = (engine.state.rows * engine.state.cols) as usize;
    let amplification = total_input_bytes as f64 / (screen_bytes as f64 * move_count as f64);
    println!("Total input:    {} bytes ({} bytes/MOVE)", total_input_bytes, total_input_bytes / move_count);
    println!("Screen:         {} bytes/屏", screen_bytes);
    println!("Write amplif.:  {:.2}x", amplification);
}

// =========================================================================
// 测试 3: 不同放大系数下的 ANR 边界
// =========================================================================

#[test]
fn test_anr_boundary_search() {
    println!("\n=== 测试 3: ANR 边界搜索 ===");
    println!("子进程自己排版，反馈回路独立于终端 buffer\n");

    // 固定配置: 10 MOVE 事件
    let move_count = 10;

    // 变量: turns（历史量）
    let scenarios = [50, 100, 200, 500, 1000];

    for &turns in &scenarios {
        let total_rows = 50000;
        let mut engine = TerminalEngine::new(80, 24, total_rows, 10, 20);
        let mut child = ChildModel::new(80, 24);
        fill_chat_history(&mut engine, &mut child, turns);

        let actual_rows = engine.state.main_screen.active_transcript_rows;

        let mut total_resize = std::time::Duration::ZERO;
        let mut total_flood = std::time::Duration::ZERO;
        let mut total_input = 0usize;

        for i in 0..move_count {
            let cols = if i % 2 == 0 { TARGET_COLS } else { 80 };
            let start = Instant::now();
            engine.state.resize(cols, 24);
            total_resize += start.elapsed();

            // 子进程在自己的模型上重新排版，不知道终端 buffer
            let flood = apply_child_redraw(&mut engine, &child);
            let start = Instant::now();
            engine.process_bytes(&flood);
            total_flood += start.elapsed();
            total_input += flood.len();
        }

        let total_render = simulate_render_overhead(&engine, true) * (move_count as u32 * 2);
        let total = total_resize + total_flood + total_render;
        let screen_bytes = (engine.state.rows * engine.state.cols) as usize;
        let amplif = total_input as f64 / (screen_bytes as f64 * move_count as f64);

        let risk = if total.as_millis() > 5000 { "⚠️ ANR" } else { "OK" };
        println!("| {:>4} turns | {:>5} rows | {:>7.2}ms | {:>5.1}x amplif | {}",
                 turns, actual_rows, total.as_secs_f64() * 1000.0, amplif, risk);
    }
}

// =========================================================================
// 测试 4: resize 耗时 vs scrollback 行数增长曲线
// =========================================================================

#[test]
fn test_resize_scaling_curve() {
    println!("\n=== 测试 4: resize 耗时 vs scrollback 行数 ===");

    let sizes = [100, 500, 1000, 2000, 5000, 10000i64];

    for &n in &sizes {
        let total_rows = n + 10000;
        let mut engine = TerminalEngine::new(80, 24, total_rows, 10, 20);

        for i in 0..n {
            let idx = (i as usize) % SAMPLE_MESSAGES.len();
            let line = format!("{}\r\n", SAMPLE_MESSAGES[idx]);
            engine.process_bytes(line.as_bytes());
        }

        let start = Instant::now();
        engine.state.resize(60, 24);
        let dur = start.elapsed();

        let actual = engine.state.main_screen.active_transcript_rows;
        println!("| {:>5} lines | {:>5} actual | {:>8.2?} | {:>6.2} ms |",
                 n, actual, dur, dur.as_secs_f64() * 1000.0);
    }

    println!("\n→ resize O(N) 增长，线性意味着 buffer 不会突然爆炸");
}
