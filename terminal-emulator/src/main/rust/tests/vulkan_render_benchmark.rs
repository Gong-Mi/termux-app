// Vulkan 渲染管线带宽/吞吐量基准测试
// 运行：cargo test --test vulkan_render_benchmark -- --nocapture

use skia_safe::{
    surfaces, Color, Paint, Font, FontMgr, FontStyle, TextBlobBuilder,
};
use std::time::{Duration, Instant};

// ============================================================
// 辅助：测量一个操作的耗时
// ============================================================
fn measure<F: FnOnce()>(f: F) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

// 统计信息结构体
struct Stats {
    samples: Vec<Duration>,
}

impl Stats {
    fn new() -> Self {
        Self { samples: Vec::with_capacity(100) }
    }

    fn add(&mut self, d: Duration) {
        self.samples.push(d);
    }

    fn analyze(&self) -> (Duration, Duration, Duration, Duration, f64) {
        if self.samples.is_empty() {
            return (Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO, 0.0);
        }
        let mut sorted = self.samples.clone();
        sorted.sort();

        let sum: Duration = sorted.iter().sum();
        let avg = sum / sorted.len() as u32;
        let min = sorted[0];
        let max = sorted[sorted.len() - 1];
        let p95 = sorted[(sorted.len() as f32 * 0.95) as usize];

        let avg_f = avg.as_secs_f64();
        let variance: f64 = self.samples.iter()
            .map(|d| {
                let diff = d.as_secs_f64() - avg_f;
                diff * diff
            })
            .sum::<f64>() / self.samples.len() as f64;
        let std_dev = variance.sqrt();

        (min, avg, p95, max, std_dev)
    }
}

fn format_duration(d: Duration) -> String {
    if d.as_secs() > 0 {
        format!("{:.2}s", d.as_secs_f64())
    } else if d.as_millis() > 0 {
        format!("{:.2}ms", d.as_secs_f64() * 1000.0)
    } else {
        format!("{:.2}μs", d.as_secs_f64() * 1_000_000.0)
    }
}

// 创建模拟终端状态的测试数据
struct TerminalMock {
    cols: usize,
    rows: usize,
    cells: Vec<(char, u64)>, // (字符, 样式)
}

impl TerminalMock {
    fn new(cols: usize, rows: usize, fill_mode: FillMode) -> Self {
        let cell_count = cols * rows;
        let cells = match fill_mode {
            FillMode::Empty => vec![(' ', 0); cell_count],
            FillMode::Ascii => (0..cell_count)
                .map(|i| ((b'A' + (i % 26) as u8) as char, 0))
                .collect(),
            FillMode::Chinese => {
                let chars: Vec<char> = "测试终端渲染性能".chars().collect();
                (0..cell_count)
                    .map(|i| (chars[i % chars.len()], 0))
                    .collect()
            }
            FillMode::Mixed => {
                let patterns: Vec<char> = "Hello 终端 文件 Test 测试".chars().collect();
                (0..cell_count)
                    .map(|i| (patterns[i % patterns.len()], (i % 3) as u64))
                    .collect()
            }
            FillMode::Stress => {
                let stress_chars: Vec<char> = "测试终端 HelloWorld 日本語 한국어".chars().collect();
                (0..cell_count)
                    .map(|i| (stress_chars[i % stress_chars.len()], (i % 9) as u64))
                    .collect()
            }
        };
        Self { cols, rows, cells }
    }
}

#[derive(Clone, Copy)]
enum FillMode {
    Empty,
    Ascii,
    Chinese,
    Mixed,
    Stress,
}

// ============================================================
// 模拟渲染管线 - 与 Vulkan 管线相同的操作模式
// ============================================================
struct DetailedDuration {
    clear: Duration,
    grouping: Duration,
    glyph_lookup: Duration,
    blob_alloc: Duration,
    blob_make: Duration,
    draw_call: Duration,
}

fn simulate_detailed_pipeline(canvas: &skia_safe::Canvas, terminal: &TerminalMock, font: &Font, paint: &Paint) -> DetailedDuration {
    let font_width = 10.0;
    let font_height = 20.0;
    
    let mut clear = Duration::ZERO;
    let mut grouping = Duration::ZERO;
    let mut glyph_lookup = Duration::ZERO;
    let mut blob_alloc = Duration::ZERO;
    let mut blob_make = Duration::ZERO;
    let mut draw_call = Duration::ZERO;

    // 1. 背景清屏
    clear = measure(|| {
        canvas.clear(Color::new(0xFF000000u32));
    });

    // 2. 逐行绘制
    for r in 0..terminal.rows {
        let y_base = (r as f32 + 1.0) * font_height;
        let y_adj = y_base + 3.0;
        let row_start = r * terminal.cols;

        let mut builder = TextBlobBuilder::new();
        let mut has_content = false;

        let mut c = 0;
        while c < terminal.cols {
            let start_group = Instant::now();
            let cell_idx = row_start + c;
            if cell_idx >= terminal.cells.len() { break; }
            let (ch, style) = terminal.cells[cell_idx];
            if ch == ' ' { c += 1; grouping += start_group.elapsed(); continue; }

            let mut run_chars = Vec::new();
            while c < terminal.cols {
                let ci = row_start + c;
                if ci >= terminal.cells.len() { break; }
                let (ch2, style2) = terminal.cells[ci];
                if style2 != style || ch2 == ' ' { break; }
                run_chars.push((ch2, c as f32 * font_width));
                c += 1;
            }
            grouping += start_group.elapsed();

            if !run_chars.is_empty() {
                // 细化 flush_run_to_blob
                let text: String = run_chars.iter().map(|(ch, _)| *ch).collect();
                let mut glyphs = vec![skia_safe::GlyphId::default(); run_chars.len()];
                
                glyph_lookup += measure(|| {
                    font.str_to_glyphs(&text, &mut glyphs);
                });

                let mut run_data: Option<(&mut [u16], &mut [f32])> = None;
                blob_alloc += measure(|| {
                    run_data = Some(builder.alloc_run_pos_h(font, run_chars.len(), 0.0, None));
                });

                if let Some((run_glyphs, run_pos)) = run_data {
                    run_glyphs.copy_from_slice(&glyphs);
                    for (i, (_, x)) in run_chars.iter().enumerate() {
                        run_pos[i] = *x;
                    }
                }
                has_content = true;
            }
        }

        if has_content {
            let mut blob: Option<skia_safe::TextBlob> = None;
            blob_make += measure(|| {
                blob = builder.make();
            });
            
            if let Some(b) = blob {
                draw_call += measure(|| {
                    canvas.draw_text_blob(&b, (0.0, y_adj), paint);
                });
            }
        }
    }

    DetailedDuration { clear, grouping, glyph_lookup, blob_alloc, blob_make, draw_call }
}

fn simulate_render_pipeline(canvas: &skia_safe::Canvas, terminal: &TerminalMock, font: &Font, paint: &Paint) -> Duration {
    let d = simulate_detailed_pipeline(canvas, terminal, font, paint);
    d.clear + d.grouping + d.glyph_lookup + d.blob_alloc + d.blob_make + d.draw_call
}

// ============================================================
// 测试 1: 不同终端大小的渲染吞吐量
// ============================================================
#[test]
fn benchmark_terminal_sizes() {
    println!("\n========== Vulkan 渲染管线吞吐量基准测试 ==========\n");

    let font_mgr = FontMgr::new();
    let tf = font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap();
    let mut font = Font::new(tf, Some(12.0));
    font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
    font.set_subpixel(true);
    let mut paint = Paint::default();
    paint.set_anti_alias(false);

    let configs = [
        (80, 24,   "标准终端 80x24"),
        (120, 40,  "中等终端 120x40"),
        (200, 60,  "大终端 200x60"),
        (300, 80,  "超大终端 300x80"),
        (400, 100, "压力终端 400x100"),
    ];

    let fill_modes = [
        (FillMode::Empty, "空白"),
        (FillMode::Ascii, "ASCII"),
        (FillMode::Chinese, "中文"),
        (FillMode::Mixed, "混合"),
        (FillMode::Stress, "压力"),
    ];

    println!("{:<20} | {:<8} | {:<10} | {:<22} | {:<8} | {:<15}",
             "终端配置", "内容", "单元格数", "耗时(min/avg/p95)", "FPS", "吞吐量 (像素)");
    println!("{:-<105}", "");

    for (cols, rows, label) in &configs {
        for (fill_mode, fill_label) in &fill_modes {
            let terminal = TerminalMock::new(*cols, *rows, *fill_mode);
            let cell_count = cols * rows;

            // Surface 大小模拟屏幕 1200x2400
            let mut surface = surfaces::raster_n32_premul((1200, 2400))
                .expect("Failed to create surface");
            let canvas = surface.canvas();

            // 预热 3 帧
            for _ in 0..3 {
                simulate_render_pipeline(canvas, &terminal, &font, &paint);
            }

            // 测量 20 帧以获得更好的统计数据
            let iterations = 20;
            let mut stats = Stats::new();
            for _ in 0..iterations {
                stats.add(simulate_render_pipeline(canvas, &terminal, &font, &paint));
            }
            let (min_d, avg_d, p95_d, _max_d, _std_dev) = stats.analyze();
            let fps = 1.0 / avg_d.as_secs_f64();
            let throughput = (cell_count as f64 / avg_d.as_secs_f64()) / 1_000_000.0;
            // 假设每个单元格 10x20 像素
            let pixel_throughput = (cell_count as f64 * 200.0 / avg_d.as_secs_f64()) / 1_000_000.0;

            let time_combined = format!("{:.1}/{:.1}/{:.1}ms", 
                min_d.as_secs_f64() * 1000.0, 
                avg_d.as_secs_f64() * 1000.0, 
                p95_d.as_secs_f64() * 1000.0);

            println!("{:<20} | {:<8} | {:>7} 格 | {:>22} | {:>8.1} | {:>7.2}Mc/s ({:>5.0}MP/s)",
                     label, fill_label, cell_count, time_combined, fps, throughput, pixel_throughput);
        }
        println!("{:-<105}", "");
    }
}

// ============================================================
// 测试 2: 单项操作耗时分解
// ============================================================
#[test]
fn benchmark_pipeline_stages() {
    println!("\n========== 渲染管线阶段耗时分解 ==========\n");

    let cols = 200;
    let rows = 60;
    let terminal = TerminalMock::new(cols, rows, FillMode::Mixed);

    let mut surface = surfaces::raster_n32_premul((1200, 2400))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    let font_mgr = FontMgr::new();
    let tf = font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap();
    let mut font = Font::new(tf, Some(12.0));
    font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
    font.set_subpixel(true);
    let mut paint = Paint::default();
    paint.set_anti_alias(false);

    // 1. 获取详细耗时
    let mut total_d = DetailedDuration {
        clear: Duration::ZERO, grouping: Duration::ZERO, glyph_lookup: Duration::ZERO,
        blob_alloc: Duration::ZERO, blob_make: Duration::ZERO, draw_call: Duration::ZERO,
    };
    
    let iterations = 20;
    for _ in 0..iterations {
        let d = simulate_detailed_pipeline(canvas, &terminal, &font, &paint);
        total_d.clear += d.clear;
        total_d.grouping += d.grouping;
        total_d.glyph_lookup += d.glyph_lookup;
        total_d.blob_alloc += d.blob_alloc;
        total_d.blob_make += d.blob_make;
        total_d.draw_call += d.draw_call;
    }

    let avg_d = |d: Duration| d.as_micros() as f64 / iterations as f64;
    let clear_us = avg_d(total_d.clear);
    let group_us = avg_d(total_d.grouping);
    let glyph_us = avg_d(total_d.glyph_lookup);
    let alloc_us = avg_d(total_d.blob_alloc);
    let make_us = avg_d(total_d.blob_make);
    let draw_us = avg_d(total_d.draw_call);
    let total_us = clear_us + group_us + glyph_us + alloc_us + make_us + draw_us;

    println!("终端配置: {}x{} ({} 单元格)", cols, rows, cols * rows);
    println!("填充模式: 混合 (中文+ASCII)");
    println!();
    println!("  渲染步骤           | 平均耗时    | 占比   | 可视化分布");
    println!("  {:-<65}", "");

    let stages = [
        ("清屏 (Clear)", clear_us),
        ("字符分组 (Grouping)", group_us),
        ("字形查找 (Glyph ID)", glyph_us),
        ("Blob 分配 (Alloc)", alloc_us),
        ("Blob 生成 (Make)", make_us),
        ("Vulkan 提交 (Draw)", draw_us),
    ];

    for (name, us) in stages {
        let pct = us / total_us * 100.0;
        let bar = "█".repeat((pct / 4.0) as usize);
        println!("  {:<18} | {:>8.1}μs | {:>5.1}% | {}", name, us, pct, bar);
    }

    println!("  {:-<65}", "");
    println!("  整帧总计           | {:>8.1}μs | 100.0% | {}", total_us, "█".repeat(25));
    println!();
    println!("  性能诊断指标:");
    println!("    理论最大帧率: {:.1} FPS", 1_000_000.0 / total_us);
    println!("    字体处理开销: {:.1}% (Glyph+Alloc+Make)", (glyph_us + alloc_us + make_us) / total_us * 100.0);
    println!("    CPU 预处理占比: {:.1}% (Grouping)", group_us / total_us * 100.0);
}

// ============================================================
// 测试 3: 连续渲染稳定性 (模拟长时间运行)
// ============================================================
#[test]
fn benchmark_sustained_rendering() {
    println!("\n========== 连续渲染稳定性测试 (1000 帧) ==========\n");

    let cols = 120;
    let rows = 40;
    let terminal = TerminalMock::new(cols, rows, FillMode::Mixed);

    let mut surface = surfaces::raster_n32_premul((1200, 2400))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    let font_mgr = FontMgr::new();
    let tf = font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap();
    let mut font = Font::new(tf, Some(12.0));
    font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
    font.set_subpixel(true);
    let mut paint = Paint::default();
    paint.set_anti_alias(false);

    let frames = 1000;
    let mut stats = Stats::new();
    let start = Instant::now();

    for i in 0..frames {
        let frame_time = measure(|| {
            simulate_render_pipeline(canvas, &terminal, &font, &paint);
        });
        stats.add(frame_time);

        if (i + 1) % 200 == 0 {
            let elapsed = start.elapsed();
            let current_fps = (i + 1) as f64 / elapsed.as_secs_f64();
            println!("  进度: {:>4}/{} | 累计耗时: {:.2}s | 实时 FPS: {:.1}", i + 1, frames, elapsed.as_secs_f64(), current_fps);
        }
    }

    let total_elapsed = start.elapsed();
    let (min_v, avg_v, p95_v, max_v, std_dev) = stats.analyze();
    let avg_fps = frames as f64 / total_elapsed.as_secs_f64();

    println!();
    println!("  统计概览:");
    println!("    总帧数:       {}", frames);
    println!("    总耗时:       {:.3}s", total_elapsed.as_secs_f64());
    println!("    平均 FPS:     {:.1}", avg_fps);
    println!("    抖动 (StdDev): {:.3}ms", std_dev * 1000.0);
    println!();
    println!("  延迟分布:");
    println!("    最小值 (Min): {:>8}", format_duration(min_v));
    println!("    平均值 (Avg): {:>8}", format_duration(avg_v));
    println!("    95分位 (P95): {:>8}", format_duration(p95_v));
    println!("    最大值 (Max): {:>8}", format_duration(max_v));
    println!();
    println!("  吞吐量分析:");
    println!("    渲染单元格总数: {} ({:.1} M cells)", frames * cols * rows, (frames * cols * rows) as f64 / 1_000_000.0);
    println!("    渲染吞吐量:     {:.2} M cells/s", (frames * cols * rows) as f64 / total_elapsed.as_secs_f64() / 1_000_000.0);
}

// ============================================================
// 测试 4: 极限压力测试
// ============================================================
#[test]
fn benchmark_extreme_stress() {
    println!("\n========== 极限压力测试 ==========\n");

    // 500 列 x 100 行 = 50,000 单元格/帧
    let cols = 500;
    let rows = 100;
    let terminal = TerminalMock::new(cols, rows, FillMode::Stress);

    let mut surface = surfaces::raster_n32_premul((4000, 4000))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    let font_mgr = FontMgr::new();
    let tf = font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap();
    let mut font = Font::new(tf, Some(10.0));
    font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
    font.set_subpixel(true);
    let mut paint = Paint::default();
    paint.set_anti_alias(false);

    let frames = 10;
    println!("  配置: {}x{} = {} 单元格/帧", cols, rows, cols * rows);
    println!("  内容: 极限压力 (多语言+混合样式)");
    println!("  帧数: {}", frames);
    println!();

    let start = Instant::now();
    for i in 0..frames {
        let frame_time = measure(|| {
            simulate_render_pipeline(canvas, &terminal, &font, &paint);
        });
        println!("  帧 {:>2}: {:>8.2}ms", i + 1, frame_time.as_secs_f64() * 1000.0);
    }

    let total = start.elapsed();
    let avg_fps = frames as f64 / total.as_secs_f64();

    println!();
    println!("  平均 FPS:     {:.2}", avg_fps);
    println!("  平均帧时间:   {:.2}ms", (total / frames as u32).as_secs_f64() * 1000.0);
    println!("  渲染吞吐量:   {:.2} M cells/s",
             (frames * cols * rows) as f64 / total.as_secs_f64() / 1_000_000.0);
}

// ============================================================
// 测试 5: 缓存效率与样式密度分析
// ============================================================
#[test]
fn benchmark_cache_and_styles() {
    println!("\n========== 缓存效率与样式密度分析 ==========\n");

    let mut surface = surfaces::raster_n32_premul((1200, 2400)).expect("Surface failed");
    let canvas = surface.canvas();
    let font_mgr = FontMgr::new();
    let tf = font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap();
    let font = Font::new(tf, Some(12.0));
    let mut paint = Paint::default();
    paint.set_anti_alias(false);

    // 1. 模拟冷启动（无缓存）
    let terminal_simple = TerminalMock::new(100, 30, FillMode::Ascii);
    let cold_time = measure(|| {
        simulate_detailed_pipeline(canvas, &terminal_simple, &font, &paint);
    });

    // 2. 模拟热启动（命中缓存）
    // 在真实场景中，缓存会跳过 Glyph Lookup, Alloc, Make 步骤
    let hot_time = measure(|| {
        canvas.clear(Color::new(0xFF000000u32));
        for r in 0..30 {
            // 模拟从缓存直接绘制 TextBlob
            let mut builder = TextBlobBuilder::new();
            let (run_glyphs, _) = builder.alloc_run_pos_h(&font, 100, 0.0, None);
            run_glyphs.fill(0); // 假数据
            if let Some(blob) = builder.make() {
                canvas.draw_text_blob(&blob, (0.0, (r + 1) as f32 * 20.0), &paint);
            }
        }
    });

    println!("  缓存效能:");
    println!("    冷启动 (Cold):   {:>8}", format_duration(cold_time));
    println!("    热启动 (Cached): {:>8}", format_duration(hot_time));
    println!("    提升倍率:        {:.1}x", cold_time.as_secs_f64() / hot_time.as_secs_f64());
    println!();

    // 3. 样式密度压力测试 (Style Density)
    println!("  样式密度影响 (100x30 终端):");
    println!("    样式配置         | 绘制 Run 数量 | 平均耗时    | FPS");
    println!("    {:-<60}", "");

    let densities = [
        (1, "单一样式 (Uniform)"),
        (5, "低密度 (5 styles/row)"),
        (20, "高密度 (20 styles/row)"),
        (100, "极限密度 (Each char different)"),
    ];

    for (runs_per_row, label) in densities {
        let mut cells = Vec::new();
        for _r in 0..30 {
            for c in 0..100 {
                let style = (c / (100 / runs_per_row)) as u64;
                cells.push(('A', style));
            }
        }
        let terminal = TerminalMock { cols: 100, rows: 30, cells };
        
        let mut total = Duration::ZERO;
        for _ in 0..20 {
            total += simulate_render_pipeline(canvas, &terminal, &font, &paint);
        }
        let avg = total / 20;
        
        println!("    {:<16} | {:>12} | {:>10} | {:>6.1}",
                 label, runs_per_row * 30, format_duration(avg), 1.0 / avg.as_secs_f64());
    }
}
