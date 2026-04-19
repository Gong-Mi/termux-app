use std::os::unix::net::UnixStream;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};

fn main() {
    let socket_addr = "\0termux_sentinel_abstract";
    let latest_state = Arc::new(Mutex::new(String::new()));
    let state_clone = Arc::clone(&latest_state);

    thread::spawn(move || {
        let stream = loop {
            // 连接到抽象命名空间
            if let Ok(s) = UnixStream::connect(socket_addr) {
                break s;
            }
            thread::sleep(Duration::from_millis(500));
        };

        println!("Connected to Abstract Socket!");
        let reader = BufReader::new(stream);

        for line in reader.lines() {
            if let Ok(msg) = line {
                let mut data = state_clone.lock().unwrap();
                *data = msg;
            } else { break; }
        }
    });

    for i in 0..5 {
        thread::sleep(Duration::from_millis(200));
        let current = latest_state.lock().unwrap().clone();
        println!("[Abstract Stream {}] Received: {}", i, current);
    }
}
