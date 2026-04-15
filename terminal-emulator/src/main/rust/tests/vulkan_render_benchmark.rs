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
fn simulate_render_pipeline(canvas: &skia_safe::Canvas, terminal: &TerminalMock, font: &Font, paint: &Paint) -> Duration {
    let font_width = 10.0;
    let font_height = 20.0;

    let elapsed = measure(|| {
        // 1. 背景清屏 (模拟 terminal clear)
        canvas.clear(Color::new(0xFF000000u32));

        // 2. 逐行绘制 (使用 TextBlob 优化)
        for r in 0..terminal.rows {
            let y_base = (r as f32 + 1.0) * font_height;
            let y_adj = y_base + 3.0;
            let row_start = r * terminal.cols;

            let mut builder = TextBlobBuilder::new();
            let mut has_content = false;

            let mut c = 0;
            while c < terminal.cols {
                let cell_idx = row_start + c;
                if cell_idx >= terminal.cells.len() { break; }
                let (ch, style) = terminal.cells[cell_idx];
                if ch == ' ' { c += 1; continue; }

                // 合并相同样式的 run
                let mut run_chars = Vec::new();

                while c < terminal.cols {
                    let ci = row_start + c;
                    if ci >= terminal.cells.len() { break; }
                    let (ch2, style2) = terminal.cells[ci];
                    if style2 != style || ch2 == ' ' { break; }
                    run_chars.push((ch2, c as f32 * font_width));
                    c += 1;
                }

                if !run_chars.is_empty() {
                    flush_run_to_blob(&mut builder, &run_chars, font);
                    has_content = true;
                }
            }

            if has_content {
                if let Some(blob) = builder.make() {
                    canvas.draw_text_blob(&blob, (0.0, y_adj), paint);
                }
            }
        }
    });

    elapsed
}

fn flush_run_to_blob(builder: &mut TextBlobBuilder, chars: &[(char, f32)], font: &Font) {
    if chars.is_empty() { return; }
    let text: String = chars.iter().map(|(c, _)| *c).collect();
    let mut glyphs = vec![skia_safe::GlyphId::default(); chars.len()];
    font.str_to_glyphs(&text, &mut glyphs);

    let (run_glyphs, run_pos) = builder.alloc_run_pos_h(font, chars.len(), 0.0, None);
    run_glyphs.copy_from_slice(&glyphs);
    for (i, (_, x)) in chars.iter().enumerate() {
        run_pos[i] = *x;
    }
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

    println!("{:<20} | {:<8} | {:<12} | {:<12} | {:<10}",
             "终端配置", "内容", "单元格数", "帧时间", "FPS 估算");
    println!("{:-<78}", "");

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

            // 测量 10 帧
            let iterations = 10;
            let mut total = Duration::ZERO;
            for _ in 0..iterations {
                total += simulate_render_pipeline(canvas, &terminal, &font, &paint);
            }
            let avg = total / iterations as u32;
            let fps = 1.0 / avg.as_secs_f64();

            let cell_str = format!("{:>6}", cell_count);
            let time_str = format!("{:>6.2}ms", avg.as_secs_f64() * 1000.0);
            let fps_str = format!("{:>7.1}", fps);

            println!("{:<20} | {:<8} | {:>6} 格 | {:>10} | {:>8}",
                     label, fill_label, cell_str, time_str, fps_str);
        }
        println!("{:-<78}", "");
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

    // 1. 纯清屏耗时
    let clear_time = measure(|| {
        for _ in 0..100 {
            canvas.clear(Color::new(0xFF000000u32));
        }
    });
    let clear_avg = clear_time / 100;

    // 2. 完整渲染
    let render_time = measure(|| {
        for _ in 0..10 {
            simulate_render_pipeline(canvas, &terminal, &font, &paint);
        }
    });
    let render_avg = render_time / 10;

    // 3. 绘制操作占比
    let draw_time = render_avg.saturating_sub(clear_avg);

    println!("终端配置: {}x{} ({} 单元格)", cols, rows, cols * rows);
    println!("填充模式: 混合 (中文+ASCII)");
    println!();
    println!("  阶段           | 平均耗时    | 占比");
    println!("  {:-<40}", "");
    println!("  清屏 (clear)   | {:>8.3}μs | {:.1}%",
             clear_avg.as_micros() as f64,
             clear_avg.as_micros() as f64 / render_avg.as_micros() as f64 * 100.0);
    println!("  文本绘制 (draw)| {:>8.3}μs | {:.1}%",
             draw_time.as_micros() as f64,
             draw_time.as_micros() as f64 / render_avg.as_micros() as f64 * 100.0);
    println!("  {:-<40}", "");
    println!("  总帧时间       | {:>8.3}μs | 100.0%",
             render_avg.as_micros() as f64);
    println!("  理论 FPS       | {:.1} FPS", 1.0 / render_avg.as_secs_f64());
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
    let start = Instant::now();

    for i in 0..frames {
        simulate_render_pipeline(canvas, &terminal, &font, &paint);
        if (i + 1) % 200 == 0 {
            let elapsed = start.elapsed();
            let fps = (i + 1) as f64 / elapsed.as_secs_f64();
            println!("  帧 {:>4}/{} | 累计 FPS: {:.1}", i + 1, frames, fps);
        }
    }

    let total = start.elapsed();
    let avg_fps = frames as f64 / total.as_secs_f64();
    let avg_frame = total / frames as u32;

    println!();
    println!("  总帧数:       {}", frames);
    println!("  总耗时:       {:.3}s", total.as_secs_f64());
    println!("  平均帧时间:   {:.2}ms", avg_frame.as_secs_f64() * 1000.0);
    println!("  平均 FPS:     {:.1}", avg_fps);
    println!("  渲染单元格总数: {} ({:.1} M cells)", frames * cols * rows, (frames * cols * rows) as f64 / 1_000_000.0);
    println!("  渲染吞吐量:   {:.2} M cells/s", (frames * cols * rows) as f64 / total.as_secs_f64() / 1_000_000.0);
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
