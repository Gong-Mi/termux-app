// Session 协调器集成测试（并行安全版）
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use termux_rust::coordinator::{SessionCoordinator, SessionState};

/// 测试场景 1: Pkg 锁互斥
#[test]
fn test_pkg_lock_mutual_exclusion() {
    // 使用独立的实例，避免并行测试干扰
    let coordinator = Arc::new(SessionCoordinator::new_coordinator());

    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();

    // Session 1 获取锁
    let result1 = coordinator.try_acquire_pkg_lock(session1);
    assert!(result1, "Session 1 should acquire lock");

    // Session 2 尝试获取锁
    let result2 = coordinator.try_acquire_pkg_lock(session2);
    assert!(!result2, "Session 2 should fail to acquire lock");

    assert!(coordinator.is_pkg_lock_held());
    assert_eq!(coordinator.get_pkg_lock_owner(), session1);
}

/// 测试场景 2: 锁状态查询
#[test]
fn test_lock_status_query() {
    let coordinator = Arc::new(SessionCoordinator::new_coordinator());
    assert!(!coordinator.is_pkg_lock_held());

    let session = coordinator.register_session();
    coordinator.try_acquire_pkg_lock(session);
    assert!(coordinator.is_pkg_lock_held());

    coordinator.release_pkg_lock(session);
    assert!(!coordinator.is_pkg_lock_held());
}

/// 测试场景 3: 并发锁竞争
#[test]
fn test_concurrent_lock_contention() {
    let coordinator = Arc::new(SessionCoordinator::new_coordinator());
    let success_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = vec![];

    for _ in 0..10 {
        let coord = coordinator.clone();
        let s_count = success_count.clone();
        let handle = thread::spawn(move || {
            let session = coord.register_session();
            if coord.try_acquire_pkg_lock(session) {
                s_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                thread::sleep(Duration::from_millis(10));
                coord.release_pkg_lock(session);
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }

    let final_success = success_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(final_success > 0 && final_success <= 10);
    println!("Lock contention test passed: {} success", final_success);
}

/// 测试场景 4: 注销时自动释放锁
#[test]
fn test_lock_release_on_unregister() {
    let coordinator = Arc::new(SessionCoordinator::new_coordinator());
    let session = coordinator.register_session();

    coordinator.try_acquire_pkg_lock(session);
    assert!(coordinator.is_pkg_lock_held());

    coordinator.unregister_session(session);
    assert!(
        !coordinator.is_pkg_lock_held(),
        "Lock should be released when owner unregisters"
    );
}

#[test]
fn test_session_states() {
    let coordinator = SessionCoordinator::new_coordinator();
    let id = coordinator.register_session();

    coordinator.update_session_state(id, SessionState::Busy);
    assert_eq!(coordinator.get_session_state(id), Some(SessionState::Busy));
}

#[test]
fn test_session_state_strings() {
    assert_eq!(SessionState::Idle.as_str(), "Idle");
    assert_eq!(SessionState::Running.as_str(), "Running");
    println!("test_session_state_strings: All state strings are correct!");
}
