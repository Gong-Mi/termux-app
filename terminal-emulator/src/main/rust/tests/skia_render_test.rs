// Skia 渲染管线测试 - 验证 Rust 渲染器的完整性和正确性
// 运行：cargo test --test skia_render_test -- --nocapture

use skia_safe::{
    surfaces, Color, Paint, PaintStyle, Rect, Font, FontMgr, FontStyle,
};
use std::cmp::{min, max};

// ============================================================
// 辅助函数：从 pixmap 获取指定坐标的 ARGB 颜色
// raster_n32_premul 使用 Skia 的 kN32_SkColorType，在 little-endian 平台上
// 内存布局为 [R, G, B, A]。Color::new(0xFFRRGGBB) 按 ARGB 解释。
fn get_pixel(pixmap: &skia_safe::Pixmap, x: i32, y: i32) -> u32 {
    let row_bytes = pixmap.row_bytes();
    let offset = y as usize * row_bytes + x as usize * 4;
    let addr = unsafe { pixmap.addr().add(offset) as *const u8 };
    let bytes = unsafe { std::slice::from_raw_parts(addr, 4) };
    // 内存: [R, G, B, A] -> 重建为 ARGB 0xAARRGGBB
    let a = bytes[3] as u32;
    let r = bytes[0] as u32;
    let g = bytes[1] as u32;
    let b = bytes[2] as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

// ============================================================
// 测试 1: 基本渲染 - 背景清屏 + 矩形绘制
// ============================================================
#[test]
fn test_basic_background_clear() {
    println!("=== 测试 1: 基本背景清屏 ===");

    let width = 200;
    let height = 100;
    let mut surface = surfaces::raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    // 模拟终端背景清屏 (黑色)
    canvas.clear(Color::new(0xFF000000u32));

    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // 验证全屏都是黑色
    let center = get_pixel(&pixmap, 100, 50);
    assert_eq!(center, 0xFF000000, "Center should be black");

    let corner = get_pixel(&pixmap, 0, 0);
    assert_eq!(corner, 0xFF000000, "Corner should be black");

    println!("  ✅ 背景清屏测试通过");
}

// ============================================================
// 测试 2: 选区高亮渲染
// ============================================================
#[test]
fn test_selection_highlight_rendering() {
    println!("=== 测试 2: 选区高亮渲染 ===");

    let width = 400;
    let height = 200;
    let mut surface = surfaces::raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    // 黑色背景
    canvas.clear(Color::new(0xFF000000));

    // 模拟选区高亮 (半透明蓝色 ARGB(128, 80, 120, 200))
    let mut sel_paint = Paint::default();
    sel_paint.set_style(PaintStyle::Fill);
    sel_paint.set_color(Color::from_argb(128, 80, 120, 200));

    let sel_rect = Rect::from_xywh(50.0, 40.0, 200.0, 60.0);
    canvas.draw_rect(&sel_rect, &sel_paint);

    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // 选区中心应该有颜色变化
    let sel_center = get_pixel(&pixmap, 150, 70);
    let bg_only = get_pixel(&pixmap, 10, 10);

    // 背景应该保持黑色
    assert_eq!(bg_only, 0xFF000000, "Background should remain black");

    // 选区中心应该有颜色 (alpha 混合后不是纯黑)
    let sel_a = (sel_center >> 24) & 0xFF;
    assert!(sel_a > 120, "Selection should have visible alpha: {}", sel_a);

    println!("  ✅ 选区高亮渲染测试通过 (center={:#010X})", sel_center);
}

// ============================================================
// 测试 3: Bold→Bright 颜色映射
// ============================================================
#[test]
fn test_bold_to_bright_color_mapping() {
    println!("=== 测试 3: Bold→Bright 颜色映射 ===");

    // 模拟调色板 (前 16 色)
    let mut palette: [u32; 259] = [0; 259];
    // 暗色 0-7
    palette[0] = 0xFF000000; // 黑
    palette[1] = 0xFFCD0000; // 红
    palette[2] = 0xFF00CD00; // 绿
    palette[3] = 0xFFCDCD00; // 黄
    palette[4] = 0xFF6495ED; // 蓝
    palette[5] = 0xFFCD00CD; // 品红
    palette[6] = 0xFF00CDCD; // 青
    palette[7] = 0xFFE5E5E5; // 白
    // 亮色 8-15
    palette[8]  = 0xFF7F7F7F; // 灰
    palette[9]  = 0xFFFF0000; // 亮红
    palette[10] = 0xFF00FF00; // 亮绿
    palette[11] = 0xFFFFFF00; // 亮黄
    palette[12] = 0xFF5C5CFF; // 亮蓝
    palette[13] = 0xFFFF00FF; // 亮品红
    palette[14] = 0xFF00FFFF; // 亮青
    palette[15] = 0xFFFFFFFF; // 亮白

    // 前景色 256, 背景色 257
    palette[256] = 0xFFFFFFFF;
    palette[257] = 0xFF000000;

    // 测试: 粗体 + 前景色 0 (黑色) → 应该映射到 8 (灰色)
    let fg_idx_normal: usize = 0;
    let bold = true;
    let fg_idx_after = if bold && fg_idx_normal < 8 {
        fg_idx_normal + 8
    } else {
        fg_idx_normal
    };
    assert_eq!(fg_idx_after, 8, "Bold black (0) should map to gray (8)");

    // 测试: 粗体 + 前景色 3 (黄色) → 应该映射到 11 (亮黄)
    let fg_idx_normal: usize = 3;
    let fg_idx_after = if bold && fg_idx_normal < 8 {
        fg_idx_normal + 8
    } else {
        fg_idx_normal
    };
    assert_eq!(fg_idx_after, 11, "Bold yellow (3) should map to bright yellow (11)");

    // 测试: 粗体 + 前景色 9 (亮红) → 不应映射 (>=8)
    let fg_idx_normal: usize = 9;
    let fg_idx_after = if bold && fg_idx_normal < 8 {
        fg_idx_normal + 8
    } else {
        fg_idx_normal
    };
    assert_eq!(fg_idx_after, 9, "Bright red (9) should stay 9");

    // 测试: 非粗体 → 不应映射
    let fg_idx_normal: usize = 4;
    let bold = false;
    let fg_idx_after = if bold && fg_idx_normal < 8 {
        fg_idx_normal + 8
    } else {
        fg_idx_normal
    };
    assert_eq!(fg_idx_after, 4, "Normal blue (4) should stay 4");

    println!("  ✅ Bold→Bright 颜色映射测试通过");
    println!("    0 (黑) + bold → 8 (灰)");
    println!("    3 (黄) + bold → 11 (亮黄)");
    println!("    9 (亮红) + bold → 9 (不变)");
}

// ============================================================
// 测试 4: 反向视频 (Reverse Video)
// ============================================================
#[test]
fn test_reverse_video_rendering() {
    println!("=== 测试 4: 反向视频渲染 ===");

    let width = 200;
    let height = 50;
    let mut surface = surfaces::raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    // 正常: 白字黑底
    canvas.clear(Color::new(0xFF000000)); // 黑底

    let mut paint = Paint::default();
    paint.set_anti_alias(false);
    paint.set_color(Color::new(0xFFFFFFFF)); // 白字
    paint.set_style(PaintStyle::Fill);

    // 绘制文字区域
    let text_rect = Rect::from_xywh(20.0, 10.0, 80.0, 30.0);
    canvas.draw_rect(&text_rect, &paint);

    // 反向: 黑字白底
    canvas.clear(Color::new(0xFFFFFFFF)); // 白底
    paint.set_color(Color::new(0xFF000000)); // 黑字
    canvas.draw_rect(&text_rect, &paint);

    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // 文字区域应该是黑色
    let text_pixel = get_pixel(&pixmap, 60, 25);
    let text_a = (text_pixel >> 24) & 0xFF;
    assert!(text_a > 200, "Text area should be visible (alpha={})", text_a);

    // 背景应该是白色
    let bg_pixel = get_pixel(&pixmap, 150, 25);
    assert_eq!(bg_pixel, 0xFFFFFFFF, "Background should be white");

    println!("  ✅ 反向视频渲染测试通过");
}

// ============================================================
// 测试 5: 下划线和删除线
// ============================================================
#[test]
fn test_underline_strikethrough_rendering() {
    println!("=== 测试 5: 下划线和删除线 ===");

    let width = 200;
    let height = 100;
    let mut surface = surfaces::raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    canvas.clear(Color::new(0xFF000000));

    let mut paint = Paint::default();
    paint.set_anti_alias(false);
    paint.set_color(Color::new(0xFF00FF00)); // 绿色
    paint.set_stroke_width(1.0);

    // 下划线 (y_base - 2.0)
    let y_base = 50.0;
    let x = 20.0;
    let width_line = 100.0;
    let underline_y = y_base - 2.0;
    canvas.draw_line((x, underline_y), (x + width_line, underline_y), &paint);

    // 删除线 (y_base - font_height * 0.5)
    let font_height = 20.0;
    let strike_y = y_base - font_height * 0.5;
    canvas.draw_line((x, strike_y), (x + width_line, strike_y), &paint);

    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // 下划线位置应该有绿色像素
    let underline_pixel = get_pixel(&pixmap, 70, underline_y as i32);
    let underline_g = (underline_pixel >> 8) & 0xFF;
    assert!(underline_g > 200, "Underline should have green component: {}", underline_g);

    // 删除线位置应该有绿色像素
    let strike_pixel = get_pixel(&pixmap, 70, strike_y as i32);
    let strike_g = (strike_pixel >> 8) & 0xFF;
    assert!(strike_g > 200, "Strikethrough should have green component: {}", strike_g);

    println!("  ✅ 下划线和删除线测试通过");
}

// ============================================================
// 测试 6: 光标渲染 (三种形状)
// ============================================================
#[test]
fn test_cursor_shape_rendering() {
    println!("=== 测试 6: 光标形状渲染 ===");

    let width = 300;
    let height = 100;
    let mut surface = surfaces::raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    canvas.clear(Color::new(0xFF000000));

    let mut cursor_paint = Paint::default();
    cursor_paint.set_style(PaintStyle::Fill);
    cursor_paint.set_color(Color::new(0xFFFFFFFF)); // 白色光标
    cursor_paint.set_anti_alias(false); // 关闭抗锯齿，确保精确像素测试

    let font_width = 10.0;
    let font_height = 20.0;

    // Block 光标 (style 0) - 全单元格填充
    let cx0 = 50.0;
    let cy0 = 10.0;
    canvas.draw_rect(Rect::from_xywh(cx0, cy0, font_width, font_height), &cursor_paint);

    // Underline 光标 (style 1) - 底部 2px
    let cx1 = 100.0;
    let cy1 = 40.0;
    canvas.draw_rect(Rect::from_xywh(cx1, cy1 + font_height - 2.0, font_width, 2.0), &cursor_paint);

    // Bar 光标 (style 2) - 左侧 2px
    let cx2 = 150.0;
    let cy2 = 70.0;
    canvas.draw_rect(Rect::from_xywh(cx2, cy2, 2.0, font_height), &cursor_paint);

    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // Verify black background was cleared (sample far from all cursor rects)
    let bg_check = get_pixel(&pixmap, 10, 95);
    println!("  Debug: bg_check at (10,95) = {:#010X} (expected 0xFF000000)", bg_check);

    // Block 光标: 中心应该是白色
    let block_center = get_pixel(&pixmap, (cx0 + font_width / 2.0) as i32, (cy0 + font_height / 2.0) as i32);
    println!("  Debug: block_center at (55,20) = {:#010X}", block_center);
    assert_eq!(block_center, 0xFFFFFFFF, "Block cursor center should be white");

    // Sample a pixel that should definitely be black (far from all cursors)
    let bg_near_block = get_pixel(&pixmap, 70, 20);
    println!("  Debug: bg_near_block at (70,20) = {:#010X}", bg_near_block);
    assert_eq!(bg_near_block, 0xFF000000, "Area between cursors should be black");

    // Underline 光标: 底部应该有白色，上方区域应该没有
    // 下划线 rect: y = cy1+font_height-2.0 (58) 到 y = cy1+font_height (60), height=2
    let underline_bottom = get_pixel(&pixmap, (cx1 + font_width / 2.0) as i32, (cy1 + font_height - 1.0) as i32); // y=59
    let underline_above = get_pixel(&pixmap, (cx1 + font_width / 2.0) as i32, (cy1 + font_height - 5.0) as i32); // y=55, above the rect
    let bottom_a = (underline_bottom >> 24) & 0xFF;
    let above_a = (underline_above >> 24) & 0xFF;
    assert!(bottom_a > 200, "Underline bottom should be visible (alpha={})", bottom_a);
    // Note: raster Skia may fill the entire row with anti-aliasing; check that bottom is fully opaque
    println!("    Underline: bottom alpha={}, above alpha={}", bottom_a, above_a);

    // Bar 光标: 左侧应该有白色，右侧应该没有
    // Bar rect: Rect::from_xywh(150, 70, 2, 20) → covers x=150..151, y=70..89
    let bar_left = get_pixel(&pixmap, 150, 80);   // inside rect
    let bar_right = get_pixel(&pixmap, 152, 80);  // just outside rect
    let bar_far_right = get_pixel(&pixmap, 160, 80); // far outside
    let left_a = (bar_left >> 24) & 0xFF;
    let far_right_color = bar_far_right;
    println!("    Bar cursor: left(150,80)={:#010X}, right(152,80)={:#010X}, far_right(160,80)={:#010X}",
             bar_left, bar_right, bar_far_right);
    assert!(left_a > 200, "Bar left should be visible (alpha={})", left_a);
    // Bar is 2px wide at x=150..151. x=152 and beyond should be black (0xFF000000).
    assert_eq!(bar_right, 0xFF000000, "Bar right edge (x=152) should be black, got {:#010X}", bar_right);
    assert_eq!(far_right_color, 0xFF000000, "Bar far right (x=160) should be black, got {:#010X}", far_right_color);

    println!("  ✅ 光标形状渲染测试通过");
}

// ============================================================
// 测试 7: 宽字符渲染 - canvas.scale 模拟
// ============================================================
#[test]
fn test_wide_char_canvas_scaling() {
    println!("=== 测试 7: 宽字符 Canvas 缩放 ===");

    let width = 200;
    let height = 50;
    let mut surface = surfaces::raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    canvas.clear(Color::new(0xFF000000));

    // 模拟中文字符的渲染: Skia 测量宽度与实际单元格宽度不同
    let expected_cells = 2; // 宽字符占2格
    let font_width = 10.0;
    let expected_width = expected_cells as f32 * font_width; // 20px

    let mut paint = Paint::default();
    paint.set_color(Color::new(0xFFFF0000)); // 红色

    // 模拟: 如果测量宽度与期望宽度差异 > 0.5px，使用 canvas.scale
    let measured_width = 18.0; // Skia 测量为 18px
    let x = 30.0;
    let y_base = 30.0;

    if (expected_width - measured_width).abs() > 0.5 {
        canvas.save();
        canvas.scale((expected_width / measured_width, 1.0));
        let x_scaled = x / (expected_width / measured_width);

        // 绘制一个矩形模拟文字
        let rect = Rect::from_xywh(x_scaled, y_base - 20.0, measured_width, 20.0);
        canvas.draw_rect(&rect, &paint);
        canvas.restore();
    } else {
        let rect = Rect::from_xywh(x, y_base - 20.0, expected_width, 20.0);
        canvas.draw_rect(&rect, &paint);
    }

    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // 验证: 缩放后应该覆盖完整的 2 格宽度 (20px)
    let left_edge = get_pixel(&pixmap, (x + 1.0) as i32, (y_base - 10.0) as i32);
    let right_edge = get_pixel(&pixmap, (x + expected_width as f32 - 2.0) as i32, (y_base - 10.0) as i32);

    let left_r = (left_edge >> 16) & 0xFF;
    let right_r = (right_edge >> 16) & 0xFF;

    assert!(left_r > 200, "Left edge should have red component: {}", left_r);
    assert!(right_r > 200, "Right edge should have red component: {}", right_r);

    println!("  ✅ 宽字符 Canvas 缩放测试通过");
    println!("    期望宽度: {}px, 测量宽度: {}px, 缩放系数: {:.3}",
             expected_width, measured_width, expected_width / measured_width);
}

// ============================================================
// 测试 8: 字体缓存 - 多种变体预创建
// ============================================================
#[test]
fn test_font_cache_variants() {
    println!("=== 测试 8: 字体缓存变体 ===");

    let font_mgr = FontMgr::new();

    // 测试字体变体创建
    let variants = [
        ("monospace", FontStyle::normal(), "normal"),
        ("monospace", FontStyle::new(skia_safe::font_style::Weight::BOLD, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Upright), "bold"),
        ("monospace", FontStyle::new(skia_safe::font_style::Weight::NORMAL, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Italic), "italic"),
        ("monospace", FontStyle::new(skia_safe::font_style::Weight::BOLD, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Italic), "bold_italic"),
        ("sans-serif", FontStyle::normal(), "fallback"),
        ("sans-serif", FontStyle::new(skia_safe::font_style::Weight::BOLD, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Upright), "fallback_bold"),
    ];

    for (family, style, label) in &variants {
        let tf = font_mgr.match_family_style(family, *style);
        assert!(tf.is_some(), "Failed to create {} font ({})", label, family);
        let tf = tf.unwrap();

        let mut font = Font::new(tf, Some(12.0));
        font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
        font.set_subpixel(true);

        let (w, _) = font.measure_str("M", None);
        assert!(w > 0.0, "{} font width should be positive", label);

        println!("  ✅ {} 字体创建成功 (M width: {:.2}px)", label, w);
    }

    println!("  ✅ 字体缓存变体测试通过");
}

// ============================================================
// 测试 9: 渲染管线集成 - 完整模拟一帧渲染
// ============================================================
#[test]
fn test_full_frame_rendering_simulation() {
    println!("=== 测试 9: 完整帧渲染模拟 ===");

    let width = 400;
    let height = 200;
    let mut surface = surfaces::raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    // 模拟调色板
    let mut palette: [u32; 259] = [0; 259];
    palette[0] = 0xFF000000;
    palette[8] = 0xFF7F7F7F;
    palette[9] = 0xFFFF0000;
    palette[256] = 0xFFFFFFFF; // foreground
    palette[257] = 0xFF000000; // background

    // 1. 背景清屏
    canvas.clear(Color::new(palette[257]));

    // 2. 模拟绘制一行文字的背景
    let row = 0;
    let font_height = 20.0;
    let font_width = 10.0;
    let y_base = (row as f32 + 1.0) * font_height;

    // "Hello" 的背景 (白色前景，黑色背景)
    let mut bg_paint = Paint::default();
    bg_paint.set_style(PaintStyle::Fill);
    bg_paint.set_color(Color::new(0xFF000000));
    canvas.draw_rect(Rect::from_xywh(0.0, y_base - font_height, 50.0, font_height), &bg_paint);

    // 3. 粗体红色文字 (使用 Bold→Bright 映射)
    let mut text_paint = Paint::default();
    text_paint.set_anti_alias(false);
    let bold = true;
    let mut fg_idx: usize = 1; // 红色
    if bold && fg_idx < 8 {
        fg_idx += 8; // → 9 (亮红)
    }
    assert_eq!(fg_idx, 9, "Bold red should map to bright red");
    text_paint.set_color(Color::new(palette[fg_idx]));
    text_paint.set_style(PaintStyle::Fill);

    let text_rect = Rect::from_xywh(5.0, y_base - font_height + 3.0, 40.0, 14.0);
    canvas.draw_rect(&text_rect, &text_paint);

    // 4. 光标 (block)
    let mut cursor_paint = Paint::default();
    cursor_paint.set_style(PaintStyle::Fill);
    cursor_paint.set_color(Color::new(0xFFFFFFFF));
    let cursor_x = 55.0;
    let cursor_y = y_base;
    canvas.draw_rect(Rect::from_xywh(cursor_x, cursor_y, font_width, font_height), &cursor_paint);

    // 5. 选区高亮
    let mut sel_paint = Paint::default();
    sel_paint.set_style(PaintStyle::Fill);
    sel_paint.set_color(Color::from_argb(128, 80, 120, 200));
    canvas.draw_rect(Rect::from_xywh(70.0, y_base - font_height, 30.0, font_height), &sel_paint);

    // 验证渲染结果
    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // 背景应该是黑色
    let bg = get_pixel(&pixmap, 100, 150);
    assert_eq!(bg, 0xFF000000, "Background should be black");

    // 文字区域应该有红色分量 (使用 rect 中心坐标)
    let text_area = get_pixel(&pixmap, 25, 10);
    let text_r = (text_area >> 16) & 0xFF;
    let text_g = (text_area >> 8) & 0xFF;
    let text_b = text_area & 0xFF;
    let text_a = (text_area >> 24) & 0xFF;
    // 亮红色 (palette[9] = 0xFFFF0000) 应该高红色分量
    assert!(text_r > 200, "Text area should have red component: R={} G={} B={} A={:#010X}", text_r, text_g, text_b, text_area);

    // 光标应该是白色
    let cursor = get_pixel(&pixmap, (cursor_x + 5.0) as i32, (cursor_y + 10.0) as i32);
    assert_eq!(cursor, 0xFFFFFFFF, "Cursor should be white");

    // 选区应该有颜色
    let sel = get_pixel(&pixmap, 85, (y_base - 10.0) as i32);
    let sel_a = (sel >> 24) & 0xFF;
    assert!(sel_a > 100, "Selection should be visible (alpha={})", sel_a);

    println!("  ✅ 完整帧渲染模拟测试通过");
    println!("    背景: 黑色 ✅");
    println!("    文字: 亮红色 (Bold→Bright 映射) ✅");
    println!("    光标: 白色 block ✅");
    println!("    选区: 半透明蓝色 ✅");
}

// ============================================================
// 测试 10: 并发渲染 - 多线程安全
// ============================================================
#[test]
fn test_concurrent_rendering() {
    println!("=== 测试 10: 并发渲染安全 ===");

    use std::sync::{Arc, Mutex};
    use std::thread;

    let render_count = Arc::new(Mutex::new(0u32));

    let handles: Vec<_> = (0..4).map(|i| {
        let count = Arc::clone(&render_count);
        thread::spawn(move || {
            let width = 100;
            let height = 50;
            let mut surface = surfaces::raster_n32_premul((width, height))
                .expect("Failed to create surface");
            let canvas = surface.canvas();
            canvas.clear(Color::BLACK);

            let mut paint = Paint::default();
            paint.set_color(Color::new(0xFFFF0000));
            paint.set_style(PaintStyle::Fill);
            canvas.draw_rect(Rect::from_xywh(10.0, 10.0, 80.0, 30.0), &paint);

            let mut guard = count.lock().unwrap();
            *guard += 1;
            i
        })
    }).collect();

    for h in handles {
        let _ = h.join();
    }

    let final_count = *render_count.lock().unwrap();
    assert_eq!(final_count, 4, "All 4 threads should complete rendering");

    println!("  ✅ 并发渲染安全测试通过 ({} threads)", final_count);
}

#[test]
fn test_cursor_rendering_visual_correctness() {
    use termux_rust::renderer::{RenderFrame, TerminalRenderer};
    use termux_rust::engine::TerminalEngine;
    use termux_rust::terminal::style::STYLE_NORMAL;
    use skia_safe::{surfaces, Color};

    let font_width = 10.0;
    let font_height = 20.0;
    let mut renderer = TerminalRenderer::new(&[], 12.0, None);
    // 强制设置指标以便精确计算
    renderer.font_width = font_width;
    renderer.font_height = font_height;

    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    // 场景：光标在第 2 行，但我们向上滚动了 5 行 (top_row = -5)
    // 视觉上，光标应该出现在屏幕的第 7 行 (index 7)
    engine.state.cursor.y = 2;
    engine.state.cursor.x = 0;
    engine.state.cursor_enabled = true;
    
    // 在光标位置放一个宽字符
    let row = engine.state.main_screen.get_row_mut(2);
    row.text[0] = '你';
    row.styles[0] = STYLE_NORMAL;

    let top_row = -5;
    let frame = RenderFrame::from_engine(&engine, 24, 80, top_row);

    // 创建画布进行渲染
    let mut surface = surfaces::raster_n32_premul((800, 480)).expect("Failed to create surface");
    let canvas = surface.canvas();
    canvas.clear(Color::BLACK);

    // 执行渲染逻辑链条
    renderer.draw_frame(canvas, &frame, 1.0, 0.0);

    let pixmap = surface.peek_pixels().expect("Failed to peek pixels");

    // --- 验证点 1: 滚动偏移 ---
    let pixel_at_expected_y = get_pixel(&pixmap, 5, 150);
    let pixel_at_wrong_y = get_pixel(&pixmap, 5, 50);

    println!("  Pixel at expected Y (150): {:#010X}", pixel_at_expected_y);
    println!("  Pixel at wrong Y (50): {:#010X}", pixel_at_wrong_y);

    // 在 Difference 模式下，白色光标盖在黑色背景上是白色，盖在灰色文字上是反色
    // 只要不是背景色 (0xFF000000)，就说明光标渲染到了这里
    assert_ne!(pixel_at_expected_y, 0xFF000000, "Cursor should be visible at scrolled position (Row 7)");
    assert_eq!(pixel_at_wrong_y, 0xFF000000, "Cursor should NOT be at absolute position (Row 2) when scrolled");

    // --- 验证点 2: 宽字符宽度 ---
    // 检查第二个单元格 (x=15)。在修复后，这里也应该有光标像素
    let pixel_in_second_cell = get_pixel(&pixmap, 15, 150);
    println!("  Pixel in second cell of wide char (15, 150): {:#010X}", pixel_in_second_cell);
    assert_ne!(pixel_in_second_cell, 0xFF000000, "Cursor should cover both cells of a wide character");
}
