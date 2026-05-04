//! Session 协调器模块
//! 
//! 负责管理多个 Termux Session 之间的协调和资源共享
//! - Pkg 操作互斥锁
//! - Session 状态管理
//! - Session 注册和注销

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use once_cell::sync::OnceCell;

use crate::utils::{android_log, LogPriority};

/// 全局 Session 协调器实例
static SESSION_COORDINATOR: OnceCell<SessionCoordinator> = OnceCell::new();

/// Session 状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    Idle = 0,           // 空闲
    Running = 1,        // 命令执行中
    Busy = 2,           // 忙碌（如 pkg 操作）
    WaitingLock = 3,    // 等待锁
    Finished = 4,       // 已结束
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Idle => "Idle",
            SessionState::Running => "Running",
            SessionState::Busy => "Busy",
            SessionState::WaitingLock => "WaitingLock",
            SessionState::Finished => "Finished",
        }
    }
}

/// Session 的 Native 端完整状态
/// Rust 层是唯一真相源，Java 层通过 JNI 查询这些字段
#[derive(Debug, Clone, Copy)]
pub struct SessionData {
    pub pty_fd: i32,
    pub pid: i32,
    pub context_ptr: usize, // Arc<TerminalContext> 的 raw pointer
    pub state: SessionState,
}

/// Session 协调器
pub struct SessionCoordinator {
    /// pkg 操作锁（true = 已锁定）
    pkg_lock: AtomicBool,
    /// 当前 pkg 锁所有者的 session ID（0 表示无所有者）
    pkg_lock_owner: AtomicUsize,
    /// Session 计数器（用于生成唯一 ID）
    session_counter: AtomicUsize,
    /// Session 状态表（兼容旧接口）
    session_states: Mutex<HashMap<usize, SessionState>>,
    /// Session 完整数据表（新增）
    session_data: Mutex<HashMap<usize, SessionData>>,
    /// context_ptr → session_id 反向索引（新增）
    ptr_to_session: Mutex<HashMap<usize, usize>>,
}

impl SessionCoordinator {
    /// 获取全局协调器实例
    pub fn get() -> &'static Self {
        SESSION_COORDINATOR.get_or_init(|| Self::new_coordinator())
    }

    /// 创建一个新的、隔离的协调器实例
    /// 注意：此方法通常仅用于测试，以避免全局单例污染。
    pub fn new_coordinator() -> Self {
        SessionCoordinator {
            pkg_lock: AtomicBool::new(false),
            pkg_lock_owner: AtomicUsize::new(0),
            session_counter: AtomicUsize::new(0),
            session_states: Mutex::new(HashMap::new()),
            session_data: Mutex::new(HashMap::new()),
            ptr_to_session: Mutex::new(HashMap::new()),
        }
    }
    
    /// 注册新 Session（仅分配 ID，不绑定数据）
    /// 返回唯一的 Session ID
    pub fn register_session(&self) -> usize {
        let id = self.session_counter.fetch_add(1, Ordering::SeqCst);
        self.update_session_state(id, SessionState::Idle);
        android_log(
            LogPriority::INFO,
            &format!("[SessionCoordinator] Registered session {}", id)
        );
        id
    }
    
    /// 绑定 Session 完整数据（PTY fd、PID、engine context）
    /// 调用时机：PTY 和引擎创建成功后
    pub fn bind_session_data(&self, session_id: usize, pty_fd: i32, pid: i32, context_ptr: usize) {
        let data = SessionData {
            pty_fd,
            pid,
            context_ptr,
            state: SessionState::Running,
        };
        if let Ok(mut map) = self.session_data.lock() {
            map.insert(session_id, data);
        }
        if let Ok(mut ptr_map) = self.ptr_to_session.lock() {
            ptr_map.insert(context_ptr, session_id);
        }
        self.update_session_state(session_id, SessionState::Running);
        android_log(
            LogPriority::INFO,
            &format!(
                "[SessionCoordinator] Bound session {}: pty_fd={}, pid={}, context_ptr={:p}",
                session_id, pty_fd, pid, context_ptr as *const ()
            )
        );
    }
    
    /// 注销 Session
    pub fn unregister_session(&self, session_id: usize) {
        self.update_session_state(session_id, SessionState::Finished);
        
        // 如果这个 session 持有 pkg 锁，释放它
        let owner = self.pkg_lock_owner.load(Ordering::SeqCst);
        if owner == session_id {
            self.release_pkg_lock(session_id);
        }
        
        // 从数据表中移除，并清理反向索引
        if let Ok(mut data_map) = self.session_data.lock() {
            if let Some(data) = data_map.remove(&session_id) {
                if let Ok(mut ptr_map) = self.ptr_to_session.lock() {
                    ptr_map.remove(&data.context_ptr);
                }
            }
        }
        
        // 从状态表中移除
        if let Ok(mut states) = self.session_states.lock() {
            states.remove(&session_id);
        }
        
        android_log(
            LogPriority::INFO,
            &format!("[SessionCoordinator] Unregistered session {}", session_id)
        );
    }
    
    /// 尝试获取 pkg 操作锁
    /// 
    /// # Arguments
    /// * `session_id` - 请求锁的 Session ID
    /// 
    /// # Returns
    /// * `true` - 成功获取锁
    /// * `false` - 锁已被其他 session 占用
    pub fn try_acquire_pkg_lock(&self, session_id: usize) -> bool {
        match self.pkg_lock.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => {
                // 成功获取锁
                self.pkg_lock_owner.store(session_id, Ordering::SeqCst);
                self.update_session_state(session_id, SessionState::Busy);
                android_log(
                    LogPriority::INFO,
                    &format!("[SessionCoordinator] Session {} acquired pkg lock", session_id)
                );
                true
            }
            Err(_) => {
                // 锁已被占用
                let owner = self.pkg_lock_owner.load(Ordering::SeqCst);
                android_log(
                    LogPriority::WARN,
                    &format!(
                        "[SessionCoordinator] Session {} failed to acquire pkg lock - owned by session {}",
                        session_id, owner
                    )
                );
                self.update_session_state(session_id, SessionState::WaitingLock);
                false
            }
        }
    }
    
    /// 释放 pkg 操作锁
    /// 
    /// # Arguments
    /// * `session_id` - 释放锁的 Session ID（必须是锁的所有者）
    pub fn release_pkg_lock(&self, session_id: usize) {
        let owner = self.pkg_lock_owner.load(Ordering::SeqCst);
        if owner == session_id {
            self.pkg_lock.store(false, Ordering::SeqCst);
            self.pkg_lock_owner.store(0, Ordering::SeqCst);
            self.update_session_state(session_id, SessionState::Running);
            android_log(
                LogPriority::INFO,
                &format!("[SessionCoordinator] Session {} released pkg lock", session_id)
            );
        } else {
            android_log(
                LogPriority::WARN,
                &format!(
                    "[SessionCoordinator] Session {} tried to release pkg lock but doesn't own it (owner: {})",
                    session_id, owner
                )
            );
        }
    }
    
    /// 检查 pkg 锁是否被占用
    pub fn is_pkg_lock_held(&self) -> bool {
        self.pkg_lock.load(Ordering::SeqCst)
    }
    
    /// 获取 pkg 锁所有者的 Session ID
    /// 返回 0 表示无所有者（锁未被占用）
    pub fn get_pkg_lock_owner(&self) -> usize {
        self.pkg_lock_owner.load(Ordering::SeqCst)
    }
    
    /// 更新 Session 状态
    pub fn update_session_state(&self, session_id: usize, state: SessionState) {
        if let Ok(mut states) = self.session_states.lock() {
            states.insert(session_id, state);
        }
    }
    
    /// 获取 Session 状态
    pub fn get_session_state(&self, session_id: usize) -> Option<SessionState> {
        self.session_states.lock().ok().and_then(|states| states.get(&session_id).copied())
    }
    
    /// 通过 session_id 获取 Session 数据
    pub fn get_session_data(&self, session_id: usize) -> Option<SessionData> {
        self.session_data.lock().ok().and_then(|data| data.get(&session_id).cloned())
    }
    
    /// 通过 context_ptr 获取 session_id
    pub fn get_session_id_by_ptr(&self, context_ptr: usize) -> Option<usize> {
        self.ptr_to_session.lock().ok().and_then(|map| map.get(&context_ptr).copied())
    }
    
    /// 通过 context_ptr 获取 Session 数据
    pub fn get_session_data_by_ptr(&self, context_ptr: usize) -> Option<SessionData> {
        let session_id = self.get_session_id_by_ptr(context_ptr)?;
        self.get_session_data(session_id)
    }
    
    /// 获取 Session 的 PID（供 JNI 查询）
    pub fn get_session_pid(&self, session_id: usize) -> i32 {
        self.get_session_data(session_id).map(|d| d.pid).unwrap_or(-1)
    }
    
    /// 获取 Session 的 PTY fd（供 JNI 查询）
    pub fn get_session_pty_fd(&self, session_id: usize) -> i32 {
        self.get_session_data(session_id).map(|d| d.pty_fd).unwrap_or(-1)
    }
    
    /// 检查 Session 是否仍在运行（进程存活）
    pub fn is_session_running(&self, session_id: usize) -> bool {
        self.get_session_data(session_id)
            .map(|d| d.state != SessionState::Finished && d.pid > 0)
            .unwrap_or(false)
    }
    
    /// 获取所有 Session 的状态列表（用于调试）
    pub fn get_all_session_states(&self) -> Vec<(usize, SessionState)> {
        self.session_states.lock()
            .map(|states| states.iter().map(|(&k, &v)| (k, v)).collect())
            .unwrap_or_default()
    }
    
    /// 检查是否有 session 在等待 pkg 锁
    pub fn has_waiting_sessions(&self) -> bool {
        self.session_states.lock()
            .map(|states| states.values().any(|&s| s == SessionState::WaitingLock))
            .unwrap_or(false)
    }
}

// ============================================================================
// JNI 接口 - 供 Java 层调用
// ============================================================================

use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{jint, jboolean, jstring};

/// 注册新 Session 并返回 Session ID
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_registerSession(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    let coordinator = SessionCoordinator::get();
    let session_id = coordinator.register_session();
    session_id as jint
}

/// 注销 Session
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_unregisterSession(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
) {
    let coordinator = SessionCoordinator::get();
    coordinator.unregister_session(session_id as usize);
}

/// 尝试获取 pkg 锁
/// 返回 true 表示成功，false 表示锁已被占用
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_tryAcquirePkgLock(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
) -> jboolean {
    let coordinator = SessionCoordinator::get();
    if coordinator.try_acquire_pkg_lock(session_id as usize) {
        1
    } else {
        0
    }
}

/// 释放 pkg 锁
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_releasePkgLock(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
) {
    let coordinator = SessionCoordinator::get();
    coordinator.release_pkg_lock(session_id as usize);
}

/// 检查 pkg 锁是否被占用
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_isPkgLockHeld(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let coordinator = SessionCoordinator::get();
    if coordinator.is_pkg_lock_held() {
        1
    } else {
        0
    }
}

/// 获取 pkg 锁所有者的 Session ID
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getPkgLockOwner(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    let coordinator = SessionCoordinator::get();
    coordinator.get_pkg_lock_owner() as jint
}

/// 获取 Session 状态字符串（用于调试）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getSessionState(
    env: JNIEnv,
    _class: JClass,
    session_id: jint,
) -> jstring {
    let coordinator = SessionCoordinator::get();
    let state = coordinator.get_session_state(session_id as usize)
        .unwrap_or(SessionState::Idle);
    
    let state_str = state.as_str();
    match env.new_string(state_str) {
        Ok(j_str) => j_str.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// 获取所有 Session 状态（调试用）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getAllSessionStates(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let coordinator = SessionCoordinator::get();
    let states = coordinator.get_all_session_states();
    
    let mut result = String::from("Session States:\n");
    for (id, state) in states {
        result.push_str(&format!("  Session {}: {}\n", id, state.as_str()));
    }
    
    match env.new_string(result) {
        Ok(j_str) => j_str.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// 通过 engine ptr 获取 PID
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_sessionGetPid(
    _env: JNIEnv,
    _class: JClass,
    engine_ptr: jni::sys::jlong,
) -> jint {
    let coordinator = SessionCoordinator::get();
    coordinator.get_session_data_by_ptr(engine_ptr as usize)
        .map(|d| d.pid)
        .unwrap_or(-1)
}

/// 通过 engine ptr 获取 PTY fd
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_sessionGetPtyFd(
    _env: JNIEnv,
    _class: JClass,
    engine_ptr: jni::sys::jlong,
) -> jint {
    let coordinator = SessionCoordinator::get();
    coordinator.get_session_data_by_ptr(engine_ptr as usize)
        .map(|d| d.pty_fd)
        .unwrap_or(-1)
}

/// 通过 engine ptr 检查 session 是否仍在运行
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_sessionIsRunning(
    _env: JNIEnv,
    _class: JClass,
    engine_ptr: jni::sys::jlong,
) -> jboolean {
    let coordinator = SessionCoordinator::get();
    let is_running = coordinator.get_session_data_by_ptr(engine_ptr as usize)
        .map(|d| {
            if d.state == SessionState::Finished {
                return false;
            }
            // kill(pid, 0) 不发送信号，只检查进程是否存在
            let exists = unsafe { libc::kill(d.pid, 0) == 0 };
            exists || nix::errno::Errno::last() == nix::errno::Errno::EPERM
        })
        .unwrap_or(false);
    if is_running { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    // new_coordinator 会通过 super::* 自动引入

    // -------------------------------------------------------------------------
    // SessionState
    // -------------------------------------------------------------------------
    #[test]
    fn session_state_as_str() {
        assert_eq!(SessionState::Idle.as_str(), "Idle");
        assert_eq!(SessionState::Running.as_str(), "Running");
        assert_eq!(SessionState::Busy.as_str(), "Busy");
        assert_eq!(SessionState::WaitingLock.as_str(), "WaitingLock");
        assert_eq!(SessionState::Finished.as_str(), "Finished");
    }

    // -------------------------------------------------------------------------
    // Registration
    // -------------------------------------------------------------------------
    #[test]
    fn register_session_returns_incrementing_ids() {
        let coord = SessionCoordinator::new_coordinator();
        let id0 = coord.register_session();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn register_session_sets_idle_state() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        assert_eq!(coord.get_session_state(id), Some(SessionState::Idle));
    }

    // -------------------------------------------------------------------------
    // Unregistration
    // -------------------------------------------------------------------------
    #[test]
    fn unregister_session_sets_finished() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.unregister_session(id);
        assert_eq!(coord.get_session_state(id), None); // removed from map
    }

    #[test]
    fn unregister_session_removes_from_all_states() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.unregister_session(id);
        let all = coord.get_all_session_states();
        assert!(!all.iter().any(|(k, _)| *k == id));
    }

    // -------------------------------------------------------------------------
    // Pkg lock - basic acquire/release
    // -------------------------------------------------------------------------
    #[test]
    fn try_acquire_pkg_lock_succeeds_when_free() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        assert!(coord.try_acquire_pkg_lock(id));
        assert!(coord.is_pkg_lock_held());
        assert_eq!(coord.get_pkg_lock_owner(), id);
        assert_eq!(coord.get_session_state(id), Some(SessionState::Busy));
    }

    #[test]
    fn release_pkg_lock_frees_lock() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.try_acquire_pkg_lock(id);
        coord.release_pkg_lock(id);
        assert!(!coord.is_pkg_lock_held());
        assert_eq!(coord.get_pkg_lock_owner(), 0);
    }

    #[test]
    fn release_pkg_lock_changes_state_to_running() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.try_acquire_pkg_lock(id);
        coord.release_pkg_lock(id);
        assert_eq!(coord.get_session_state(id), Some(SessionState::Running));
    }

    // -------------------------------------------------------------------------
    // Pkg lock - contention
    // -------------------------------------------------------------------------
    #[test]
    fn try_acquire_pkg_lock_fails_when_held() {
        let coord = SessionCoordinator::new_coordinator();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        coord.try_acquire_pkg_lock(id1);
        assert!(!coord.try_acquire_pkg_lock(id2));
    }

    #[test]
    fn contending_session_gets_waiting_lock_state() {
        let coord = SessionCoordinator::new_coordinator();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        coord.try_acquire_pkg_lock(id1);
        coord.try_acquire_pkg_lock(id2);
        assert_eq!(coord.get_session_state(id2), Some(SessionState::WaitingLock));
    }

    #[test]
    fn release_pkg_lock_not_owner_does_nothing() {
        let coord = SessionCoordinator::new_coordinator();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        coord.try_acquire_pkg_lock(id1);
        coord.release_pkg_lock(id2); // id2 does not own the lock
        assert!(coord.is_pkg_lock_held());
        assert_eq!(coord.get_pkg_lock_owner(), id1);
    }

    // -------------------------------------------------------------------------
    // Unregister releases lock
    // -------------------------------------------------------------------------
    #[test]
    fn unregister_releases_owned_lock() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.try_acquire_pkg_lock(id);
        coord.unregister_session(id);
        assert!(!coord.is_pkg_lock_held());
        assert_eq!(coord.get_pkg_lock_owner(), 0);
    }

    #[test]
    fn unregister_does_not_release_others_lock() {
        let coord = SessionCoordinator::new_coordinator();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        coord.try_acquire_pkg_lock(id1);
        coord.unregister_session(id2); // id2 doesn't own the lock
        assert!(coord.is_pkg_lock_held());
        assert_eq!(coord.get_pkg_lock_owner(), id1);
    }

    // -------------------------------------------------------------------------
    // State queries
    // -------------------------------------------------------------------------
    #[test]
    fn get_session_state_unknown_returns_none() {
        let coord = SessionCoordinator::new_coordinator();
        assert_eq!(coord.get_session_state(999), None);
    }

    #[test]
    fn get_all_session_states_reflects_changes() {
        let coord = SessionCoordinator::new_coordinator();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        coord.try_acquire_pkg_lock(id1);
        let all = coord.get_all_session_states();
        assert_eq!(all.len(), 2);
        let states: HashMap<usize, SessionState> = all.into_iter().collect();
        assert_eq!(states[&id1], SessionState::Busy);
        assert_eq!(states[&id2], SessionState::Idle);
    }

    #[test]
    fn has_waiting_sessions_true() {
        let coord = SessionCoordinator::new_coordinator();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        coord.try_acquire_pkg_lock(id1);
        coord.try_acquire_pkg_lock(id2);
        assert!(coord.has_waiting_sessions());
    }

    #[test]
    fn has_waiting_sessions_false() {
        let coord = SessionCoordinator::new_coordinator();
        let id1 = coord.register_session();
        let id2 = coord.register_session();
        coord.try_acquire_pkg_lock(id1);
        assert!(!coord.has_waiting_sessions());
    }

    #[test]
    fn has_waiting_sessions_empty() {
        let coord = SessionCoordinator::new_coordinator();
        assert!(!coord.has_waiting_sessions());
    }

    // -------------------------------------------------------------------------
    // SessionData binding and queries
    // -------------------------------------------------------------------------
    #[test]
    fn bind_session_data_stores_all_fields() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.bind_session_data(id, 42, 12345, 0xDEADBEEF);

        let data = coord.get_session_data(id).unwrap();
        assert_eq!(data.pty_fd, 42);
        assert_eq!(data.pid, 12345);
        assert_eq!(data.context_ptr, 0xDEADBEEF);
        assert_eq!(data.state, SessionState::Running);
    }

    #[test]
    fn get_session_id_by_ptr_reverse_lookup() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.bind_session_data(id, 1, 100, 0xABC);

        assert_eq!(coord.get_session_id_by_ptr(0xABC), Some(id));
        assert_eq!(coord.get_session_id_by_ptr(0x999), None);
    }

    #[test]
    fn get_session_data_by_ptr_works() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.bind_session_data(id, 3, 300, 0x123);

        let data = coord.get_session_data_by_ptr(0x123).unwrap();
        assert_eq!(data.pty_fd, 3);
        assert_eq!(data.pid, 300);
    }

    #[test]
    fn get_session_pid_and_pty_fd() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.bind_session_data(id, 99, 7777, 0x0);

        assert_eq!(coord.get_session_pid(id), 7777);
        assert_eq!(coord.get_session_pty_fd(id), 99);
    }

    #[test]
    fn is_session_running_true_after_bind() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.bind_session_data(id, 1, 1, 0x0);
        assert!(coord.is_session_running(id));
    }

    #[test]
    fn is_session_running_false_after_unregister() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.bind_session_data(id, 1, 1, 0x0);
        coord.unregister_session(id);
        assert!(!coord.is_session_running(id));
        assert_eq!(coord.get_session_pid(id), -1);
        assert_eq!(coord.get_session_pty_fd(id), -1);
    }

    #[test]
    fn unregister_cleans_ptr_to_session_index() {
        let coord = SessionCoordinator::new_coordinator();
        let id = coord.register_session();
        coord.bind_session_data(id, 1, 1, 0xBEEF);
        coord.unregister_session(id);
        assert_eq!(coord.get_session_id_by_ptr(0xBEEF), None);
    }
}
