use std::net::TcpListener;
use std::io::Write;
use std::time::{Duration, Instant};
use std::thread;
use std::fs;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:54321").expect("Failed to bind TCP socket");
    println!("Persistent Sentinel Daemon running at 127.0.0.1:54321");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("Thread connected! Starting high-frequency telemetry...");
                
                thread::spawn(move || {
                    loop {
                        let start = Instant::now();
                        let load = fs::read_to_string("/proc/loadavg").unwrap_or_default();
                        let report = format!("SYS_LOAD={}", load.trim());
                        
                        if let Err(_) = writeln!(stream, "{}", report) {
                            println!("Client thread disconnected.");
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