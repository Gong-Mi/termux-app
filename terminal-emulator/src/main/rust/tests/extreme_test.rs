use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use termux_rust::pty;

#[test]
fn test_extreme_concurrent_spawn_under_pressure() {
    let running = Arc::new(AtomicBool::new(true));
    let success_count = Arc::new(AtomicUsize::new(0));
    let total_attempts = 64; // 扩大到 64 个并发线程
    let total_latency_ms = Arc::new(AtomicUsize::new(0));

    // 1. 启动 10 个高强度干扰线程
    for t in 0..10 {
        let r = running.clone();
        thread::spawn(move || {
            while r.load(Ordering::Relaxed) {
                let mut trash = Vec::with_capacity(100);
                for _ in 0..100 {
                    trash.push(vec![t as u8; 1024]);
                }
                drop(trash);
            }
        });
    }
    println!(
        "[TEST] 10 noise threads active. Attempting {} spawns with system limit=24.",
        total_attempts
    );

    // 2. 并发启动子进程
    let mut handles = vec![];
    let test_start = Instant::now();
    for i in 0..total_attempts {
        let s_count = success_count.clone();
        let lat_acc = total_latency_ms.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(i as u64 * 2)); // 极短间隔模拟突发

            let start = Instant::now();
            let cmd = "/system/bin/sh".to_string();
            let result = pty::create_subprocess_with_data(
                cmd,
                "/data/user/0/com.termux/files/home".to_string(),
                vec!["sh".to_string(), "-c".to_string(), "exit 0".to_string()],
                24,
                80,
                10,
                20,
                true,
            );

            let duration = start.elapsed().as_millis() as usize;
            lat_acc.fetch_add(duration, Ordering::SeqCst);

            match result {
                Ok((fd, pid)) => {
                    let status = pty::wait_for(pid);
                    unsafe {
                        libc::close(fd);
                    }
                    if status == 0 {
                        s_count.fetch_add(1, Ordering::SeqCst);
                    } else {
                        eprintln!("[TEST] Process {} failed (status {})", pid, status);
                    }
                }
                Err(_) => {
                    eprintln!("[TEST] Fork failed at attempt {}", i);
                }
            }
        });
        handles.push(handle);
    }

    // 3. 等待完成
    for h in handles {
        let _ = h.join();
    }
    running.store(false, Ordering::Relaxed);
    let total_duration = test_start.elapsed();

    let final_success = success_count.load(Ordering::SeqCst);
    let avg_latency = total_latency_ms.load(Ordering::SeqCst) as f64 / total_attempts as f64;

    println!("--------------------------------------------------");
    println!("System Limit (Phantom): 24");
    println!("Total Attempts: {}", total_attempts);
    println!("Successful Spawns: {}/{}", final_success, total_attempts);
    println!("Total Wall-Clock Time: {:?}", total_duration);
    println!("Average Latency per Process: {:.2} ms", avg_latency);
    println!("--------------------------------------------------");

    assert!(
        final_success > 50,
        "Success rate should be high despite system limits"
    );
}
