
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicBool, Ordering};

// 模拟 VulkanContext 及其生命周期漏洞
struct MockVulkanContext {
    window_id: u32,
    is_valid: Arc<AtomicBool>,
}

impl MockVulkanContext {
    fn new(id: u32, is_valid: Arc<AtomicBool>) -> Self {
        println!("MockVulkanContext: Created for window {}", id);
        Self { window_id: id, is_valid }
    }

    // 模拟 queue_present 操作，模拟驱动层访问硬件句柄
    fn present(&self) {
        // 检查资源是否已被系统回收
        if !self.is_valid.load(Ordering::SeqCst) {
            // 在真实环境中，这里会是 SIGSEGV
            panic!("CRASH: SIGSEGV at fault addr 0x5b0! Accessing invalidated ANativeWindow handle for window {}", self.window_id);
        }
        
        // 模拟驱动内部的耗时提交
        thread::sleep(Duration::from_millis(10));
        
        // 再次检查（模拟在提交过程中资源被并发回收）
        if !self.is_valid.load(Ordering::SeqCst) {
            panic!("CRASH: SIGSEGV during queue_present! Surface was destroyed while GPU was active.");
        }
    }
}

impl Drop for MockVulkanContext {
    fn drop(&mut self) {
        println!("MockVulkanContext: Dropping resources for window {}", self.window_id);
        // 模拟释放顺序错误：如果先销毁了 Instance，再销毁 Swapchain 就会崩溃
        // 这里简化为标记失效
        self.is_valid.store(false, Ordering::SeqCst);
    }
}

#[test]
fn test_surface_destruction_race_condition() {
    let window_valid = Arc::new(AtomicBool::new(true));
    let context = Arc::new(Mutex::new(Some(MockVulkanContext::new(1001, window_valid.clone()))));
    let running = Arc::new(AtomicBool::new(true));

    // 1. 启动渲染线程
    let ctx_r = context.clone();
    let running_r = running.clone();
    let render_thread = thread::spawn(move || {
        println!("RenderThread: Started");
        let mut frames = 0;
        while running_r.load(Ordering::SeqCst) {
            // 模仿 render_thread.rs 的逻辑：尝试获取锁
            if let Ok(guard) = ctx_r.try_lock() {
                if let Some(ctx) = guard.as_ref() {
                    println!("RenderThread: Presenting frame {}...", frames);
                    // 模拟驱动正在工作时，锁被释放
                    // 在真实驱动中，即便 guard 还在，底层的 window 句柄也可能被系统标记无效
                    ctx.present();
                    frames += 1;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        println!("RenderThread: Exited cleanly after {} frames", frames);
    });

    // 让渲染跑一会儿
    thread::sleep(Duration::from_millis(15));

    // 2. 模拟 nativeSetSurface(null) -> surfaceDestroyed
    println!("\n[UI Thread] surfaceDestroyed() called!");
    
    // 关键复现场景：Android 系统在 nativeSetSurface 还没处理完时就标记 Surface 为无效
    // 或者 nativeSetSurface 内部的逻辑导致了竞争
    
    // 模拟异步销毁
    thread::spawn({
        let window_valid = window_valid.clone();
        move || {
            thread::sleep(Duration::from_millis(2)); // 在 present 期间销毁
            println!("\n[Android OS] !!! KILLING SURFACE MEMORY NOW !!!");
            window_valid.store(false, Ordering::SeqCst);
        }
    });

    running.store(false, Ordering::SeqCst);
    
    {
        println!("[UI Thread] Forcing context = None...");
        // 模拟因为 try_lock 导致的非阻塞竞争
        if let Ok(mut guard) = context.lock() {
            *guard = None; 
            println!("[UI Thread] Context cleared.");
        }
    }

    // 模拟 Android 系统回收 Surface 内存（异步或由于 surfaceDestroyed 返回）
    println!("[Android OS] Reclaiming Surface memory...");
    window_valid.store(false, Ordering::SeqCst);

    // 等待渲染线程结束（或者观察它崩溃）
    let start = Instant::now();
    let result = render_thread.join();
    
    match result {
        Ok(_) => println!("Test finished: Thread exited (unexpectedly successful)"),
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() { s.to_string() }
                      else if let Some(s) = e.downcast_ref::<String>() { s.clone() }
                      else { "Unknown panic".to_string() };
            println!("\nReproduced expected failure: {}", msg);
            assert!(msg.contains("CRASH"), "Panic should represent a simulated crash");
        }
    }
    println!("Wait time for join: {:?}", start.elapsed());
}
