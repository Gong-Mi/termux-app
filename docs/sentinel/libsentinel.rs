use std::net::TcpListener;
use std::io::Write;
use std::time::{Duration, Instant};
use std::thread;
use std::fs;
use std::ffi::CString;

// 使用 libc 修改进程名
extern "C" {
    fn prctl(option: i32, arg2: *const i8, arg3: u64, arg4: u64, arg5: u64) -> i32;
}

const PR_SET_NAME: i32 = 15;

fn main() {
    // 【深度伪装】: 修改进程在系统中的显示名称
    let process_name = CString::new("android.system.proxy").unwrap();
    unsafe {
        // 修正类型强制转换为 *const i8
        prctl(PR_SET_NAME, process_name.as_ptr() as *const i8, 0, 0, 0);
    }

    let listener = TcpListener::bind("127.0.0.1:54321").expect("Failed to bind TCP");
    println!("Sentinel library service started (disguised as .so)...");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                thread::spawn(move || {
                    loop {
                        let start = Instant::now();
                        let load = fs::read_to_string("/proc/loadavg").unwrap_or_default();
                        let report = format!("SYS_LOAD={}", load.trim());
                        
                        if let Err(_) = writeln!(stream, "{}", report) { break; }

                        let elapsed = start.elapsed();
                        let target = Duration::from_millis(50);
                        if elapsed < target { thread::sleep(target - elapsed); }
                    }
                });
            }
            Err(_) => continue,
        }
    }
}
