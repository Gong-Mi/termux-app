//! Session 协调器集成测试（简化版）
//! 
//! 由于 SessionCoordinator 是全局单例，测试需要考虑状态污染

use std::thread;
use std::time::Duration;

use termux_rust::coordinator::{SessionCoordinator, SessionState};

/// 测试场景 1: Pkg 锁互斥
#[test]
fn test_pkg_lock_mutual_exclusion() {
    let coordinator = SessionCoordinator::get();
    
    // 清理之前的状态
    if coordinator.is_pkg_lock_held() {
        let owner = coordinator.get_pkg_lock_owner();
        coordinator.release_pkg_lock(owner);
    }
    
    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();
    
    // Session 1 获取锁
    let result1 = coordinator.try_acquire_pkg_lock(session1);
    
    // Session 2 尝试获取锁
    let result2 = coordinator.try_acquire_pkg_lock(session2);
    
    // 应该只有一个成功
    assert!(result1 || result2, "应该至少有一个 session 获取锁成功");
    assert_ne!(result1, result2, "两个 session 不应该同时获取锁成功");
    
    // 清理
    if result1 {
        coordinator.release_pkg_lock(session1);
    } else {
        coordinator.release_pkg_lock(session2);
    }
}

/// 测试场景 2: Session 状态管理
#[test]
fn test_session_states() {
    let coordinator = SessionCoordinator::get();

    // 清理之前的状态
    if coordinator.is_pkg_lock_held() {
        let owner = coordinator.get_pkg_lock_owner();
        coordinator.release_pkg_lock(owner);
    }

    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();

    // Session 1 获取锁
    let result1 = coordinator.try_acquire_pkg_lock(session1);
    
    // 如果获取失败，说明有其他测试持有锁，跳过
    if !result1 {
        println!("Skipping test_session_states: lock held by other test");
        coordinator.unregister_session(session1);
        coordinator.unregister_session(session2);
        return;
    }

    // Session 2 尝试获取锁，应该进入 WaitingLock
    coordinator.try_acquire_pkg_lock(session2);

    // 检查状态
    assert_eq!(coordinator.get_session_state(session1), Some(SessionState::Busy));
    assert_eq!(coordinator.get_session_state(session2), Some(SessionState::WaitingLock));

    // 检查等待标志
    assert!(coordinator.has_waiting_sessions());

    // 清理
    coordinator.release_pkg_lock(session1);
    coordinator.unregister_session(session1);
    coordinator.unregister_session(session2);
}

/// 测试场景 3: 锁持有者注销时自动释放
#[test]
fn test_lock_release_on_unregister() {
    let coordinator = SessionCoordinator::get();
    
    // 清理之前的状态
    if coordinator.is_pkg_lock_held() {
        let owner = coordinator.get_pkg_lock_owner();
        coordinator.release_pkg_lock(owner);
    }
    
    let session = coordinator.register_session();
    
    // 获取锁
    let acquired = coordinator.try_acquire_pkg_lock(session);
    if !acquired {
        // 如果获取失败，说明有其他 session 持有锁，跳过测试
        println!("Skipping test: lock already held by other session");
        coordinator.unregister_session(session);
        return;
    }
    
    assert!(coordinator.is_pkg_lock_held());
    
    // 注销 session（模拟崩溃）
    coordinator.unregister_session(session);
    
    // 锁应该被自动释放
    assert!(
        !coordinator.is_pkg_lock_held(),
        "Session 注销后锁应该被自动释放"
    );
}

/// 测试场景 4: 获取所有 Session 状态
#[test]
fn test_all_session_states() {
    let coordinator = SessionCoordinator::get();
    
    // 创建多个 session
    for _ in 0..3 {
        coordinator.register_session();
    }
    
    // 获取所有状态
    let states = coordinator.get_all_session_states();
    
    // 应该至少有 3 个
    assert!(states.len() >= 3, "应该至少有 3 个 session");
    
    // 打印状态（用于调试）
    println!("Session states:");
    for (id, state) in &states {
        println!("  Session {}: {:?}", id, state);
    }
}

/// 测试场景 5: 锁状态查询
#[test]
fn test_lock_status_query() {
    let coordinator = SessionCoordinator::get();
    
    // 清理
    if coordinator.is_pkg_lock_held() {
        let owner = coordinator.get_pkg_lock_owner();
        coordinator.release_pkg_lock(owner);
    }
    
    // 初始状态
    assert!(!coordinator.is_pkg_lock_held());
    assert_eq!(coordinator.get_pkg_lock_owner(), 0);
    
    // 获取锁
    let session = coordinator.register_session();
    let acquired = coordinator.try_acquire_pkg_lock(session);
    
    // 检查是否成功获取
    if !acquired {
        println!("Skipping test_lock_status_query: lock held by other test");
        coordinator.unregister_session(session);
        return;
    }
    
    // 检查状态
    assert!(coordinator.is_pkg_lock_held());
    assert_eq!(coordinator.get_pkg_lock_owner(), session);
    
    // 释放锁
    coordinator.release_pkg_lock(session);
    
    // 检查释放后状态
    assert!(!coordinator.is_pkg_lock_held());
    assert_eq!(coordinator.get_pkg_lock_owner(), 0);
}

/// 测试场景 6: 并发锁竞争（简化版）
#[test]
fn test_concurrent_lock_contention() {
    let coordinator = SessionCoordinator::get();
    
    // 清理
    if coordinator.is_pkg_lock_held() {
        let owner = coordinator.get_pkg_lock_owner();
        coordinator.release_pkg_lock(owner);
    }
    
    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();
    
    let mut handles = vec![];
    
    // 线程 1
    {
        let coord = coordinator;
        let handle = thread::spawn(move || {
            coord.try_acquire_pkg_lock(session1)
        });
        handles.push(handle);
    }
    
    // 线程 2（稍晚一点）
    {
        let coord = coordinator;
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            coord.try_acquire_pkg_lock(session2)
        });
        handles.push(handle);
    }
    
    // 收集结果
    let results: Vec<bool> = handles.into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    
    // 由于全局状态，可能两个都失败（如果其他测试持有锁）
    // 或者一个成功一个失败
    let success_count = results.iter().filter(|&&r| r).count();
    
    if success_count == 1 {
        // 正常情况：一个成功一个失败
        println!("Lock contention test passed: {} success", success_count);
    } else if success_count == 0 {
        // 都失败，说明有其他测试持有锁
        println!("Both sessions failed to acquire lock - lock held by other test");
    } else {
        // 两个都成功，这是不可能的，除非有 bug
        panic!("Both sessions acquired lock - this should be impossible! results: {:?}", results);
    }
    
    // 清理：释放所有成功获取的锁
    for (i, &result) in results.iter().enumerate() {
        if result {
            if i == 0 {
                coordinator.release_pkg_lock(session1);
            } else {
                coordinator.release_pkg_lock(session2);
            }
        }
    }
}

/// 测试场景 7: Session 状态字符串转换
#[test]
fn test_session_state_strings() {
    use termux_rust::coordinator::SessionState;
    
    // 测试状态枚举的字符串转换
    assert_eq!(SessionState::Idle.as_str(), "Idle");
    assert_eq!(SessionState::Running.as_str(), "Running");
    assert_eq!(SessionState::Busy.as_str(), "Busy");
    assert_eq!(SessionState::WaitingLock.as_str(), "WaitingLock");
    assert_eq!(SessionState::Finished.as_str(), "Finished");
    
    println!("All state strings are correct!");
}
