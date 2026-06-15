use std::hint::black_box;
use std::time::{Duration, Instant};

use termux_rust::sve_scan::{
    fast_skip_printable_len, fast_skip_printable_len_scalar_reference, sve_vector_len_bytes,
};

#[cfg(target_arch = "aarch64")]
use termux_rust::sve_scan::fast_skip_printable_len_sve_unchecked_for_bench;

fn deterministic_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        out.push((x >> 32) as u8);
    }
    out
}

fn ascii(len: usize) -> Vec<u8> {
    let pattern = b"abcdefghijklmnopqrstuvwxyz0123456789 [INFO] build log line ";
    (0..len).map(|i| pattern[i % pattern.len()]).collect()
}

fn mixed_every(len: usize, marker: u8, every: usize) -> Vec<u8> {
    let mut data = ascii(len);
    if every > 0 {
        for i in (every.saturating_sub(1)..len).step_by(every) {
            data[i] = marker;
        }
    }
    data
}

fn utf8_mixed(len: usize) -> Vec<u8> {
    let pattern = "abc中文def🙂ghi".as_bytes();
    (0..len).map(|i| pattern[i % pattern.len()]).collect()
}

fn time_case<F>(data: &[u8], min_duration: Duration, mut f: F) -> (u128, usize, usize)
where
    F: FnMut(&[u8]) -> usize,
{
    let mut iters = 0usize;
    let mut checksum = 0usize;
    let start = Instant::now();
    while start.elapsed() < min_duration {
        checksum = checksum.wrapping_add(black_box(f(black_box(data))));
        iters += 1;
    }
    (start.elapsed().as_nanos(), iters, checksum)
}

fn ns_per_input_byte(ns: u128, iters: usize, len: usize) -> f64 {
    ns as f64 / (iters as f64 * len.max(1) as f64)
}

fn ns_per_scanned_byte(ns: u128, scanned_sum: usize) -> f64 {
    if scanned_sum == 0 {
        f64::INFINITY
    } else {
        ns as f64 / scanned_sum as f64
    }
}

fn print_result(case: &str, data: &[u8]) {
    let min_duration = Duration::from_millis(250);
    let (scalar_ns, scalar_iters, scalar_sum) =
        time_case(data, min_duration, fast_skip_printable_len_scalar_reference);
    let scalar_ns_per_input_byte = ns_per_input_byte(scalar_ns, scalar_iters, data.len());
    let scalar_ns_per_scanned_byte = ns_per_scanned_byte(scalar_ns, scalar_sum);

    let (dispatch_ns, dispatch_iters, dispatch_sum) =
        time_case(data, min_duration, fast_skip_printable_len);
    let dispatch_ns_per_input_byte = ns_per_input_byte(dispatch_ns, dispatch_iters, data.len());
    let dispatch_ns_per_scanned_byte = ns_per_scanned_byte(dispatch_ns, dispatch_sum);

    print!(
        "case={case} len={} scalar_ns_per_input_byte={:.4} dispatch_ns_per_input_byte={:.4} input_speedup={:.3} scalar_ns_per_scanned_byte={:.4} dispatch_ns_per_scanned_byte={:.4} scanned_speedup={:.3} scalar_sum={} dispatch_sum={}",
        data.len(),
        scalar_ns_per_input_byte,
        dispatch_ns_per_input_byte,
        scalar_ns_per_input_byte / dispatch_ns_per_input_byte,
        scalar_ns_per_scanned_byte,
        dispatch_ns_per_scanned_byte,
        scalar_ns_per_scanned_byte / dispatch_ns_per_scanned_byte,
        scalar_sum,
        dispatch_sum
    );

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("sve") {
            let (sve_ns, sve_iters, sve_sum) = unsafe {
                time_case(data, min_duration, |d| {
                    fast_skip_printable_len_sve_unchecked_for_bench(d)
                })
            };
            let sve_ns_per_input_byte = ns_per_input_byte(sve_ns, sve_iters, data.len());
            let sve_ns_per_scanned_byte = ns_per_scanned_byte(sve_ns, sve_sum);
            print!(
                " sve_direct_ns_per_input_byte={:.4} sve_direct_input_speedup={:.3} sve_direct_ns_per_scanned_byte={:.4} sve_direct_scanned_speedup={:.3} sve_sum={}",
                sve_ns_per_input_byte,
                scalar_ns_per_input_byte / sve_ns_per_input_byte,
                sve_ns_per_scanned_byte,
                scalar_ns_per_scanned_byte / sve_ns_per_scanned_byte,
                sve_sum
            );
        }
    }

    println!();
}

fn main() {
    #[cfg(target_arch = "aarch64")]
    {
        println!(
            "arch=aarch64 neon={} sve={} sve2={} vl_bytes={}",
            std::arch::is_aarch64_feature_detected!("neon"),
            std::arch::is_aarch64_feature_detected!("sve"),
            std::arch::is_aarch64_feature_detected!("sve2"),
            sve_vector_len_bytes()
        );
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        println!("arch=not-aarch64 vl_bytes={}", sve_vector_len_bytes());
    }

    let cases = [
        ("short_16", ascii(16)),
        ("short_32", ascii(32)),
        ("short_64", ascii(64)),
        ("ascii_1kb", ascii(1024)),
        ("ascii_64kb", ascii(64 * 1024)),
        ("ascii_1mb", ascii(1024 * 1024)),
        ("escape_every_80", mixed_every(1024 * 1024, 0x1b, 80)),
        ("newline_every_80", mixed_every(1024 * 1024, b'\n', 80)),
        ("utf8_mixed", utf8_mixed(1024 * 1024)),
        ("random_1mb", deterministic_bytes(1024 * 1024, 0x515645)),
    ];

    for (name, data) in cases {
        print_result(name, &data);
    }
}
