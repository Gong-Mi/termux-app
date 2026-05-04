use termux_rust::pty;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering, AtomicUsize};
use std::sync::Arc;
#[test]
fn test_extreme_concurrent_spawn_under_pressure() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let total_attempts = 10;

    println!("[TEST] Starting 10 concurrent subprocess spawns...");

    // 2. 并发启动 10 个子进程
    let mut handles = vec![];
    for i in 0..total_attempts {
        let s_count = success_count.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(i as u64 * 30));

            let cmd = "/system/bin/sh".to_string();
            let result = pty::create_subprocess_with_data(
                cmd,
                "/data/data/com.termux/files/home".to_string(),
                vec!["sh".to_string(), "-c".to_string(), "exit 0".to_string()],
                24, 80, 10, 20,
                true
            );

            match result {
                Ok((fd, pid)) => {
                    let status = pty::wait_for(pid);
                    unsafe { libc::close(fd); }
                    if status == 0 {
                        s_count.fetch_add(1, Ordering::SeqCst);
                    } else {
                        eprintln!("[TEST] Process {} exited with status {}", pid, status);
                    }
                }
                Err(_) => {
                    eprintln!("[TEST] Failed to fork at attempt {}", i);
                }
            }
        });
        handles.push(handle);
    }

    // 3. 等待完成
    for h in handles {
        let _ = h.join();
    }

    let final_success = success_count.load(Ordering::SeqCst);
    println!("--------------------------------------------------");
    println!("Concurrent Success Rate: {}/{}", final_success, total_attempts);
    
    assert!(final_success > 0, "No processes survived");
}
