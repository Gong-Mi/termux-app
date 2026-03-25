// Sixel 颜色寄存器测试
// 运行：cargo test --test sixel_color_test -- --nocapture

use termux_rust::terminal::sixel::SixelDecoder;
use termux_rust::vte_parser::Params;

/// 测试颜色寄存器初始化
#[test]
fn test_color_registers_init() {
    println!("\n=== Test: Color Registers Initialization ===");
    
    let decoder = SixelDecoder::new();
    
    // 验证颜色寄存器已初始化
    assert_eq!(decoder.color_registers.len(), 256, "Should have 256 color registers");
    
    // 初始时所有颜色寄存器应为 None
    for (i, reg) in decoder.color_registers.iter().enumerate() {
        assert!(reg.is_none(), "Color register {} should be None initially", i);
    }
    
    println!("  ✅ Color registers initialized correctly");
}

/// 测试颜色选择命令 #
#[test]
fn test_color_select_command() {
    println!("\n=== Test: Color Select Command (#) ===");
    
    let mut decoder = SixelDecoder::new();
    
    // 测试 #0 - 选择颜色 0
    decoder.process_data(b"#0");
    assert_eq!(decoder.current_color, 0, "Current color should be 0");
    println!("  ✅ #0 - Selected color 0");
    
    // 测试 #5 - 选择颜色 5
    decoder.process_data(b"#5");
    assert_eq!(decoder.current_color, 5, "Current color should be 5");
    println!("  ✅ #5 - Selected color 5");
    
    // 测试 #255 - 选择颜色 255
    decoder.process_data(b"#255");
    assert_eq!(decoder.current_color, 255, "Current color should be 255");
    println!("  ✅ #255 - Selected color 255");
}

/// 测试颜色寄存器设置（RGB 空间）
#[test]
fn test_color_register_rgb() {
    println!("\n=== Test: Color Register RGB Space ===");
    
    let mut decoder = SixelDecoder::new();
    
    // 测试 #0;1;100;50;0 - RGB 空间，红色
    // 格式：# Pc ; Ps ; Pr ; Pg ; Pb
    // Ps=1 表示 RGB 空间
    decoder.process_data(b"#0;1;100;0;0");
    
    // 验证颜色寄存器已设置
    assert!(decoder.color_registers[0].is_some(), "Color register 0 should be set");
    let color = decoder.color_registers[0].as_ref().unwrap();
    println!("  Color 0: RGB({}, {}, {})", color.r, color.g, color.b);
    
    // 验证 RGB 值（允许一定误差）
    assert!(color.r >= 250, "Red should be high, got {}", color.r);
    
    println!("  ✅ RGB color register set correctly");
}

/// 测试颜色寄存器设置（HLS 空间）
#[test]
fn test_color_register_hls() {
    println!("\n=== Test: Color Register HLS Space ===");
    
    let mut decoder = SixelDecoder::new();
    
    // 测试 HLS 颜色：红色 (H=0, L=50, S=100)
    decoder.process_data(b"#1;0;0;50;100");
    
    assert!(decoder.color_registers[1].is_some(), "Color register 1 should be set");
    let color = decoder.color_registers[1].as_ref().unwrap();
    println!("  Color 1 (HLS 0,50,100): RGB({}, {}, {})", color.r, color.g, color.b);
    
    // 红色应该主要是红色分量
    assert!(color.r > color.g && color.r > color.b, "Should be red-dominant");
    
    println!("  ✅ HLS color register converted correctly");
}

/// 测试 HLS 转 RGB 辅助函数
#[test]
fn test_hls_to_rgb() {
    println!("\n=== Test: HLS to RGB Conversion ===");
    
    use termux_rust::terminal::sixel::hls_to_rgb;
    
    // 测试红色 (H=0, L=50, S=100)
    let (r, g, b) = hls_to_rgb(0, 50, 100);
    println!("  HLS(0,50,100) -> RGB({},{},{})", r, g, b);
    assert!(r > 200 && g < 50 && b < 50, "Should be red");
    
    // 测试绿色 (H=120, L=50, S=100)
    let (r, g, b) = hls_to_rgb(120, 50, 100);
    println!("  HLS(120,50,100) -> RGB({},{},{})", r, g, b);
    assert!(g > 200 && r < 50 && b < 50, "Should be green");
    
    // 测试蓝色 (H=240, L=50, S=100)
    let (r, g, b) = hls_to_rgb(240, 50, 100);
    println!("  HLS(240,50,100) -> RGB({},{},{})", r, g, b);
    assert!(b > 200 && r < 50 && g < 50, "Should be blue");
    
    // 测试白色 (H=0, L=100, S=0)
    let (r, g, b) = hls_to_rgb(0, 100, 0);
    println!("  HLS(0,100,0) -> RGB({},{},{})", r, g, b);
    assert!(r > 250 && g > 250 && b > 250, "Should be white");
    
    // 测试黑色 (H=0, L=0, S=0)
    let (r, g, b) = hls_to_rgb(0, 0, 0);
    println!("  HLS(0,0,0) -> RGB({},{},{})", r, g, b);
    assert!(r < 10 && g < 10 && b < 10, "Should be black");
    
    // 测试灰色 (H=0, L=50, S=0)
    let (r, g, b) = hls_to_rgb(0, 50, 0);
    println!("  HLS(0,50,0) -> RGB({},{},{})", r, g, b);
    assert!(r > 120 && r < 140, "Should be middle gray");
    assert!(r == g && g == b, "Gray should have equal RGB components");
    
    println!("  ✅ HLS to RGB conversion correct");
}

/// 测试默认颜色表
#[test]
fn test_default_colors() {
    println!("\n=== Test: Default Color Table ===");
    
    use termux_rust::terminal::sixel::index_to_default_color;
    
    // 测试前 16 色
    let expected = vec![
        (0, 0, 0),         // 0: 黑
        (170, 0, 0),       // 1: 红
        (0, 170, 0),       // 2: 绿
        (170, 170, 0),     // 3: 黄
        (0, 0, 170),       // 4: 蓝
        (170, 0, 170),     // 5: 品红
        (0, 170, 170),     // 6: 青
        (170, 170, 170),   // 7: 白
        (85, 85, 85),      // 8: 亮黑
        (255, 85, 85),     // 9: 亮红
        (85, 255, 85),     // 10: 亮绿
        (255, 255, 85),    // 11: 亮黄
        (85, 85, 255),     // 12: 亮蓝
        (255, 85, 255),    // 13: 亮品红
        (85, 255, 255),    // 14: 亮青
        (255, 255, 255),   // 15: 亮白
    ];
    
    for (i, exp) in expected.iter().enumerate() {
        let (r, g, b) = index_to_default_color(i);
        println!("  Color {}: RGB({},{},{})", i, r, g, b);
        assert_eq!((r, g, b), *exp, "Color {} mismatch", i);
    }
    
    println!("  ✅ Default color table correct");
}

/// 测试完整 Sixel 图像带颜色
#[test]
fn test_sixel_image_with_colors() {
    println!("\n=== Test: Sixel Image with Colors ===");
    
    let mut decoder = SixelDecoder::new();
    
    // 初始化 Sixel 图像
    let params = Params::new();
    decoder.start(&params);  // 使用默认尺寸
    
    // 设置颜色寄存器 - 使用 RGB 空间
    decoder.process_data(b"#0;1;100;0;0");  // 颜色 0: 红色 (100%, 0%, 0%)
    decoder.process_data(b"#1;1;0;100;0");  // 颜色 1: 绿色 (0%, 100%, 0%)
    decoder.process_data(b"#2;1;0;0;100");  // 颜色 2: 蓝色 (0%, 0%, 100%)
    
    // 验证颜色寄存器
    assert!(decoder.color_registers[0].is_some(), "Color 0 should be set");
    assert!(decoder.color_registers[1].is_some(), "Color 1 should be set");
    assert!(decoder.color_registers[2].is_some(), "Color 2 should be set");
    
    let color0 = decoder.color_registers[0].as_ref().unwrap();
    let color1 = decoder.color_registers[1].as_ref().unwrap();
    let color2 = decoder.color_registers[2].as_ref().unwrap();
    
    println!("  Color 0: RGB({}, {}, {})", color0.r, color0.g, color0.b);
    println!("  Color 1: RGB({}, {}, {})", color1.r, color1.g, color1.b);
    println!("  Color 2: RGB({}, {}, {})", color2.r, color2.g, color2.b);
    
    // 验证颜色值
    assert!(color0.r > 200 && color0.g < 50 && color0.b < 50, "Color 0 should be red");
    assert!(color1.g > 200 && color1.r < 50 && color1.b < 50, "Color 1 should be green");
    assert!(color2.b > 200 && color2.r < 50 && color2.g < 50, "Color 2 should be blue");
    
    // 获取 RGBA 数据
    let rgba = decoder.get_image_data();
    println!("  Generated {} bytes of RGBA data", rgba.len());
    
    // 验证 RGBA 数据长度
    let expected_pixels = decoder.pixel_data.iter().map(|r| r.len()).sum::<usize>();
    assert_eq!(rgba.len(), expected_pixels * 4, "RGBA data should have 4 bytes per pixel");
    
    println!("  ✅ Sixel image with colors generated correctly");
}

/// 测试颜色循环
#[test]
fn test_color_cycling() {
    println!("\n=== Test: Color Cycling ===");
    
    let mut decoder = SixelDecoder::new();
    
    // 循环设置多个颜色
    for i in 0..16 {
        decoder.process_data(&format!("#{}", i).into_bytes());
        assert_eq!(decoder.current_color, i as usize, "Current color should be {}", i);
    }
    
    // 测试超过 255 的颜色索引（应该循环）
    decoder.process_data(b"#300");
    assert_eq!(decoder.current_color, 300 % 256, "Color index should wrap at 256");
    
    println!("  ✅ Color cycling works correctly");
}

fn main() {
    println!("Sixel Color Register Tests");
    println!("==========================\n");
    
    test_color_registers_init();
    test_color_select_command();
    test_color_register_rgb();
    test_color_register_hls();
    test_hls_to_rgb();
    test_default_colors();
    test_sixel_image_with_colors();
    test_color_cycling();
    
    println!("\n==========================");
    println!("All color tests passed! ✅");
}
