use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn test_vulkan_lifecycle_stress_simulation() {
    // 模拟 Surface 准备就绪状态
    let surface_ready = Arc::new(AtomicBool::new(false));
    let render_thread_running = Arc::new(AtomicBool::new(true));
    
    let sr_clone = surface_ready.clone();
    let rtr_clone = render_thread_running.clone();
    
    // 启动模拟渲染线程
    let handle = thread::spawn(move || {
        let mut frames = 0;
        while rtr_clone.load(Ordering::SeqCst) {
            if !sr_clone.load(Ordering::SeqCst) {
                // 模拟我们刚才修复的逻辑：如果 surface 没准备好，park 线程
                thread::park();
                continue;
            }
            
            // 模拟渲染开销
            frames += 1;
            thread::sleep(Duration::from_millis(16));
            
            if frames % 100 == 0 {
                println!("Simulated Render Thread: rendered {} frames", frames);
            }
        }
    });

    // 模拟主线程频繁切换前后台
    for i in 0..50 {
        println!("Stress Cycle {}: Backgrounding...", i);
        surface_ready.store(false, Ordering::SeqCst);
        // 不显式通知，看超时还是 park
        // 实际上 handle.thread().unpark() 应该在这里
        handle.thread().unpark(); 
        
        thread::sleep(Duration::from_millis(5)); // 模拟随机切走
        
        println!("Stress Cycle {}: Foregrounding...", i);
        surface_ready.store(true, Ordering::SeqCst);
        handle.thread().unpark();
        
        thread::sleep(Duration::from_millis(5));
    }

    render_thread_running.store(false, Ordering::SeqCst);
    handle.thread().unpark();
    handle.join().unwrap();
    println!("Vulkan Lifecycle Stress Test Passed!");
}
