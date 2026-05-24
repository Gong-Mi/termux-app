//! Benchmark binary for SVE2 vs scalar conversion speed

use std::time::Instant;
use std::hint::black_box;
use termux_rust::simd;
use termux_rust::pixel::Pixel8;

fn generate_data(len: usize) -> Vec<Pixel8> {
    // generate dummy pixel data (RGBA8 packed as u32)
    (0..len).map(|i| Pixel8 { r: (i as u8), g: (i as u8), b: (i as u8), a: 255 }).collect()
}

fn main() {
    // Use a larger dataset to get meaningful timings
    let src = generate_data(1_000_000); // ~4 MiB of pixel data
    let mut dst = vec![0u32; src.len()];

    // Warm‑up both implementations
    simd::scalar::convert_rgba8_to_rgba10_scalar(black_box(&src), black_box(&mut dst));
    unsafe { simd::sve2::convert_rgba8_to_rgba10_sve2(black_box(&src), black_box(&mut dst)) };

    // Scalar timing
    let start = Instant::now();
    simd::scalar::convert_rgba8_to_rgba10_scalar(black_box(&src), black_box(&mut dst));
    let scalar_dur = start.elapsed();

    // SVE2 timing
    let start = Instant::now();
    unsafe { simd::sve2::convert_rgba8_to_rgba10_sve2(black_box(&src), black_box(&mut dst)) };
    let sve2_dur = start.elapsed();

    println!("Scalar elapsed:   {:.3?}", scalar_dur);
    println!("SVE2 elapsed:    {:.3?}", sve2_dur);
}
