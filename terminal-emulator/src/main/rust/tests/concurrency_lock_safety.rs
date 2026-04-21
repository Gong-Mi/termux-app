use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

#[test]
fn test_render_lock_concurrency_non_blocking() {
    let state = Arc::new(RwLock::new(0));
    
    // 线程 A：模拟渲染线程，长时间持有读锁
    let state_a = state.clone();
    let handle_a = thread::spawn(move || {
        let _read_guard = state_a.read().unwrap();
        println!("Thread A: Holding read lock...");
        thread::sleep(Duration::from_millis(200));
        println!("Thread A: Releasing read lock.");
    });

    thread::sleep(Duration::from_millis(50));

    // 线程 B：模拟主线程，尝试非阻塞式地获取写锁
    let state_b = state.clone();
    let handle_b = thread::spawn(move || {
        println!("Thread B: Attempting try_write...");
        let start = std::time::Instant::now();
        let mut acquired = false;
        
        // 模拟我们的循环 try_lock 逻辑
        for _ in 0..10 {
            if let Ok(_write_guard) = state_b.try_write() {
                acquired = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        
        let elapsed = start.elapsed();
        println!("Thread B: Finished attempt in {:?}. Acquired: {}", elapsed, acquired);
        
        // 如果 A 还在持有锁，B 应该拿不到，但不能卡死 200ms
        assert!(elapsed < Duration::from_millis(150));
    });

    handle_a.join().unwrap();
    handle_b.join().unwrap();
}
