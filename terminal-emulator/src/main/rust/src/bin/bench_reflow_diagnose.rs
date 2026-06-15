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

fn measure_split(
    name: &str,
    setup: impl Fn() -> Screen,
    new_cols: i32,
    new_rows: i32,
    samples: usize,
) {
    let mut alloc_ns: Vec<u64> = Vec::with_capacity(samples);
    let mut fill_ns: Vec<u64> = Vec::with_capacity(samples);
    let mut resize_ns: Vec<u64> = Vec::with_capacity(samples);

    for _ in 0..samples {
        let t0 = Instant::now();
        let mut s = setup();
        let t1 = Instant::now();
        fill_ascii_screen(
            &mut s,
            80,
            24,
            "abcdefghijklmnopqrstuvwxyz0123456789 [INFO] build log line ",
        );
        let t2 = Instant::now();
        black_box(s.resize_with_reflow(new_cols, new_rows, 0, 0, 0));
        let t3 = Instant::now();

        alloc_ns.push((t1 - t0).as_nanos() as u64);
        fill_ns.push((t2 - t1).as_nanos() as u64);
        resize_ns.push((t3 - t2).as_nanos() as u64);
    }

    fn report(label: &str, v: &mut [u64]) {
        v.sort_unstable();
        let n = v.len();
        let min = v[0];
        let avg = v.iter().sum::<u64>() / n as u64;
        let p50 = v[n / 2];
        let p90 = v[n * 9 / 10];
        let p99 = v[n * 99 / 100];
        let p999 = v[n * 999 / 1000];
        let max = v[n - 1];
        println!(
            "  {label}: n={n} min={min} avg={avg} p50={p50} p90={p90} p99={p99} p999={p999} max={max}",
        );
    }

    println!("case={name}");
    report("alloc", &mut alloc_ns);
    report("fill ", &mut fill_ns);
    report("resize", &mut resize_ns);
}

fn main() {
    let samples = 5000;

    measure_split(
        "ascii_80x24_to_40x12",
        || Screen::new(80, 24, 100),
        40,
        12,
        samples,
    );

    measure_split(
        "ascii_80x24_to_120x24",
        || Screen::new(80, 24, 100),
        120,
        24,
        samples,
    );

    measure_split(
        "ascii_200x50_to_100x25",
        || Screen::new(200, 50, 200),
        100,
        25,
        samples,
    );
}
