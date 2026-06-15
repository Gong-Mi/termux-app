use std::hint::black_box;
use std::time::Instant;

use termux_rust::terminal::screen::Screen;

fn fill_ascii_screen(s: &mut Screen, cols: usize, rows: usize, pattern: &str) {
    let bytes = pattern.as_bytes();
    for y in 0..rows {
        let row = s.get_row_mut(y as i64);
        for x in 0..cols {
            let b = bytes[(y * cols + x) % bytes.len()];
            row.set_char(x as u64, b as u32, 0);
        }
    }
}

fn bench_latency(name: &str, setup: impl Fn() -> Screen, new_cols: i32, new_rows: i32) {
    const WARMUP: usize = 100;
    const SAMPLES: usize = 5000;

    let mut samples: Vec<u64> = Vec::with_capacity(SAMPLES);

    // Warmup
    for _ in 0..WARMUP {
        let mut s = setup();
        black_box(s.resize_with_reflow(new_cols, new_rows, 0, 0, 0));
    }

    // Collect samples
    for _ in 0..SAMPLES {
        let mut s = setup();
        let t0 = Instant::now();
        black_box(s.resize_with_reflow(new_cols, new_rows, 0, 0, 0));
        let ns = t0.elapsed().as_nanos() as u64;
        samples.push(ns);
    }

    samples.sort_unstable();

    let min = samples[0];
    let p50 = samples[SAMPLES / 2];
    let p90 = samples[SAMPLES * 9 / 10];
    let p99 = samples[SAMPLES * 99 / 100];
    let p999 = samples[SAMPLES * 999 / 1000];
    let max = samples[SAMPLES - 1];
    let avg = samples.iter().sum::<u64>() / SAMPLES as u64;

    println!(
        "case={name} samples={SAMPLES} min={min} avg={avg} p50={p50} p90={p90} p99={p99} p999={p999} max={max}",
    );
}

fn main() {
    bench_latency(
        "ascii_80x24_to_40x12",
        || {
            let mut s = Screen::new(80, 24, 100);
            fill_ascii_screen(
                &mut s,
                80,
                24,
                "abcdefghijklmnopqrstuvwxyz0123456789 [INFO] build log line ",
            );
            s
        },
        40,
        12,
    );

    bench_latency(
        "ascii_80x24_to_120x24",
        || {
            let mut s = Screen::new(80, 24, 100);
            fill_ascii_screen(
                &mut s,
                80,
                24,
                "abcdefghijklmnopqrstuvwxyz0123456789 [INFO] build log line ",
            );
            s
        },
        120,
        24,
    );

    bench_latency(
        "ascii_200x50_to_100x25",
        || {
            let mut s = Screen::new(200, 50, 200);
            fill_ascii_screen(
                &mut s,
                200,
                50,
                "abcdefghijklmnopqrstuvwxyz0123456789 [INFO] build log line ",
            );
            s
        },
        100,
        25,
    );

    bench_latency(
        "ascii_80x24_rows_only_to_12",
        || {
            let mut s = Screen::new(80, 24, 100);
            fill_ascii_screen(
                &mut s,
                80,
                24,
                "abcdefghijklmnopqrstuvwxyz0123456789 [INFO] build log line ",
            );
            s
        },
        80,
        12,
    );

    bench_latency("empty_80x24_to_40x12", || Screen::new(80, 24, 100), 40, 12);
}
