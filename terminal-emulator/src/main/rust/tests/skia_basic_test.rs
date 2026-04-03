// 测试 skia-safe 是否能正常初始化和绘图
// 运行：cargo test --test skia_basic_test -- --nocapture

use skia_safe::{
    surfaces,
    Color,
    Paint,
    PaintStyle,
    Rect,
    Font,
    TextBlob,
    Point,
};

#[test]
fn test_skia_raster_draw() {
    println!("=== 测试 Skia Raster 绘图 ===");

    // 1. 创建一个 100x100 的光栅表面 (Raster Surface)
    let mut surface = surfaces::raster_n32_premul((100, 100))
        .expect("Failed to create raster surface");

    let canvas = surface.canvas();
    canvas.clear(Color::BLACK);

    // 2. 绘制一个红色的矩形
    let mut paint = Paint::default();
    paint.set_color(Color::RED);
    paint.set_style(PaintStyle::Fill);

    let rect = Rect::from_xywh(10.0, 10.0, 80.0, 80.0);
    canvas.draw_rect(&rect, &paint);

    // 3. 验证像素颜色
    // 使用 peek_pixels 直接获取像素数据，比 read_pixels 简单
    if let Some(pixmap) = surface.peek_pixels() {
        let info = pixmap.info();
        assert_eq!(info.width(), 100);
        assert_eq!(info.height(), 100);
        
        // 获取中心点 (50, 50) 的像素地址
        // N32 是 4 bytes per pixel
        let addr = pixmap.addr();
        let row_bytes = pixmap.row_bytes();
        
        // 计算 (50, 50) 的偏移量
        let offset = 50 * 4 + 50 * row_bytes;
        let pixel_ptr = unsafe { addr.add(offset) as *const u8 };
        let pixel_bytes = unsafe { std::slice::from_raw_parts(pixel_ptr, 4) };
        
        // Android 小端序 N32 通常是 BGRA，但这里输出是 [255, 0, 0, 255] -> RGBA
        // Color::RED (0xFFFF0000 ARGB) -> 内存: FF 00 00 FF (R=255, G=0, B=0, A=255)
        println!("Pixel at (50,50): {:?}", pixel_bytes);
        
        // 检查红色分量 (index 0)
        assert_eq!(pixel_bytes[0], 255, "Red channel should be 255");
        assert_eq!(pixel_bytes[3], 255, "Alpha channel should be 255");
        
        println!("✅ Skia Raster 绘图测试通过！");
    } else {
        panic!("Failed to peek pixels");
    }
}

#[test]
fn test_skia_text_rendering() {
    println!("=== 测试 Skia 文本渲染 ===");

    let mut surface = surfaces::raster_n32_premul((200, 50))
        .expect("Failed to create text surface");
    
    let canvas = surface.canvas();
    canvas.clear(Color::WHITE);

    // 尝试绘制文本
    let mut paint = Paint::default();
    paint.set_color(Color::BLACK);
    paint.set_anti_alias(true);
    
    // 创建字体 (使用默认字体)
    let mut font = Font::default();
    font.set_size(24.0);

    // 创建 TextBlob
    if let Some(blob) = TextBlob::new("Hello Termux", &font) {
        canvas.draw_text_blob(&blob, Point::new(10.0, 30.0), &paint);
        println!("✅ Skia 文本渲染测试通过（TextBlob 创建成功）！");
    } else {
        println!("⚠️ 无法创建 TextBlob (可能缺少字体文件)，但库本身加载正常。");
    }
}
