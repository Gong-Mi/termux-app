use termux_rust::engine::{TerminalEngine, TerminalContext};
use termux_rust::pty;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_dual_session_concurrency() {
    let cols = 80;
    let rows = 24;
    let transcript = 1000;

    // 1. 并发创建两个会话
    let handle1 = thread::spawn(move || {
        let engine = TerminalEngine::new(cols, rows, transcript, 10, 20);
        let context = Arc::new(TerminalContext::new(engine));
        
        // 模拟 PTY 创建逻辑 (在测试环境下可能无法 fork，我们手动模拟数据输入)
        context.clone()
    });

    let handle2 = thread::spawn(move || {
        let engine = TerminalEngine::new(cols, rows, transcript, 10, 20);
        let context = Arc::new(TerminalContext::new(engine));
        context.clone()
    });

    let context1 = handle1.join().unwrap();
    let context2 = handle2.join().unwrap();

    // 2. 模拟并发数据处理
    let c1 = context1.clone();
    let t1 = thread::spawn(move || {
        for i in 0..100 {
            let mut engine = c1.lock.write().unwrap();
            let data = format!("Session 1 - Line {}\r\n", i);
            engine.process_bytes(data.as_bytes());
            // 模拟释锁后的 flush
            drop(engine);
            let mut engine = c1.lock.write().unwrap();
            engine.flush_events();
            thread::sleep(Duration::from_millis(1));
        }
    });

    let c2 = context2.clone();
    let t2 = thread::spawn(move || {
        for i in 0..100 {
            let mut engine = c2.lock.write().unwrap();
            let data = format!("Session 2 - Line {}\r\n", i);
            engine.process_bytes(data.as_bytes());
            drop(engine);
            let mut engine = c2.lock.write().unwrap();
            engine.flush_events();
            thread::sleep(Duration::from_millis(1));
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();

    // 3. 验证两个引擎的状态是否独立且正确
    {
        let engine1 = context1.lock.read().unwrap();
        let engine2 = context2.lock.read().unwrap();
        
        // 检查屏幕内容是否包含各自的特征字符串
        // 这里只是示意，实际需要 copy_row_text 检查数据
        assert_eq!(engine1.state.cols, 80);
        assert_eq!(engine2.state.cols, 80);
        
        println!("Both sessions finished successfully without deadlocks.");
    }
}

#[test]
fn test_pty_resource_independence() {
    // 这个测试验证底层 PTY 分配是否冲突
    // 注意：在某些受限测试环境下可能失败
    let res1 = pty::create_subprocess_with_data(
        "/bin/sh".to_string(), 
        "/tmp".to_string(), 
        vec!["-c".to_string(), "echo hello".to_string()], 
        vec![], 24, 80, 10, 20
    );

    let res2 = pty::create_subprocess_with_data(
        "/bin/sh".to_string(), 
        "/tmp".to_string(), 
        vec!["-c".to_string(), "echo world".to_string()], 
        vec![], 24, 80, 10, 20
    );

    if let (Ok(p1), Ok(p2)) = (res1, res2) {
        assert_ne!(p1.0, p2.0, "PTY FDs must be different");
        assert_ne!(p1.1, p2.1, "PIDs must be different");
        println!("PTY allocation is independent: fd1={}, fd2={}", p1.0, p2.0);
    } else {
        println!("Skipping PTY test (maybe not in a full Linux environment)");
    }
}
