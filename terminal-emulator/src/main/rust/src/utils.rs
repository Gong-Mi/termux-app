#[cfg(target_os = "android")]
use std::ffi::CString;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, Duration};

/// 性能监控器：记录吞吐量和延迟
pub struct PerformanceMetrics {
    pub total_bytes_processed: AtomicU64,
    pub total_render_time_ns: AtomicU64,
    pub frame_count: AtomicU64,
    pub last_report: std::sync::Mutex<Instant>,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            total_bytes_processed: AtomicU64::new(0),
            total_render_time_ns: AtomicU64::new(0),
            frame_count: AtomicU64::new(0),
            last_report: std::sync::Mutex::new(Instant::now()),
        }
    }

    pub fn record_bytes(&self, bytes: u64) {
        self.total_bytes_processed.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_render(&self, duration: Duration) {
        self.total_render_time_ns.fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn try_report(&self) {
        let mut last = self.last_report.lock().unwrap();
        if last.elapsed() >= Duration::from_secs(2) {
            let elapsed_secs = last.elapsed().as_secs_f64();
            let bytes = self.total_bytes_processed.swap(0, Ordering::Relaxed);
            let ns = self.total_render_time_ns.swap(0, Ordering::Relaxed);
            let frames = self.frame_count.swap(0, Ordering::Relaxed);

            let throughput = (bytes as f64 / 1024.0 / 1024.0) / elapsed_secs;
            let avg_render = if frames > 0 { (ns as f64 / 1_000_000.0) / frames as f64 } else { 0.0 };

            android_log(LogPriority::INFO, &format!(
                "[PERF] Throughput: {:.2} MB/s | Avg Render: {:.2} ms | Frames: {}",
                throughput, avg_render, frames
            ));
            
            *last = Instant::now();
        }
    }
}

pub static METRICS: once_cell::sync::Lazy<PerformanceMetrics> = once_cell::sync::Lazy::new(|| PerformanceMetrics::new());

pub enum LogPriority {
    VERBOSE = 2,
    DEBUG = 3,
    INFO = 4,
    WARN = 5,
    ERROR = 6,
}

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn __android_log_print(prio: i32, tag: *const libc::c_char, fmt: *const libc::c_char, ...);
}

pub fn android_log(prio: LogPriority, msg: &str) {
    #[cfg(target_os = "android")]
    {
        let tag = CString::new("Termux-Rust").unwrap();
        let msg_c = CString::new(msg).unwrap();
        unsafe {
            __android_log_print(prio as i32, tag.as_ptr(), msg_c.as_ptr());
        }
    }
    
    #[cfg(not(target_os = "android"))]
    {
        let prefix = match prio {
            LogPriority::ERROR => "E",
            LogPriority::WARN => "W",
            LogPriority::INFO => "I",
            _ => "D",
        };
        println!("[{}] Termux-Rust: {}", prefix, msg);
    }
}

pub fn map_line_drawing(c: u8) -> char {
    match c {
        b'_' => ' ', b'`' => '◆', b'0' => '█', b'a' => '▒', b'b' => '␉',
        b'c' => '␌', b'd' => '\r', b'e' => '␊', b'f' => '°', b'g' => '±',
        b'h' => '\n', b'i' => '␋', b'j' => '┘', b'k' => '┐', b'l' => '┌',
        b'm' => '└', b'n' => '┼', b'o' => '⎺', b'p' => '⎻', b'q' => '─',
        b'r' => '⎼', b's' => '⎽', b't' => '├', b'u' => '┤', b'v' => '┴',
        b'w' => '┬', b'x' => '│', b'y' => '≤', b'z' => '≥', b'{' => 'π',
        b'|' => '≠', b'}' => '£', b'~' => '·', _ => c as char,
    }
}

pub fn get_char_width(ucs: u32) -> usize {
    crate::wcwidth::wcwidth(ucs)
}
