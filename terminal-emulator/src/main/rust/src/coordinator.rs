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

/// Session 引擎数据（用于异步拉取）
#[derive(Debug, Clone, Copy)]
pub struct SessionEngineData {
    pub ptr: jni::sys::jlong,
    pub pty_fd: jni::sys::jint,
    pub pid: jni::sys::jint,
}

/// Session 状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    Idle = 0,
    Running = 1,
    Busy = 2,
    WaitingLock = 3,
    Finished = 4,
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

/// 全局 Session 协调器实例
static SESSION_COORDINATOR: OnceCell<SessionCoordinator> = OnceCell::new();

/// Session 协调器
pub struct SessionCoordinator {
    pkg_lock: AtomicBool,
    pkg_lock_owner: AtomicUsize,
    session_counter: AtomicUsize,
    session_states: Mutex<HashMap<usize, SessionState>>,
    pid_map: Mutex<HashMap<i32, usize>>,
    engine_data_map: Mutex<HashMap<usize, SessionEngineData>>,
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
            engine_data_map: Mutex::new(HashMap::new()),
        });
        instance.ensure_monitor_started();
        instance
    }
    
    pub fn register_session(&self) -> usize {
        let id = self.session_counter.fetch_add(1, Ordering::SeqCst);
        self.update_session_state(id, SessionState::Idle);
        android_log(LogPriority::INFO, &format!("[SessionCoordinator] Registered session {}", id));
        id
    }
    
    pub fn unregister_session(&self, session_id: usize) {
        self.update_session_state(session_id, SessionState::Finished);
        let owner = self.pkg_lock_owner.load(Ordering::SeqCst);
        if owner == session_id {
            self.release_pkg_lock(session_id);
        }
        if let Ok(mut states) = self.session_states.lock() {
            states.remove(&session_id);
        }
    }

    pub fn ensure_monitor_started(&'static self) {
        static START: std::sync::Once = std::sync::Once::new();
        START.call_once(|| {
            std::thread::spawn(move || {
                android_log(LogPriority::INFO, "CHECKPOINT: Global Process Monitor STARTED");
                loop {
                    let mut status: i32 = 0;
                    let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
                    if pid > 0 {
                        let mut exit_code = 0;
                        if libc::WIFEXITED(status) {
                            exit_code = libc::WEXITSTATUS(status);
                        } else if libc::WIFSIGNALED(status) {
                            exit_code = -libc::WTERMSIG(status);
                        }

                        let mut pid_map = self.pid_map.lock().unwrap();
                        if let Some(&session_id) = pid_map.get(&pid) {
                            self.update_session_state(session_id, SessionState::Finished);
                            pid_map.remove(&pid);
                            android_log(LogPriority::WARN, &format!("[Monitor] Session {} (PID {}) exited with {}", session_id, pid, exit_code));
                        }
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            });
        });
    }

    pub fn bind_pid(&self, session_id: usize, pid: i32) {
        let mut pid_map = self.pid_map.lock().unwrap();
        pid_map.insert(pid, session_id);
        self.update_session_state(session_id, SessionState::Running);
    }

    pub fn set_engine_data(&self, session_id: usize, data: SessionEngineData) {
        let mut map = self.engine_data_map.lock().unwrap();
        map.insert(session_id, data);
    }

    pub fn take_engine_data(&self, session_id: usize) -> Option<SessionEngineData> {
        let mut map = self.engine_data_map.lock().unwrap();
        map.remove(&session_id)
    }
    
    pub fn try_acquire_pkg_lock(&self, session_id: usize) -> bool {
        match self.pkg_lock.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => {
                self.pkg_lock_owner.store(session_id, Ordering::SeqCst);
                self.update_session_state(session_id, SessionState::Busy);
                true
            }
            Err(_) => {
                self.update_session_state(session_id, SessionState::WaitingLock);
                false
            }
        }
    }
    
    pub fn release_pkg_lock(&self, session_id: usize) {
        let owner = self.pkg_lock_owner.load(Ordering::SeqCst);
        if owner == session_id {
            self.pkg_lock.store(false, Ordering::SeqCst);
            self.pkg_lock_owner.store(0, Ordering::SeqCst);
            self.update_session_state(session_id, SessionState::Running);
        }
    }
    
    pub fn is_pkg_lock_held(&self) -> bool { self.pkg_lock.load(Ordering::SeqCst) }
    pub fn get_pkg_lock_owner(&self) -> usize { self.pkg_lock_owner.load(Ordering::SeqCst) }
    
    fn update_session_state(&self, session_id: usize, state: SessionState) {
        if let Ok(mut states) = self.session_states.lock() {
            states.insert(session_id, state);
        }
    }
    
    pub fn get_session_state(&self, session_id: usize) -> Option<SessionState> {
        self.session_states.lock().ok().and_then(|states| states.get(&session_id).copied())
    }
    
    pub fn get_all_session_states(&self) -> Vec<(usize, SessionState)> {
        self.session_states.lock()
            .map(|states| states.iter().map(|(&k, &v)| (k, v)).collect())
            .unwrap_or_default()
    }
    
    pub fn has_waiting_sessions(&self) -> bool {
        self.session_states.lock()
            .map(|states| states.values().any(|&s| s == SessionState::WaitingLock))
            .unwrap_or(false)
    }
}

use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{jint, jboolean, jstring};

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_registerSession(_env: JNIEnv, _class: JClass) -> jint {
    SessionCoordinator::get().register_session() as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_unregisterSession(_env: JNIEnv, _class: JClass, session_id: jint) {
    SessionCoordinator::get().unregister_session(session_id as usize);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_tryAcquirePkgLock(_env: JNIEnv, _class: JClass, session_id: jint) -> jboolean {
    if SessionCoordinator::get().try_acquire_pkg_lock(session_id as usize) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_releasePkgLock(_env: JNIEnv, _class: JClass, session_id: jint) {
    SessionCoordinator::get().release_pkg_lock(session_id as usize);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_isPkgLockHeld(_env: JNIEnv, _class: JClass) -> jboolean {
    if SessionCoordinator::get().is_pkg_lock_held() { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getPkgLockOwner(_env: JNIEnv, _class: JClass) -> jint {
    SessionCoordinator::get().get_pkg_lock_owner() as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getSessionState(env: JNIEnv, _class: JClass, session_id: jint) -> jstring {
    let state = SessionCoordinator::get().get_session_state(session_id as usize).unwrap_or(SessionState::Idle);
    match env.new_string(state.as_str()) {
        Ok(j_str) => j_str.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getAllSessionStates(env: JNIEnv, _class: JClass) -> jstring {
    let states = SessionCoordinator::get().get_all_session_states();
    let mut res = String::from("Session States:\n");
    for (id, state) in states {
        res.push_str(&format!("  Session {}: {}\n", id, state.as_str()));
    }
    match env.new_string(res) {
        Ok(j_str) => j_str.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_linkPidToSession(_env: JNIEnv, _class: JClass, session_id: jint, pid: jint) {
    SessionCoordinator::get().bind_pid(session_id as usize, pid);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_pollEngineData(env: JNIEnv, _class: JClass, session_id: jint) -> jni::sys::jlongArray {
    if let Some(data) = SessionCoordinator::get().take_engine_data(session_id as usize) {
        let res = [data.ptr, data.pty_fd as i64, data.pid as i64];
        if let Ok(j_array) = env.new_long_array(3) {
            let _ = env.set_long_array_region(&j_array, 0, &res);
            j_array.into_raw()
        } else { std::ptr::null_mut() }
    } else { std::ptr::null_mut() }
}
