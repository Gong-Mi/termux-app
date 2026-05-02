//! Session 协调器单元测试

use termux_rust::coordinator::{SessionCoordinator, SessionState};

#[test]
fn test_coordinator_initialization() {
    // 测试协调器可以正确初始化
    let coordinator = SessionCoordinator::get();
    
    // 初始状态检查
    assert!(!coordinator.is_pkg_lock_held(), "初始状态 pkg 锁应该是未锁定");
    assert_eq!(coordinator.get_pkg_lock_owner(), 0, "初始状态锁所有者应该是 0");
}

#[test]
fn test_session_registration() {
    let coordinator = SessionCoordinator::get();
    
    // 注册 session（ID 可能不是从 0 开始，因为其他测试也注册了）
    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();
    let session3 = coordinator.register_session();
    
    // 检查 ID 是递增的
    assert!(session2 > session1, "session ID 应该递增");
    assert!(session3 > session2, "session ID 应该递增");
    
    // 检查状态
    assert_eq!(
        coordinator.get_session_state(session1),
        Some(SessionState::Idle),
        "Session 1 应该是 Idle 状态"
    );
    assert_eq!(
        coordinator.get_session_state(session2),
        Some(SessionState::Idle),
        "Session 2 应该是 Idle 状态"
    );
}

#[test]
fn test_session_unregister() {
    let coordinator = SessionCoordinator::get();
    
    // 注册并注销
    let session = coordinator.register_session();
    coordinator.unregister_session(session);
    
    // 注销后状态应该是 Finished（或者 None，如果状态表清理了）
    let state = coordinator.get_session_state(session);
    assert!(
        state == Some(SessionState::Finished) || state.is_none(),
        "注销后状态应该是 Finished 或 None"
    );
}

#[test]
fn test_pkg_lock_acquire_release() {
    let coordinator = SessionCoordinator::get();
    
    let session1 = coordinator.register_session();
    
    // 第一次获取锁应该成功
    assert!(
        coordinator.try_acquire_pkg_lock(session1),
        "第一次获取锁应该成功"
    );
    assert!(
        coordinator.is_pkg_lock_held(),
        "锁应该被标记为已持有"
    );
    assert_eq!(
        coordinator.get_pkg_lock_owner(),
        session1,
        "锁所有者应该是 session1"
    );
    
    // 释放锁
    coordinator.release_pkg_lock(session1);
    assert!(
        !coordinator.is_pkg_lock_held(),
        "释放后锁应该是未锁定"
    );
    assert_eq!(
        coordinator.get_pkg_lock_owner(),
        0,
        "释放后锁所有者应该是 0"
    );
}

#[test]
fn test_pkg_lock_contention() {
    let coordinator = SessionCoordinator::get();
    
    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();
    
    // Session 1 获取锁
    assert!(
        coordinator.try_acquire_pkg_lock(session1),
        "Session 1 获取锁应该成功"
    );
    
    // Session 2 尝试获取锁应该失败
    assert!(
        !coordinator.try_acquire_pkg_lock(session2),
        "Session 2 获取锁应该失败"
    );
    
    // 检查 Session 2 的状态应该是 WaitingLock
    assert_eq!(
        coordinator.get_session_state(session2),
        Some(SessionState::WaitingLock),
        "Session 2 应该是 WaitingLock 状态"
    );
    
    // Session 1 释放锁
    coordinator.release_pkg_lock(session1);
    
    // 现在 Session 2 应该可以获取锁
    assert!(
        coordinator.try_acquire_pkg_lock(session2),
        "Session 2 现在应该能获取锁"
    );
    
    coordinator.release_pkg_lock(session2);
}

#[test]
fn test_pkg_lock_state_transitions() {
    let coordinator = SessionCoordinator::get();
    
    let session = coordinator.register_session();
    
    // 初始状态：Idle 或 Running（取决于之前的测试）
    let initial_state = coordinator.get_session_state(session);
    assert!(
        initial_state == Some(SessionState::Idle) || initial_state == Some(SessionState::Running),
        "初始状态应该是 Idle 或 Running"
    );
    
    // 获取 pkg 锁后：Busy
    coordinator.try_acquire_pkg_lock(session);
    assert_eq!(
        coordinator.get_session_state(session),
        Some(SessionState::Busy)
    );
    
    // 释放锁后：Running
    coordinator.release_pkg_lock(session);
    assert_eq!(
        coordinator.get_session_state(session),
        Some(SessionState::Running)
    );
    
    // 注销后：Finished 或 None
    coordinator.unregister_session(session);
    let final_state = coordinator.get_session_state(session);
    assert!(
        final_state == Some(SessionState::Finished) || final_state.is_none(),
        "注销后状态应该是 Finished 或 None"
    );
}

#[test]
fn test_waiting_lock_state() {
    let coordinator = SessionCoordinator::get();
    
    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();
    
    // Session 1 持有锁
    coordinator.try_acquire_pkg_lock(session1);
    
    // Session 2 尝试获取锁，应该进入 WaitingLock 状态
    coordinator.try_acquire_pkg_lock(session2);
    assert_eq!(
        coordinator.get_session_state(session2),
        Some(SessionState::WaitingLock)
    );
    
    // 检查是否有等待的 session
    assert!(
        coordinator.has_waiting_sessions(),
        "应该检测到有 session 在等待锁"
    );
    
    // Session 1 释放锁
    coordinator.release_pkg_lock(session1);
    
    // Session 2 仍然在 WaitingLock 状态（需要主动再次尝试获取）
    assert_eq!(
        coordinator.get_session_state(session2),
        Some(SessionState::WaitingLock)
    );
    
    coordinator.unregister_session(session1);
    coordinator.unregister_session(session2);
}

#[test]
fn test_multiple_sessions_cleanup() {
    let coordinator = SessionCoordinator::get();
    
    // 创建多个 session
    let session1 = coordinator.register_session();
    let session2 = coordinator.register_session();
    let session3 = coordinator.register_session();
    
    // Session 1 持有锁
    coordinator.try_acquire_pkg_lock(session1);
    
    // 注销持有锁的 session 应该自动释放锁
    coordinator.unregister_session(session1);
    
    // 锁应该被释放
    assert!(
        !coordinator.is_pkg_lock_held(),
        "注销持有锁的 session 后锁应该被释放"
    );
    
    // 现在 Session 2 可以获取锁
    assert!(
        coordinator.try_acquire_pkg_lock(session2),
        "Session 2 现在应该能获取锁"
    );
    
    // 清理
    coordinator.unregister_session(session2);
    coordinator.unregister_session(session3);
}

#[test]
fn test_get_all_session_states() {
    let coordinator = SessionCoordinator::get();
    
    // 创建多个 session
    let _session1 = coordinator.register_session();
    let _session2 = coordinator.register_session();
    let _session3 = coordinator.register_session();
    
    // 获取所有状态
    let states = coordinator.get_all_session_states();
    
    // 应该有 3 个 session
    assert!(
        states.len() >= 3,
        "应该至少有 3 个 session 状态"
    );
    
    // 打印状态（用于调试）
    println!("All session states:");
    for (id, state) in &states {
        println!("  Session {}: {:?}", id, state);
    }
}

#[test]
fn test_session_state_as_str() {
    // 测试状态枚举的字符串转换
    assert_eq!(SessionState::Idle.as_str(), "Idle");
    assert_eq!(SessionState::Running.as_str(), "Running");
    assert_eq!(SessionState::Busy.as_str(), "Busy");
    assert_eq!(SessionState::WaitingLock.as_str(), "WaitingLock");
    assert_eq!(SessionState::Finished.as_str(), "Finished");
}

#[test]
fn test_double_register_cleanup() {
    let coordinator = SessionCoordinator::get();
    
    let session = coordinator.register_session();
    
    // 手动设置为 Busy
    coordinator.try_acquire_pkg_lock(session);
    
    // 注销持有锁的 session 应该自动释放锁
    coordinator.unregister_session(session);
    
    // 锁应该被释放
    assert!(
        !coordinator.is_pkg_lock_held(),
        "注销持有锁的 session 后锁应该被释放"
    );
    
    // 再次注销应该不会导致问题（幂等操作）
    coordinator.unregister_session(session);
}
