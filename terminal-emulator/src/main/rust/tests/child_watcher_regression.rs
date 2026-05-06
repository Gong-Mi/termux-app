// child_watcher_regression.rs
// 验证 939c852a 中 ChildWatcher 引入的回归缺陷
//
// 运行: cargo test --test child_watcher_regression --features test-helpers -- --test-threads=1 --nocapture

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// 全局串行锁：child_watcher 测试共享全局 mutable 状态（WATCHER_STATE），
// reset_watcher_state() 会清空所有 PID 并关闭线程，必须串行执行。
// 代码层面已将 CHILD_WATCHER + WATCHER_THREAD 合并为单 Mutex，
// 但 reset_watcher_state 的全局语义决定了并发测试仍需互斥。
static TEST_LOCK: Mutex<()> = Mutex::new(());

// -------------------------------------------------------------------------
// 测试 1: JoinHandle 存活检测——线程死亡后应能重新创建
// -------------------------------------------------------------------------
#[test]
fn test_watcher_thread_restarted_after_handle_cleared() {
    let _guard = TEST_LOCK.lock().unwrap();
    termux_rust::pty::reset_watcher_state();
    let count_before = termux_rust::pty::watcher_thread_count();

    // 第一次启动 watcher
    let cb = Arc::new(|_: i32| {});
    termux_rust::pty::spawn_waiter(11111, cb);
    thread::sleep(Duration::from_millis(200));

    let count_after = termux_rust::pty::watcher_thread_count();
    assert_eq!(
        count_after, count_before + 1,
        "首次 spawn_waiter 应启动一个 watcher 线程"
    );

    // 模拟 watcher 线程"死亡后残留状态"：
    // 清空 CHILD_WATCHER 并将 JoinHandle 设为 None（这正是线程死亡后会出现的状态）
    termux_rust::pty::reset_watcher_state();

    let count_before2 = termux_rust::pty::watcher_thread_count();

    // 再次尝试注册新进程
    let cb2 = Arc::new(|_: i32| {});
    termux_rust::pty::spawn_waiter(22222, cb2);
    thread::sleep(Duration::from_millis(200));

    let count_after2 = termux_rust::pty::watcher_thread_count();

    // 关键断言：JoinHandle 为 None 时，ensure_watcher_thread 会重新创建线程
    assert_eq!(
        count_after2, count_before2 + 1,
        "JoinHandle 为空时应重新创建 watcher 线程。\
         旧 AtomicBool 实现下，flag 一旦为 true 就永远不会恢复。"
    );

    // 验证 entry 最终被处理（PID 不存在 -> ECHILD -> 移除 + 通知）
    thread::sleep(Duration::from_millis(800));
    assert_eq!(
        termux_rust::pty::watcher_map_len(),
        0,
        "不存在的 PID 应被 ECHILD 路径清理"
    );
}

// -------------------------------------------------------------------------
// 测试 2: waitpid 对已收割子进程返回 ECHILD，说明不检查 errno 的风险
// -------------------------------------------------------------------------
#[test]
fn test_waitpid_echild_semantics() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut child = Command::new("true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("无法启动子进程");
    let pid = child.id() as i32;
    child.wait().expect("wait 失败"); // 父进程先收割子进程

    let mut status = 0i32;
    let res = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    assert_eq!(
        res, -1,
        "对已收割子进程再次 waitpid 应返回 -1"
    );

    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    assert_eq!(
        errno, libc::ECHILD,
        "errno 应为 ECHILD"
    );
}

// -------------------------------------------------------------------------
// 测试 3: ECHILD 路径现在触发 callback（修复前不触发）
// -------------------------------------------------------------------------
#[test]
fn test_watcher_echild_notifies_callback() {
    let _guard = TEST_LOCK.lock().unwrap();
    termux_rust::pty::reset_watcher_state();

    let notified = Arc::new(AtomicUsize::new(0));
    let n = notified.clone();

    let cb = Arc::new(move |code: i32| {
        n.fetch_add(1, Ordering::SeqCst);
        println!("[test] callback invoked with exit_code={}", code);
    });

    // 使用一个绝对不会存在的 PID（确保 waitpid 返回 ECHILD）
    termux_rust::pty::spawn_waiter(999999, cb);
    thread::sleep(Duration::from_millis(800));

    // PID 不存在 -> waitpid 返回 -1 -> ECHILD -> 移除 + 通知
    assert_eq!(
        termux_rust::pty::watcher_map_len(),
        0,
        "不存在的 PID 应从 map 中移除"
    );

    let notify_count = notified.load(Ordering::SeqCst);
    // 修复后：ECHILD 路径会触发 callback，通知 Java 层进程已退出
    assert_eq!(
        notify_count, 1,
        "ECHILD 路径应触发 callback，让 Java 层感知进程已退出"
    );
}

// -------------------------------------------------------------------------
// 测试 4: 多个 spawn_waiter 不会启动多个 watcher 线程
// -------------------------------------------------------------------------
#[test]
fn test_single_watcher_thread_for_multiple_sessions() {
    let _guard = TEST_LOCK.lock().unwrap();
    termux_rust::pty::reset_watcher_state();
    let count_before = termux_rust::pty::watcher_thread_count();

    let counter = Arc::new(AtomicUsize::new(0));

    for i in 0..5 {
        let c = counter.clone();
        let cb = Arc::new(move |code: i32| {
            c.fetch_add(1, Ordering::SeqCst);
            println!("[test] session {} exited with code {}", i, code);
        });
        termux_rust::pty::spawn_waiter(90000 + i, cb);
    }

    // spawn_waiter 是同步插入，立即检查
    assert_eq!(
        termux_rust::pty::watcher_map_len(),
        5,
        "5 个 PID 都应在 map 中"
    );

    thread::sleep(Duration::from_millis(200));

    // 只应启动了一个 watcher 线程
    let count_after = termux_rust::pty::watcher_thread_count();
    assert_eq!(
        count_after, count_before + 1,
        "5 次 spawn_waiter 只应启动 1 个 watcher 线程"
    );

    // 等待 watcher 线程处理（PID 不存在，都会被 ECHILD 移除并通知）
    thread::sleep(Duration::from_millis(1200));

    assert_eq!(
        termux_rust::pty::watcher_map_len(),
        0,
        "所有不存在的 PID 应被清理"
    );

    // 修复后：每个 ECHILD 都会通知 callback
    let count = counter.load(Ordering::SeqCst);
    assert_eq!(
        count, 5,
        "5 个 session 退出各应触发一次 callback"
    );
}
