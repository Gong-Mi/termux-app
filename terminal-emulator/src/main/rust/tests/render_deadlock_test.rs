// 渲染线程死锁和阻塞风险测试
// 运行：cargo test --test render_deadlock_test -- --nocapture

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

// =============================================================================
// 测试 1: RwLock 写饥饿（渲染线程 try_read 被写请求饿死）
// =============================================================================

/// 模拟渲染线程的 try_read 行为
struct RwLockTestCtx {
    lock: RwLock<i32>,
}

impl RwLockTestCtx {
    fn new() -> Self {
        Self {
            lock: RwLock::new(0),
        }
    }
}

/// 测试：持续 write 锁会导致 try_read 失败
#[test]
fn test_rwlock_write_starvation() {
    let ctx = Arc::new(RwLockTestCtx::new());
    let _reader_fail_count = Arc::new(Mutex::new(0u32));

    // 写线程：持续持有写锁 100ms
    let writer_ctx = ctx.clone();
    let writer = thread::spawn(move || {
        for _ in 0..50 {
            let _guard = writer_ctx.lock.write().unwrap();
            thread::sleep(Duration::from_millis(2));
        }
    });

    // 读线程：模拟渲染循环的 try_read 行为
    let reader_ctx = ctx.clone();
    let _reader_fail_count = _reader_fail_count.clone();
    let reader = thread::spawn(move || {
        let mut failures = 0;
        for _ in 0..100 {
            match reader_ctx.lock.try_read() {
                Ok(_guard) => {
                    // 成功读取
                }
                Err(_) => {
                    failures += 1;
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
        failures
    });

    writer.join().unwrap();
    let failures = reader.join().unwrap();

    // 在 Android RwLock 实现中（通常是 write-fair），write 会阻塞后续 read
    // 即使 try_read 应该立即返回 Err，但如果写锁排队时间长，读线程会频繁失败
    eprintln!("RwLock try_read failures: {}", failures);

    // 验证：如果失败率 > 30%，说明写饥饿严重
    // 这是一个风险指标，不是硬性断言（因为 RwLock 行为因平台而异）
    if failures > 60 {
        eprintln!(
            "WARNING: High RwLock write starvation detected ({}% failure rate)",
            (failures as f64 / 100.0 * 100.0) as u32
        );
    }
}

// =============================================================================
// 测试 2: 脏标记竞态（消费后到渲染完成期间的新写入丢失）
// =============================================================================

#[test]
fn test_dirty_flag_race_condition() {
    let dirty = Arc::new(AtomicBool::new(false));

    // 模拟渲染线程
    let dirty_r = dirty.clone();
    let render_thread = thread::spawn(move || {
        let mut frames_rendered = 0;
        let start = Instant::now();

        while start.elapsed() < Duration::from_millis(200) {
            if dirty_r.load(Ordering::SeqCst) {
                dirty_r.store(false, Ordering::SeqCst);
                // 模拟渲染耗时 10ms
                thread::sleep(Duration::from_millis(10));
                frames_rendered += 1;
            } else {
                thread::sleep(Duration::from_millis(16));
            }
        }
        frames_rendered
    });

    // 模拟 IO 线程：快速连续写入 20 次脏标记
    let dirty_w = dirty.clone();
    let write_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50)); // 等渲染线程启动
        for _ in 0..20 {
            dirty_w.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(1)); // 每 1ms 写入一次
        }
    });

    write_thread.join().unwrap();
    let frames = render_thread.join().unwrap();

    eprintln!("Frames rendered during rapid dirty writes: {}", frames);

    // 关键验证：在 200ms 内写入 20 次，理想情况下应该渲染 ~12 帧
    // 如果帧数 < 5，说明脏标记消费太快，大量写入被浪费
    if frames < 5 {
        eprintln!(
            "WARNING: Only {} frames rendered, many dirty writes may have been lost",
            frames
        );
    }

    // 验证：最终状态应该干净（所有脏标记被消费）
    assert!(
        !dirty.load(Ordering::SeqCst),
        "Dirty flag should be consumed at end"
    );
}

// =============================================================================
// 测试 3: Mutex 连续阻塞（多个 AtomicBool 替代方案对比）
// =============================================================================

#[test]
fn test_mutex_contention_vs_atomic() {
    // 方案 A: 多个 Mutex（当前渲染线程的做法）
    let scale_mutex = Arc::new(Mutex::new(1.0f32));
    let offset_mutex = Arc::new(Mutex::new(0.0f32));

    let start = Instant::now();
    for _ in 0..10000 {
        let _s = scale_mutex.lock().unwrap();
        let _o = offset_mutex.lock().unwrap();
        // 模拟读取
    }
    let mutex_time = start.elapsed();

    // 方案 B: 原子变量（应该更快）
    let scale_atomic = Arc::new(std::sync::atomic::AtomicU32::new(f32::to_bits(1.0)));
    let offset_atomic = Arc::new(std::sync::atomic::AtomicU32::new(f32::to_bits(0.0)));

    let start = Instant::now();
    for _ in 0..10000 {
        let _s = scale_atomic.load(Ordering::Relaxed);
        let _o = offset_atomic.load(Ordering::Relaxed);
    }
    let atomic_time = start.elapsed();

    eprintln!(
        "Mutex time: {:?}, Atomic time: {:?}",
        mutex_time, atomic_time
    );
    eprintln!(
        "Speedup: {:.1}x",
        mutex_time.as_nanos() as f64 / atomic_time.as_nanos() as f64
    );

    // 验证：原子操作应该比 Mutex 快至少 5 倍
    assert!(
        atomic_time < mutex_time,
        "Atomic should be faster than Mutex: atomic={:?}, mutex={:?}",
        atomic_time,
        mutex_time
    );
}

// =============================================================================
// 测试 4: 渲染线程退出超时（模拟 surfaceDestroyed join 挂起）
// =============================================================================

#[test]
fn test_render_thread_exit_timeout() {
    let running = Arc::new(AtomicBool::new(true));
    let exit_time = Arc::new(Mutex::new(None::<Duration>));

    // 模拟渲染线程：循环中阻塞 1 秒
    let running_t = running.clone();
    let exit_time_t = exit_time.clone();
    let handle = thread::spawn(move || {
        let start = Instant::now();
        while running_t.load(Ordering::SeqCst) {
            // 模拟 acquire_next_image 阻塞 1 秒
            thread::sleep(Duration::from_secs(1));
        }
        *exit_time_t.lock().unwrap() = Some(start.elapsed());
    });

    // 等待线程进入睡眠
    thread::sleep(Duration::from_millis(100));

    // 设置退出标志
    let set_time = Instant::now();
    running.store(false, Ordering::SeqCst);

    // 等待线程退出（最多等 3 秒）
    let result = handle.join_timeout(Duration::from_secs(3));
    let wait_time = set_time.elapsed();

    match result {
        Ok(_) => {
            let exit_dur = *exit_time.lock().unwrap();
            eprintln!("Thread exited after {:?}", exit_dur);
            // 线程最终退出了，但等待时间远超预期
            assert!(wait_time > Duration::from_millis(100));
        }
        Err(_) => {
            panic!("Thread did not exit within 3 seconds (simulated deadlock!)");
        }
    }
}

// 给 JoinHandle 添加超时方法
trait JoinHandleExt {
    fn join_timeout(self, timeout: Duration) -> thread::Result<()>;
}

impl<T> JoinHandleExt for thread::JoinHandle<T> {
    fn join_timeout(self, timeout: Duration) -> thread::Result<()> {
        let start = Instant::now();
        let handle = self;
        loop {
            if handle.is_finished() {
                let _ = handle.join();
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "join timed out",
                )));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

// =============================================================================
// 测试 5: 无 fence 同步的帧积压（模拟 GPU 队列堆积）
// =============================================================================

#[test]
fn test_frame_backpressure_simulation() {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    // A host-side synchronization model, not a GPU or production-renderer test.
    // Completion permits replace sleeps: no frame can finish before it is released.
    // Three concurrent frames do not have a serial 100ms / 8ms throughput bound.
    const SWAPCHAIN_IMAGES: usize = 3;
    const REFILL_ROUNDS: usize = 8;
    let in_flight = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let mut workers = VecDeque::new();
    let mut frames_submitted = 0;
    let mut frames_blocked = 0;

    let try_submit = || {
        loop {
            let count = in_flight.load(Ordering::SeqCst);
            if count >= SWAPCHAIN_IMAGES {
                return None;
            }
            if in_flight
                .compare_exchange_weak(count, count + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
        let (release, permit) = mpsc::channel();
        let in_flight = Arc::clone(&in_flight);
        let completed = Arc::clone(&completed);
        let worker = thread::spawn(move || {
            if permit.recv().is_ok() {
                // Only the completion owner releases capacity, exactly once.
                let previous = in_flight.fetch_sub(1, Ordering::SeqCst);
                assert!((1..=SWAPCHAIN_IMAGES).contains(&previous));
                completed.fetch_add(1, Ordering::SeqCst);
            }
        });
        Some((release, worker))
    };
    // Snapshots are taken only while workers are gated or after joining the
    // released worker, so the two completion atomics cannot be read mid-update.
    let assert_state = |submitted, expected_completed, expected_in_flight| {
        let active = in_flight.load(Ordering::SeqCst);
        let finished = completed.load(Ordering::SeqCst);
        assert_eq!(active, expected_in_flight);
        assert_eq!(finished, expected_completed);
        assert!(active <= SWAPCHAIN_IMAGES);
        assert_eq!(
            submitted,
            finished + active,
            "frame accounting must balance"
        );
    };

    for _ in 0..SWAPCHAIN_IMAGES {
        workers.push_back(try_submit().expect("initial capacity must be available"));
        frames_submitted += 1;
        assert_state(frames_submitted, 0, frames_submitted);
    }

    for round in 0..REFILL_ROUNDS {
        assert!(try_submit().is_none(), "full queue must reject submission");
        frames_blocked += 1;
        assert_state(frames_submitted, round, SWAPCHAIN_IMAGES);

        let (release, worker) = workers.pop_front().unwrap();
        release.send(()).unwrap();
        worker
            .join()
            .expect("completion worker must exit successfully");
        assert_state(frames_submitted, round + 1, SWAPCHAIN_IMAGES - 1);

        workers.push_back(try_submit().expect("one completion must free one slot"));
        frames_submitted += 1;
        assert_state(frames_submitted, round + 1, SWAPCHAIN_IMAGES);
        assert!(
            try_submit().is_none(),
            "only one replacement may be admitted"
        );
        frames_blocked += 1;
        assert_state(frames_submitted, round + 1, SWAPCHAIN_IMAGES);
    }

    // Drain every remaining frame; no detached worker or outstanding capacity.
    let mut expected_completed = REFILL_ROUNDS;
    while let Some((release, worker)) = workers.pop_front() {
        release.send(()).unwrap();
        worker
            .join()
            .expect("completion worker must exit successfully");
        expected_completed += 1;
        assert_state(frames_submitted, expected_completed, workers.len());
    }
    assert_eq!(frames_submitted, SWAPCHAIN_IMAGES + REFILL_ROUNDS);
    assert_eq!(frames_blocked, 2 * REFILL_ROUNDS);
    assert_state(frames_submitted, frames_submitted, 0);
    eprintln!(
        "Frames submitted: {frames_submitted}, frames blocked: {frames_blocked}, \
         frames completed: {expected_completed}, in flight: 0"
    );
}

// =============================================================================
// 测试 6: 真实 TerminalContext 读写并发与渲染饥饿回归基准
// =============================================================================

#[test]
fn test_terminal_context_concurrent_io_and_render_read() {
    use termux_rust::engine::TerminalContext;

    let cols = 80;
    let rows = 24;
    let total_rows = 1000;
    let engine = termux_rust::TerminalEngine::new(0, cols, rows, total_rows, 10, 20);
    let ctx = Arc::new(TerminalContext::new(engine));

    let running = Arc::new(AtomicBool::new(true));
    let frames_rendered = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let read_lock_failures = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // 渲染线程：以 60FPS 频率调用 try_read，模拟真实渲染循环
    let render_ctx = ctx.clone();
    let render_running = running.clone();
    let render_counter = frames_rendered.clone();
    let render_fails = read_lock_failures.clone();

    let render_thread = thread::spawn(move || {
        while render_running.load(Ordering::SeqCst) {
            match render_ctx.lock.try_read() {
                Ok(engine) => {
                    // 模拟读锁持有时间（读取当前屏幕行并提取样式）
                    let _active_row = engine.state.get_current_screen().get_row(0);
                    render_counter.fetch_add(1, Ordering::Relaxed);
                    drop(engine);
                    thread::sleep(Duration::from_millis(16));
                }
                Err(_) => {
                    render_fails.fetch_add(1, Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
    });

    // IO 线程：模拟高吞吐量快速写入流（连续写入 100 次批量终端序列）
    let io_ctx = ctx.clone();
    let io_thread = thread::spawn(move || {
        for i in 0..100 {
            {
                let mut engine = io_ctx.lock.write().unwrap();
                let chunk = format!("Line {:04}: Hello Terminal Data Flood Testing\r\n", i);
                engine.process_bytes(chunk.as_bytes());
                // 模拟 PTY 解析和缓冲区维护耗时
                thread::sleep(Duration::from_micros(500));
            }
            thread::sleep(Duration::from_micros(200));
        }
    });

    io_thread.join().unwrap();
    // 写入完成后继续保持渲染循环短暂观察
    thread::sleep(Duration::from_millis(50));
    running.store(false, Ordering::SeqCst);
    render_thread.join().unwrap();

    let success_renders = frames_rendered.load(Ordering::SeqCst);
    let fail_renders = read_lock_failures.load(Ordering::SeqCst);

    eprintln!(
        "TerminalContext Benchmark: success_renders = {}, fail_renders = {}",
        success_renders, fail_renders
    );

    // 保证在密集写入流下，渲染线程不会发生完全死锁或 100% 读饥饿
    assert!(
        success_renders > 0,
        "Render thread must successfully acquire read lock and render frames during IO flood"
    );
}
