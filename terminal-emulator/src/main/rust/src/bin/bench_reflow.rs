use std::fs;
use std::hint::black_box;
use std::time::{Duration, Instant};

use termux_rust::terminal::screen::Screen;

fn read_rss_kb() -> usize {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                return parts[1].parse::<usize>().unwrap_or(0);
            }
        }
    }
    0
}

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

fn fill_mixed_screen(s: &mut Screen, cols: usize, rows: usize) {
    let pattern: Vec<char> = "abc中文def🙂ghi".chars().collect();
    for y in 0..rows {
        let row = s.get_row_mut(y as i64);
        let mut x = 0usize;
        let mut pat_idx = 0usize;
        while x < cols {
            let c = pattern[pat_idx % pattern.len()];
            let w = termux_rust::wcwidth::wcwidth(c as u32);
            if w == 0 {
                pat_idx += 1;
                continue;
            }
            if x + w > cols {
                break;
            }
            row.set_char(x as u64, c as u32, 0);
            if w == 2 && x + 1 < cols {
                row.set_char((x + 1) as u64, '\0' as u32, 0);
            }
            x += w;
            pat_idx += 1;
        }
    }
}

fn bench_case(name: &str, setup: impl Fn() -> Screen, new_cols: i32, new_rows: i32) {
    let min_duration = Duration::from_millis(250);
    let mut iters = 0usize;
    let mut max_rss = 0usize;
    let start = Instant::now();

    while start.elapsed() < min_duration {
        let mut s = setup();
        black_box(s.resize_with_reflow(new_cols, new_rows, 0, 0, 0));
        let rss = read_rss_kb();
        if rss > max_rss {
            max_rss = rss;
        }
        iters += 1;
    }

    let ns = start.elapsed().as_nanos();
    let ns_per_call = ns as f64 / iters as f64;
    println!("case={name} iters={iters} ns_per_call={ns_per_call:.2} max_rss_kb={max_rss}",);
}

fn main() {
    bench_case(
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

    bench_case(
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

    bench_case(
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

    bench_case(
        "utf8_80x24_to_40x12",
        || {
            let mut s = Screen::new(80, 24, 100);
            fill_mixed_screen(&mut s, 80, 24);
            s
        },
        40,
        12,
    );

    bench_case(
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

    bench_case("empty_80x24_to_40x12", || Screen::new(80, 24, 100), 40, 12);
}
