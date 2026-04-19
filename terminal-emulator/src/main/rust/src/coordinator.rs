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

/// Session 协调器
pub struct SessionCoordinator {
    /// pkg 操作锁（true = 已锁定）
    pkg_lock: AtomicBool,
    /// 当前 pkg 锁所有者的 session ID（0 表示无所有者）
    pkg_lock_owner: AtomicUsize,
    /// Session 计数器（用于生成唯一 ID）
    session_counter: AtomicUsize,
    /// Session 状态表
    session_states: Mutex<HashMap<usize, SessionState>>,
    /// PID 到 Session ID 的映射
    pid_map: Mutex<HashMap<i32, usize>>,
}

impl SessionCoordinator {
    /// 获取全局协调器实例
    pub fn get() -> &'static Self {
        let instance = SESSION_COORDINATOR.get_or_init(|| SessionCoordinator {
            pkg_lock: AtomicBool::new(false),
            pkg_lock_owner: AtomicUsize::new(0),
            session_counter: AtomicUsize::new(0),
            session_states: Mutex::new(HashMap::new()),
            pid_map: Mutex::new(HashMap::new()),
        });
        instance.ensure_monitor_started();
        instance
    }
    
    /// 注册新 Session
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
    
    /// 注销 Session
    pub fn unregister_session(&self, session_id: usize) {
        self.update_session_state(session_id, SessionState::Finished);
        
        // 如果这个 session 持有 pkg 锁，释放它
        let owner = self.pkg_lock_owner.load(Ordering::SeqCst);
        if owner == session_id {
            self.release_pkg_lock(session_id);
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
    fn update_session_state(&self, session_id: usize, state: SessionState) {
        if let Ok(mut states) = self.session_states.lock() {
            states.insert(session_id, state);
        }
    }
    
    /// 获取 Session 状态
    pub fn get_session_state(&self, session_id: usize) -> Option<SessionState> {
        self.session_states.lock().ok().and_then(|states| states.get(&session_id).copied())
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

    /// 启动全局进程监控线程（单例）
    pub fn ensure_monitor_started(&'static self) {
        static START: std::sync::Once = std::sync::Once::new();
        START.call_once(|| {
            std::thread::spawn(move || {
                android_log(LogPriority::INFO, "CHECKPOINT: Global Process Monitor STARTED");
                loop {
                    let mut status: i32 = 0;
                    // 使用 WNOHANG | WUNTRACED 以防阻塞
                    // 或者干脆阻塞式等待，直到有子进程变动
                    let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
                    
                    if pid > 0 {
                        let mut exit_code = 0;
                        let mut signaled = false;
                        
                        if unsafe { libc::WIFEXITED(status) } {
                            exit_code = unsafe { libc::WEXITSTATUS(status) };
                        } else if unsafe { libc::WIFSIGNALED(status) } {
                            exit_code = -unsafe { libc::WTERMSIG(status) };
                            signaled = true;
                        }

                        // 查找这个 PID 属于哪个 Session
                        let mut pid_map = self.pid_map.lock().unwrap();
                        if let Some(&session_id) = pid_map.get(&pid) {
                            android_log(
                                LogPriority::WARN,
                                &format!("[Monitor] Child {} (Session {}) exited with code {}{}", 
                                    pid, session_id, exit_code, if signaled { " (signaled)" } else { "" })
                            );
                            self.update_session_state(session_id, SessionState::Finished);
                            pid_map.remove(&pid);
                        } else {
                            android_log(LogPriority::DEBUG, &format!("[Monitor] Unknown child {} exited", pid));
                        }
                    } else {
                        // 如果没有子进程了，稍微睡一下防止死循环空转
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            });
        });
    }

    /// 关联 PID 和 Session
    pub fn bind_pid(&self, session_id: usize, pid: i32) {
        let mut pid_map = self.pid_map.lock().unwrap();
        pid_map.insert(pid, session_id);
        self.update_session_state(session_id, SessionState::Running);
        android_log(LogPriority::DEBUG, &format!("[SessionCoordinator] Session {} linked to PID {}", session_id, pid));
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

/// 关联 PID 和 Session ID
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_linkPidToSession(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
    pid: jint,
) {
    let coordinator = SessionCoordinator::get();
    coordinator.bind_pid(session_id as usize, pid);
}
