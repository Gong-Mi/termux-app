use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering, AtomicI32};
use std::thread;
use std::time::{Duration, Instant};
use std::os::unix::io::{FromRawFd};
use std::io::{Read};
use nix::unistd::pipe;
use std::os::fd::IntoRawFd;

/// 模拟 TerminalContext 结构体
struct MockTerminalContext {
    running: AtomicBool,
    pty_fd: AtomicI32,
}

impl MockTerminalContext {
    fn new() -> Self {
        Self {
            running: AtomicBool::new(true),
            pty_fd: AtomicI32::new(-1),
        }
    }
}

/// 模拟 IO 线程
fn start_mock_io_thread(context: Arc<MockTerminalContext>, read_fd: i32) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        // 模拟 std::fs::File::from_raw_fd(dup_fd)
        let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        
        println!("IO Thread: Started");
        while context.running.load(Ordering::SeqCst) {
            match file.read(&mut buffer) {
                Ok(0) => {
                    println!("IO Thread: EOF reached");
                    break;
                }
                Ok(n) => {
                    println!("IO Thread: Read {} bytes", n);
                }
                Err(_) => {
                    println!("IO Thread: Read error (likely closed)");
                    break;
                }
            }
        }
        println!("IO Thread: Exited");
    })
}

#[test]
fn test_phantom_thread_behavior() {
    // 1. 创建管道模拟 PTY
    let (read_pipe, write_pipe) = pipe().unwrap();
    // 使用 IntoRawFd 放弃所有权，避免 IO Safety 检查
    let read_raw_fd = read_pipe.into_raw_fd();
    let write_raw_fd = write_pipe.into_raw_fd();
    
    // 我们需要 dup 一下，因为 start_mock_io_thread 会获取所有权并关闭它
    let dup_read_fd = unsafe { libc::dup(read_raw_fd) };
    
    let context = Arc::new(MockTerminalContext::new());
    context.pty_fd.store(read_raw_fd, Ordering::SeqCst);
    
    // 2. 启动 IO 线程
    let handle = start_mock_io_thread(context.clone(), dup_read_fd);
    
    // 给线程一点时间启动并进入 read
    thread::sleep(Duration::from_millis(100));
    
    // 3. 模拟 destroyEngine: 设置 running = false
    println!("Main: Setting running = false (but NOT closing FD yet)");
    context.running.store(false, Ordering::SeqCst);
    
    // 4. 检查线程是否退出
    let start_wait = Instant::now();
    let mut exited = false;
    while start_wait.elapsed() < Duration::from_millis(500) {
        if handle.is_finished() {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    
    assert!(!exited, "IO 线程在 FD 未关闭时居然退出了！这不符合阻塞 read 的预期。");
    println!("Main: Verified - Thread is still alive (Phantom Thread confirmed)");
    
    // 5. 模拟 destroyEngine 的关键步骤：关闭 FD
    println!("Main: Now closing the PTY FD to kill the phantom thread...");
    let fd_to_close = context.pty_fd.swap(-1, Ordering::SeqCst);
    if fd_to_close != -1 {
        unsafe { libc::close(fd_to_close); }
    }
    
    // 6. 再次检查线程是否退出
    let start_wait = Instant::now();
    exited = false;
    while start_wait.elapsed() < Duration::from_secs(2) {
        if handle.is_finished() {
            exited = true;
            break;
        }
        
        // 关闭写入端，模拟所有 FD 被关闭，触发读端 EOF/Error
        unsafe { libc::close(write_raw_fd); }
        
        thread::sleep(Duration::from_millis(50));
    }
    
    assert!(exited, "IO 线程在 FD 关闭后仍未退出！");
    println!("Main: Success - Phantom thread eliminated.");
}

/// 测试 2: 确定渲染线程的 UAF 崩溃条件
#[test]
fn test_render_thread_uaf_condition() {
    let engine_ptr: AtomicI32 = AtomicI32::new(0);
    
    let context = Box::new(MockTerminalContext::new());
    let ptr = &*context as *const MockTerminalContext as i32;
    engine_ptr.store(ptr, Ordering::SeqCst);
    
    let current_ptr = engine_ptr.load(Ordering::SeqCst);
    assert_eq!(current_ptr, ptr);
    
    // 模拟销毁
    drop(context);
    
    // 崩溃条件：engine_ptr 仍为旧值
    assert_ne!(engine_ptr.load(Ordering::SeqCst), 0);
    println!("Main: UAF Condition confirmed: engine_ptr is dangling.");
}
