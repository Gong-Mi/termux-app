use std::net::TcpStream;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};

fn main() {
    let latest_state = Arc::new(Mutex::new(String::new()));
    let state_clone = Arc::clone(&latest_state);

    println!("Starting specific receiver thread...");
    thread::spawn(move || {
        let stream = loop {
            if let Ok(s) = TcpStream::connect("127.0.0.1:54321") {
                break s;
            }
            thread::sleep(Duration::from_millis(500));
        };

        println!("Connected to Daemon via TCP!");
        let reader = BufReader::new(stream);

        for line in reader.lines() {
            if let Ok(msg) = line {
                let mut data = state_clone.lock().unwrap();
                *data = msg;
            } else {
                println!("Lost connection to Daemon!");
                break;
            }
        }
    });

    for i in 0..10 {
        thread::sleep(Duration::from_millis(150));
        let current = latest_state.lock().unwrap().clone();
        println!("[Main Thread loop {}] Current sensed state: {}", i, current);
    }
}