// Vulkan 10-bit HDR 色彩转换与 SIMD 自适应加速真机基准测试
// 运行：cargo test --test vulkan_hdr_simulation --release -- --nocapture
//
// 验证从标准 8-bit RGBA (RGBA8888) 到 10-bit 高精度 RGBA1010102 (Format::A2B10G10R10)
// 的色彩转换与 Alpha 饱和混合在 CPU SIMD / SVE2 架构下的向量化吞吐表现。

use std::time::{Duration, Instant};

// 模拟 RGBA 8-bit 像素
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pixel8 {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

// 模拟 10-bit 高精度像素的比特紧凑结构 (32-bit: R=10, G=10, B=10, A=2)
type Pixel10Packed = u32;

// =============================================================================
// 1. 标量算法实现 (Scalar Implementation)
// =============================================================================
#[inline(never)]
fn convert_rgba8_to_rgba10_scalar(src: &[Pixel8], dst: &mut [Pixel10Packed]) {
    let len = src.len().min(dst.len());
    for i in 0..len {
        let p = src[i];
        // 将 8-bit (0..255) 线性映射到 10-bit (0..1023)
        let r10 = ((p.r as u32) * 1023 + 127) / 255;
        let g10 = ((p.g as u32) * 1023 + 127) / 255;
        let b10 = ((p.b as u32) * 1023 + 127) / 255;
        let a2 = (p.a as u32) >> 6; // 8-bit alpha 降到 2-bit

        // 紧凑打包：R(10) | G(10) << 10 | B(10) << 20 | A(2) << 30
        dst[i] = r10 | (g10 << 10) | (b10 << 20) | (a2 << 30);
    }
}

// =============================================================================
// 2. SIMD / Autovectorized 算法实现 (可被 ARM NEON/SVE2 自动向量化)
// =============================================================================
// 采用对齐、无跳转、无分支的迭代器与查表/直接移位乘加逻辑，使 LLVM 编译器在 +sve2 特性下
// 能够自动产生自适应向量宽度并行指令（使用 SVE2 的跨步乘加和谓词遮罩）。
#[inline(never)]
fn convert_rgba8_to_rgba10_vectorized(src: &[Pixel8], dst: &mut [Pixel10Packed]) {
    let len = src.len().min(dst.len());
    let src = &src[..len];
    let dst = &mut dst[..len];

    // 使用迭代器 zip 进行无边界检查（Boundary Check Elimination）的快速向量化循环
    for (s_pix, d_packed) in src.iter().zip(dst.iter_mut()) {
        // 利用乘位移估算，代替除法以利于 SIMD 乘法管线 (1023/255 = 4.0117)
        // 映射公式：(val * 4096) / 1020 约等于 val * 4.0117
        let r10 = (((s_pix.r as u32) * 263) >> 6).min(1023);
        let g10 = (((s_pix.g as u32) * 263) >> 6).min(1023);
        let b10 = (((s_pix.b as u32) * 263) >> 6).min(1023);
        let a2 = (s_pix.a as u32) >> 6;

        *d_packed = r10 | (g10 << 10) | (b10 << 20) | (a2 << 30);
    }
}

// =============================================================================
// 3. 测试与基准测试用例
// =============================================================================
#[test]
fn test_color_space_conversion_correctness() {
    let src = vec![
        Pixel8 { r: 255, g: 0, b: 128, a: 255 },
        Pixel8 { r: 0, g: 255, b: 0, a: 64 },
    ];
    let mut dst_scalar = vec![0u32; 2];
    let mut dst_vector = vec![0u32; 2];

    convert_rgba8_to_rgba10_scalar(&src, &mut dst_scalar);
    convert_rgba8_to_rgba10_vectorized(&src, &mut dst_vector);

    // 验证标量与向量化加速算法的精度与输出一致性
    for i in 0..src.len() {
        let diff_r = ((dst_scalar[i] & 0x3FF) as i32 - (dst_vector[i] & 0x3FF) as i32).abs();
        let diff_g = (((dst_scalar[i] >> 10) & 0x3FF) as i32 - ((dst_vector[i] >> 10) & 0x3FF) as i32).abs();
        let diff_b = (((dst_scalar[i] >> 20) & 0x3FF) as i32 - ((dst_vector[i] >> 20) & 0x3FF) as i32).abs();
        let diff_a = (((dst_scalar[i] >> 30) & 0x3) as i32 - ((dst_vector[i] >> 30) & 0x3) as i32).abs();

        // 允许位移乘加有极微小的 1-LSB 精度差，但绝不能发生大跨度溢出
        assert!(diff_r <= 1, "R channel mismatch: scalar={:#x}, vector={:#x}", dst_scalar[i], dst_vector[i]);
        assert!(diff_g <= 1, "G channel mismatch: scalar={:#x}, vector={:#x}", dst_scalar[i], dst_vector[i]);
        assert!(diff_b <= 1, "B channel mismatch: scalar={:#x}, vector={:#x}", dst_scalar[i], dst_vector[i]);
        assert!(diff_a == 0, "Alpha channel mismatch: scalar={:#x}, vector={:#x}", dst_scalar[i], dst_vector[i]);
    }
    println!("精度一致性校验：✅ PASS");
}

#[test]
fn benchmark_hdr_rgba10_conversion() {
    println!("\n========== Vulkan 10-bit HDR 色彩转换 SIMD 跑分 ==========\n");

    // 模拟全屏终端的高精度位图缓冲数据量 (例如 2400x1200 = 2,880,000 像素)
    let size = 2400 * 1200;
    let src_data = vec![Pixel8 { r: 128, g: 64, b: 192, a: 255 }; size];
    let mut dst_scalar = vec![0u32; size];
    let mut dst_vector = vec![0u32; size];

    // 预热 3 次
    for _ in 0..3 {
        convert_rgba8_to_rgba10_scalar(&src_data, &mut dst_scalar);
        convert_rgba8_to_rgba10_vectorized(&src_data, &mut dst_vector);
    }

    // 1. 测量标量算法耗时
    let iters = 10;
    let start_scalar = Instant::now();
    for _ in 0..iters {
        convert_rgba8_to_rgba10_scalar(&src_data, &mut dst_scalar);
    }
    let duration_scalar = start_scalar.elapsed() / iters as u32;

    // 2. 测量 SIMD / Autovectorized 算法耗时
    let start_vector = Instant::now();
    for _ in 0..iters {
        convert_rgba8_to_rgba10_vectorized(&src_data, &mut dst_vector);
    }
    let duration_vector = start_vector.elapsed() / iters as u32;

    let speedup = duration_scalar.as_secs_f64() / duration_vector.as_secs_f64();
    let mpps_scalar = (size as f64 / 1_000_000.0) / duration_scalar.as_secs_f64();
    let mpps_vector = (size as f64 / 1_000_000.0) / duration_vector.as_secs_f64();

    println!("像素数据量:   2,880,000 像素 (模拟 2400x1200 帧图层)");
    println!();
    println!("  算法类型       | 平均耗时   | 像素处理吞吐量");
    println!("  {:-<45}", "");
    println!(
        "  标量算法 (Scalar)| {:>7.2}ms | {:.2} Million Pixels/s",
        duration_scalar.as_secs_f64() * 1000.0,
        mpps_scalar
    );
    println!(
        "  SIMD 向量化 (Auto) | {:>7.2}ms | {:.2} Million Pixels/s",
        duration_vector.as_secs_f64() * 1000.0,
        mpps_vector
    );
    println!("  {:-<45}", "");
    println!("  🚀 性能加速比 (Speedup Ratio): {:.2}x", speedup);
    println!();
}
