use std::os::unix::net::UnixListener;
use std::io::Write;
use std::time::{Duration, Instant};
use std::thread;
use std::fs;

fn main() {
    // 抽象套接字地址以 \0 开头，Rust 中使用字节数组表示
    let socket_addr = "\0termux_sentinel_abstract";
    
    // UnixListener 在 Linux 上完美支持抽象命名空间
    let listener = UnixListener::bind(socket_addr).expect("Failed to bind abstract socket");
    println!("Sentinel Daemon (Abstract) running...");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                thread::spawn(move || {
                    loop {
                        let start = Instant::now();
                        let load = fs::read_to_string("/proc/loadavg").unwrap_or_default();
                        let report = format!("SYS_LOAD={}", load.trim());
                        
                        if let Err(_) = writeln!(stream, "{}", report) {
                            break;
                        }

                        let elapsed = start.elapsed();
                        let target = Duration::from_millis(50);
                        if elapsed < target {
                            thread::sleep(target - elapsed);
                        }
                    }
                });
            }
            Err(e) => eprintln!("Connection failed: {}", e),
        }
    }
}
