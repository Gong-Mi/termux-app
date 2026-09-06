//! Session 协调器模块
//!
//! 负责管理多个 Termux Session 之间的协调和资源共享
//! - Pkg 操作互斥锁
//! - Session 状态管理
//! - Session 注册和注销

use crate::engine::io_runtime::IoOutcome;
use crate::process_owner::{ExitOutcome, ProcessOwner};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use crate::utils::{LogPriority, android_log};

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

/// Forget pending OR claimed tokens after engine revocation. destroy_engine
/// releases the engine-handle lock before entering here; never destroy here.
pub(crate) fn discard_engine_data(handle: jni::sys::jlong) {
    if let Some(coordinator) = SESSION_COORDINATOR.get() {
        let mut registry = coordinator
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry
            .engine_data
            .retain(|_, value| value.data.ptr != handle);
    }
}

/// Both states retain native cleanup responsibility. Claimed metadata is only
/// provisional: callers must ack successfully before treating it as ownership.
struct EngineDelivery {
    data: SessionEngineData,
    claimed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionObserver {
    session_id: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub session_id: usize,
    pub process: ExitOutcome,
    pub io: IoOutcome,
}

struct SessionRecord {
    state: SessionState,
    process: Option<Arc<ProcessOwner>>,
    terminate_requested: bool,
    process_outcome: Option<ExitOutcome>,
    io_outcome: Option<IoOutcome>,
    candidate_taken: bool,
}
#[derive(Default)]
struct Registry {
    next_session: usize,
    sessions: HashMap<usize, SessionRecord>,
    pkg_owner: Option<usize>,
    pid_owners: HashMap<i32, Weak<ProcessOwner>>,
    engine_data: HashMap<usize, EngineDelivery>,
}

/// Membership, delivery, process publication and package ownership share one
/// state lock. Delivery methods never enter engine/JNI/IO code under this lock.
pub struct SessionCoordinator {
    registry: Mutex<Registry>,
}

/// Legacy known-PID waits share the managed status cache, never a second reaper.
/// Raw PID compatibility does not encode identity after a caller loses ownership.
pub(crate) fn managed_process_for_pid(pid: i32) -> Option<Arc<ProcessOwner>> {
    SESSION_COORDINATOR.get().and_then(|coordinator| {
        let registry = coordinator
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry.pid_owners.get(&pid).and_then(Weak::upgrade)
    })
}

impl SessionCoordinator {
    pub fn get() -> &'static Self {
        SESSION_COORDINATOR.get_or_init(|| Self {
            registry: Mutex::new(Registry::default()),
        })
    }

    /// Non-reused IDs fit the JNI signed int. usize::MAX signals exhaustion.
    pub fn register_session(&self) -> usize {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let id = registry.next_session;
        let Some(next) = id.checked_add(1).filter(|next| *next <= i32::MAX as usize) else {
            return usize::MAX;
        };
        registry.next_session = next;
        registry.sessions.insert(
            id,
            SessionRecord {
                state: SessionState::Idle,
                process: None,
                terminate_requested: false,
                process_outcome: None,
                io_outcome: None,
                candidate_taken: false,
            },
        );
        id
    }

    pub fn unregister_session(&self, session_id: usize) {
        let delivery = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.sessions.remove(&session_id);
            if registry.pkg_owner == Some(session_id) {
                registry.pkg_owner = None;
            }
            registry.engine_data.remove(&session_id)
        };
        // No coordinator lock is held while destroy cancels/reaps IO resources.
        if let Some(delivery) = delivery {
            crate::engine::destroy_unadopted_engine(delivery.data.ptr);
        }
    }

    /// Compatibility entry point: there is deliberately no global monitor now.
    pub fn ensure_monitor_started(&'static self) {}

    pub fn has_session(&self, session_id: usize) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .contains_key(&session_id)
    }

    pub fn completion_observer(&self, session_id: usize) -> Option<CompletionObserver> {
        self.has_session(session_id)
            .then_some(CompletionObserver { session_id })
    }

    pub fn bind_pid(
        &'static self,
        session_id: usize,
        pid: i32,
    ) -> std::io::Result<Arc<ProcessOwner>> {
        self.bind_child(session_id, pid, false)
    }
    pub(crate) fn bind_pty_child(
        &'static self,
        session_id: usize,
        pid: i32,
    ) -> std::io::Result<Arc<ProcessOwner>> {
        self.bind_child(session_id, pid, true)
    }

    fn bind_child(
        &'static self,
        session_id: usize,
        pid: i32,
        counted: bool,
    ) -> std::io::Result<Arc<ProcessOwner>> {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        registry
            .pid_owners
            .retain(|_, owner| owner.strong_count() != 0);
        let already_owned = registry
            .pid_owners
            .get(&pid)
            .and_then(Weak::upgrade)
            .is_some_and(|owner| owner.is_running());
        let rejection = match registry.sessions.get(&session_id) {
            None => Some(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "session was unregistered",
            )),
            Some(record) if record.process.is_some() || already_owned => Some(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "child already owned",
            )),
            _ => None,
        };
        if let Some(error) = rejection {
            if counted && !already_owned {
                // The asynchronous creator transfers responsibility even when
                // its session disappeared. Claim once, keep that exact owner
                // for cleanup, and never reconstruct a PID after it was reaped.
                let cleanup = ProcessOwner::claim(pid);
                if let Ok(owner) = &cleanup {
                    registry.pid_owners.insert(pid, Arc::downgrade(owner));
                }
                drop(registry);
                if let Ok(owner) = cleanup {
                    let _ = owner.terminate();
                    let _ = owner.wait();
                }
                crate::pty::record_managed_child_exit();
            }
            return Err(error);
        }
        // Claim only after duplicate/membership checks. A fast exit is retained
        // by the owner even when it is reaped here, before monitor publication.
        let owner = match ProcessOwner::claim(pid) {
            Ok(owner) => owner,
            Err(error) => {
                if counted {
                    crate::pty::record_managed_child_exit();
                }
                return Err(error);
            }
        };
        registry.pid_owners.insert(pid, Arc::downgrade(&owner));
        let busy = registry.pkg_owner == Some(session_id);
        let record = registry.sessions.get_mut(&session_id).unwrap();
        let terminate = record.terminate_requested;
        record.state = if owner.is_running() {
            if busy {
                SessionState::Busy
            } else {
                SessionState::Running
            }
        } else {
            SessionState::Finished
        };
        record.process = Some(Arc::clone(&owner));
        if let Some(outcome) = owner.outcome() {
            record.process_outcome = Some(outcome);
        }
        let monitored = Arc::clone(&owner);
        let spawn = std::thread::Builder::new()
            .name("session-process".into())
            .spawn(move || {
                let outcome = monitored.wait();
                if counted {
                    crate::pty::record_managed_child_exit();
                }
                self.process_exited(session_id, &monitored, outcome);
            });
        if let Err(error) = spawn {
            let record = registry.sessions.get_mut(&session_id).unwrap();
            record.state = SessionState::Finished;
            if registry.pkg_owner == Some(session_id) {
                registry.pkg_owner = None;
            }
            drop(registry);
            // bind_pty_child is called on the asynchronous creation thread.
            // No published engine can escape without a reaping owner.
            let _ = owner.terminate();
            let _ = owner.wait();
            if counted {
                crate::pty::record_managed_child_exit();
            }
            return Err(error);
        }
        drop(registry);
        if terminate && let Err(error) = owner.terminate() {
            android_log(
                LogPriority::ERROR,
                &format!("pending terminate failed: {error}"),
            );
        }
        Ok(owner)
    }

    fn process_exited(&self, session_id: usize, owner: &Arc<ProcessOwner>, outcome: ExitOutcome) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = registry.sessions.get_mut(&session_id)
            && record
                .process
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, owner))
        {
            if record.process_outcome.is_none() {
                record.process_outcome = Some(outcome);
            }
            record.state = SessionState::Finished;
            if registry.pkg_owner == Some(session_id) {
                registry.pkg_owner = None;
            }
        }
        drop(registry);
        android_log(
            LogPriority::INFO,
            &format!(
                "PROCESS_EXIT: session={session_id} pid={} {outcome:?}; IO/drain/UI remain separate",
                owner.pid()
            ),
        );
    }

    pub(crate) fn record_io_outcome(&self, observer: CompletionObserver, outcome: IoOutcome) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let Some(record) = registry.sessions.get_mut(&observer.session_id) else {
            return;
        };
        if record.io_outcome.is_none() {
            record.io_outcome = Some(outcome);
        }
    }

    /// Consume the joined process/IO facts once. This does not transfer any
    /// engine, fd, runtime, or process cleanup ownership.
    pub fn take_completion_candidate(&self, session_id: usize) -> Option<CompletionCandidate> {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let record = registry.sessions.get_mut(&session_id)?;
        let process = record.process_outcome?;
        let io = record.io_outcome?;
        if record.candidate_taken {
            return None;
        }
        record.candidate_taken = true;
        Some(CompletionCandidate {
            session_id,
            process,
            io,
        })
    }

    pub fn completion_facts(&self, session_id: usize) -> Option<(ExitOutcome, IoOutcome)> {
        let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let record = registry.sessions.get(&session_id)?;
        Some((record.process_outcome?, record.io_outcome?))
    }

    #[cfg(test)]
    fn record_process_outcome_for_test(&self, session_id: usize, outcome: ExitOutcome) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = registry.sessions.get_mut(&session_id)
            && record.process_outcome.is_none()
        {
            record.process_outcome = Some(outcome);
        }
    }

    /// A request before child bind is retained. Unknown/terminal sessions cannot
    /// become raw-PID signal targets. No Java presentation PID is consulted.
    pub fn terminate_session(&self, session_id: usize) -> std::io::Result<bool> {
        let process = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = registry.sessions.get_mut(&session_id) else {
                return Ok(false);
            };
            record.terminate_requested = true;
            record.process.clone()
        };
        match process {
            Some(owner) => owner.terminate(),
            None => Ok(true),
        }
    }

    pub fn process_for_session(&self, session_id: usize) -> Option<Arc<ProcessOwner>> {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .get(&session_id)
            .and_then(|record| record.process.clone())
    }

    /// [kind, pid, code]: pending=0, running=1, exited=2, lost=3.
    /// A terminal snapshot never publishes a killable PID.
    pub fn process_status(&self, session_id: usize) -> Option<[i32; 3]> {
        let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let record = registry.sessions.get(&session_id)?;
        Some(match record.process.as_ref() {
            None => [0, 0, 0],
            Some(owner) => match owner.outcome() {
                None => [1, owner.pid(), 0],
                Some(ExitOutcome::Exited(code)) => [2, -1, code],
                Some(ExitOutcome::Lost(error)) => [3, -1, error],
            },
        })
    }

    /// Offer an engine for native-owned delivery. The async creator publishes
    /// once; as before, a different replacement reclaims the displaced engine.
    /// Repeating the same token updates metadata but must not reopen a claim.
    /// This consumes cleanup responsibility, including when membership is gone.
    pub fn set_engine_data(&self, session_id: usize, data: SessionEngineData) {
        let displaced = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            if !registry.sessions.contains_key(&session_id) {
                Some(data)
            } else if let Some(current) = registry.engine_data.get_mut(&session_id)
                && current.data.ptr == data.ptr
            {
                current.data = data;
                None
            } else {
                registry
                    .engine_data
                    .insert(
                        session_id,
                        EngineDelivery {
                            data,
                            claimed: false,
                        },
                    )
                    .map(|old| old.data)
            }
        };
        if let Some(old) = displaced {
            crate::engine::destroy_unadopted_engine(old.ptr);
        }
    }

    /// Legacy poll is an atomic single-step ownership transfer, never a claim
    /// thief. Removal and membership revocation use the same linearization lock.
    pub fn take_engine_data(&self, session_id: usize) -> Option<SessionEngineData> {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry.engine_data.get(&session_id)?.claimed {
            return None;
        }
        registry
            .engine_data
            .remove(&session_id)
            .map(|offer| offer.data)
    }

    /// Pending -> Claimed; native still owns cancellation/reclamation until ack.
    /// A claim is not a lifetime lease: unregister/reject/destroy may revoke it.
    pub fn claim_engine_data(
        &self,
        session_id: usize,
        expected_handle: jni::sys::jlong,
    ) -> Option<SessionEngineData> {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let offer = registry.engine_data.get_mut(&session_id)?;
        if offer.claimed || offer.data.ptr != expected_handle {
            return None;
        }
        offer.claimed = true;
        Some(offer.data)
    }

    /// Only a matching claim transfers cleanup responsibility to the caller.
    /// False means the caller must not adopt the provisional metadata.
    pub fn ack_engine_data(&self, session_id: usize, expected_handle: jni::sys::jlong) -> bool {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry
            .engine_data
            .get(&session_id)
            .is_none_or(|offer| !offer.claimed || offer.data.ptr != expected_handle)
        {
            return false;
        }
        registry.engine_data.remove(&session_id);
        true
    }

    /// Revoke either delivery state, then destroy outside ALL coordinator locks.
    pub fn reject_engine_data(&self, session_id: usize, expected_handle: jni::sys::jlong) -> bool {
        let delivery = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            if registry
                .engine_data
                .get(&session_id)
                .is_none_or(|offer| offer.data.ptr != expected_handle)
            {
                return false;
            }
            registry.engine_data.remove(&session_id).unwrap()
        };
        crate::engine::destroy_unadopted_engine(delivery.data.ptr);
        true
    }

    pub fn try_acquire_pkg_lock(&self, session_id: usize) -> bool {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let Some(record) = registry.sessions.get(&session_id) else {
            return false;
        };
        if record.state == SessionState::Finished {
            return false;
        }
        if registry.pkg_owner.is_none() {
            registry.pkg_owner = Some(session_id);
            registry.sessions.get_mut(&session_id).unwrap().state = SessionState::Busy;
            true
        } else {
            registry.sessions.get_mut(&session_id).unwrap().state = SessionState::WaitingLock;
            false
        }
    }
    pub fn release_pkg_lock(&self, session_id: usize) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry.pkg_owner == Some(session_id) {
            registry.pkg_owner = None;
            if let Some(record) = registry.sessions.get_mut(&session_id)
                && record.state != SessionState::Finished
            {
                record.state = SessionState::Running;
            }
        }
    }
    pub fn is_pkg_lock_held(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pkg_owner
            .is_some()
    }
    pub fn get_pkg_lock_owner(&self) -> usize {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pkg_owner
            .unwrap_or(0)
    }
    pub fn get_session_state(&self, session_id: usize) -> Option<SessionState> {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .get(&session_id)
            .map(|record| record.state)
    }
    pub fn get_all_session_states(&self) -> Vec<(usize, SessionState)> {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .iter()
            .map(|(&id, record)| (id, record.state))
            .collect()
    }
    pub fn has_waiting_sessions(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .values()
            .any(|record| record.state == SessionState::WaitingLock)
    }
}

use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{jboolean, jint, jstring};

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_registerSession(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    SessionCoordinator::get().register_session() as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_unregisterSession(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
) {
    SessionCoordinator::get().unregister_session(session_id as usize);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_tryAcquirePkgLock(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
) -> jboolean {
    if SessionCoordinator::get().try_acquire_pkg_lock(session_id as usize) {
        1
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_releasePkgLock(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
) {
    SessionCoordinator::get().release_pkg_lock(session_id as usize);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_isPkgLockHeld(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if SessionCoordinator::get().is_pkg_lock_held() {
        1
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getPkgLockOwner(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    SessionCoordinator::get().get_pkg_lock_owner() as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getSessionState(
    env: JNIEnv,
    _class: JClass,
    session_id: jint,
) -> jstring {
    let state = SessionCoordinator::get()
        .get_session_state(session_id as usize)
        .unwrap_or(SessionState::Idle);
    match env.new_string(state.as_str()) {
        Ok(j_str) => j_str.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getAllSessionStates(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
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
pub extern "system" fn Java_com_termux_terminal_JNI_linkPidToSession(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
    pid: jint,
) {
    if let Err(error) = SessionCoordinator::get().bind_pid(session_id as usize, pid) {
        android_log(
            LogPriority::ERROR,
            &format!("linkPidToSession rejected: {error}"),
        );
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_pollEngineData(
    env: JNIEnv,
    _class: JClass,
    session_id: jint,
) -> jni::sys::jlongArray {
    // Allocate before claiming so allocation failure cannot strand a handle.
    let Ok(j_array) = env.new_long_array(3) else {
        return std::ptr::null_mut();
    };
    if let Some(data) = SessionCoordinator::get().take_engine_data(session_id as usize) {
        let res = [data.ptr, data.pty_fd as i64, data.pid as i64];
        if env.set_long_array_region(&j_array, 0, &res).is_ok() {
            j_array.into_raw()
        } else {
            crate::engine::destroy_unadopted_engine(data.ptr);
            std::ptr::null_mut()
        }
    } else {
        std::ptr::null_mut()
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_terminateSession(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
) -> jboolean {
    match SessionCoordinator::get().terminate_session(session_id as usize) {
        Ok(sent) => {
            if sent {
                1
            } else {
                0
            }
        }
        Err(error) => {
            android_log(
                LogPriority::ERROR,
                &format!("terminateSession failed: {error}"),
            );
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getSessionProcessStatus(
    env: JNIEnv,
    _class: JClass,
    session_id: jint,
) -> jni::sys::jintArray {
    let Some(status) = SessionCoordinator::get().process_status(session_id as usize) else {
        return std::ptr::null_mut();
    };
    let Ok(array) = env.new_int_array(3) else {
        return std::ptr::null_mut();
    };
    if env.set_int_array_region(&array, 0, &status).is_err() {
        return std::ptr::null_mut();
    }
    array.into_raw()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_claimEngineData(
    env: JNIEnv,
    _class: JClass,
    session_id: jint,
    handle: jni::sys::jlong,
) -> jni::sys::jlongArray {
    // Allocate before reserving the offer: allocation failure leaves it pending.
    let Ok(array) = env.new_long_array(3) else {
        return std::ptr::null_mut();
    };
    let coordinator = SessionCoordinator::get();
    let Some(data) = coordinator.claim_engine_data(session_id as usize, handle) else {
        return std::ptr::null_mut();
    };
    let values = [data.ptr, data.pty_fd as i64, data.pid as i64];
    if env.set_long_array_region(&array, 0, &values).is_err() {
        coordinator.reject_engine_data(session_id as usize, handle);
        return std::ptr::null_mut();
    }
    array.into_raw()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_ackEngineData(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
    handle: jni::sys::jlong,
) -> jboolean {
    SessionCoordinator::get().ack_engine_data(session_id as usize, handle) as jboolean
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_rejectEngineData(
    _env: JNIEnv,
    _class: JClass,
    session_id: jint,
    handle: jni::sys::jlong,
) -> jboolean {
    SessionCoordinator::get().reject_engine_data(session_id as usize, handle) as jboolean
}

#[cfg(test)]
mod completion_candidate_tests {
    use super::*;

    #[test]
    fn all_io_terminal_classes_are_retained_and_first_fact_wins() {
        let coordinator = SessionCoordinator::get();
        for io in [
            IoOutcome::Eof,
            IoOutcome::Cancelled,
            IoOutcome::IoError(libc::EIO),
            IoOutcome::ResponseOverflow,
            IoOutcome::Panicked,
        ] {
            let session = coordinator.register_session();
            let observer = coordinator.completion_observer(session).unwrap();
            coordinator.record_process_outcome_for_test(session, ExitOutcome::Exited(7));
            assert!(coordinator.take_completion_candidate(session).is_none());
            coordinator.record_io_outcome(observer, io);
            coordinator.record_io_outcome(observer, IoOutcome::Eof);
            assert_eq!(
                coordinator.completion_facts(session),
                Some((ExitOutcome::Exited(7), io))
            );
            let candidate = coordinator.take_completion_candidate(session).unwrap();
            assert_eq!(candidate.io, io);
            assert_eq!(
                coordinator.completion_facts(session),
                Some((ExitOutcome::Exited(7), io))
            );
            assert!(coordinator.take_completion_candidate(session).is_none());
            coordinator.unregister_session(session);
        }
    }

    #[test]
    fn concurrent_candidate_take_has_one_winner() {
        let coordinator = SessionCoordinator::get();
        let session = coordinator.register_session();
        let observer = coordinator.completion_observer(session).unwrap();
        coordinator.record_process_outcome_for_test(session, ExitOutcome::Lost(libc::ECHILD));
        coordinator.record_io_outcome(observer, IoOutcome::Cancelled);
        let gate = Arc::new(std::sync::Barrier::new(17));
        let mut workers = Vec::new();
        for _ in 0..16 {
            let gate = Arc::clone(&gate);
            workers.push(std::thread::spawn(move || {
                gate.wait();
                SessionCoordinator::get()
                    .take_completion_candidate(session)
                    .is_some()
            }));
        }
        gate.wait();
        let winners = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|won| *won)
            .count();
        assert_eq!(winners, 1);
        coordinator.unregister_session(session);
    }
}
