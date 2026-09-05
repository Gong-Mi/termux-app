//! One coordinator-owned identity per child; no threads or global reaper.
//! All managed child reaping must go through this owner. Without P_PIDFD waits the
//! mutex closes internal reap/signal races, not an unmanaged third-party reaper
//! racing between waitpid and kill. Never create competing owners for one PID.
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

#[cfg(test)]
thread_local! {
    static WAIT_PIDFD_ERROR: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

fn wait_pidfd(fd: i32, info: &mut libc::siginfo_t) -> io::Result<()> {
    #[cfg(test)]
    if let Some(errno) = WAIT_PIDFD_ERROR.with(|error| error.take()) {
        return Err(io::Error::from_raw_os_error(errno));
    }
    let rc = unsafe { libc::waitid(libc::P_PIDFD, fd as libc::id_t,
        info, libc::WEXITED | libc::WNOHANG) };
    if rc < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitOutcome {
    /// Normal exit code, or negative terminating signal.
    Exited(i32),
    /// Ownership/status lost; value is the waitpid errno (not an exit code).
    Lost(i32),
}

struct State {
    outcome: Option<ExitOutcome>,
    pidfd_wait: bool,
}

pub struct ProcessOwner {
    pid: i32,
    pidfd: Option<OwnedFd>,
    state: Mutex<State>,
    changed: Condvar,
}

impl ProcessOwner {
    pub fn claim(pid: i32) -> io::Result<Arc<Self>> {
        Self::claim_inner(pid, false)
    }

    /// Force the old-kernel path, including in real-child regression tests.
    pub fn claim_fallback(pid: i32) -> io::Result<Arc<Self>> {
        Self::claim_inner(pid, true)
    }

    fn claim_inner(pid: i32, fallback: bool) -> io::Result<Arc<Self>> {
        if pid <= 0 {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let pidfd = if fallback {
            None
        } else {
            loop {
                // pidfd_open alone does NOT establish parent/child ownership.
                let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
                if fd >= 0 {
                    break Some(unsafe { OwnedFd::from_raw_fd(fd as i32) });
                }
                if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    break None;
                }
            }
        };
        let pidfd_wait = pidfd.is_some();
        let owner = Arc::new(Self {
            pid,
            pidfd,
            state: Mutex::new(State { outcome: None, pidfd_wait }),
            changed: Condvar::new(),
        });
        {
            let mut state = owner.lock();
            owner.refresh(&mut state);
            if let Some(ExitOutcome::Lost(errno)) = state.outcome {
                return Err(io::Error::from_raw_os_error(errno));
            }
        }
        Ok(owner)
    }

    /// Compatibility label only; callers must not use it to reap or signal.
    pub fn pid(&self) -> i32 { self.pid }

    pub fn has_pidfd(&self) -> bool { self.pidfd.is_some() }

    /// Cached state, refreshed by wait/terminate (not a kernel liveness query).
    pub fn is_running(&self) -> bool { self.outcome().is_none() }

    pub fn outcome(&self) -> Option<ExitOutcome> { self.lock().outcome }

    pub fn has_pidfd_wait(&self) -> bool { self.lock().pidfd_wait }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Must hold the state mutex across every reap and fallback signal.
    fn refresh(&self, state: &mut State) {
        if state.outcome.is_some() { return; }
        if state.pidfd_wait {
            let fd = self.pidfd.as_ref().unwrap().as_raw_fd();
            loop {
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                if let Err(error) = wait_pidfd(fd, &mut info) {
                    let errno = error.raw_os_error().unwrap_or(libc::EIO);
                    if errno == libc::EINTR { continue; }
                    if errno == libc::EINVAL || errno == libc::ENOSYS || errno == libc::EPERM {
                        // Older kernels or syscall policy may reject P_PIDFD
                        // while waitpid remains usable. Keep pidfd signal identity;
                        // a wait-method denial is not evidence of lost ownership.
                        state.pidfd_wait = false;
                        break;
                    }
                    state.outcome = Some(ExitOutcome::Lost(errno));
                } else {
                    if unsafe { info.si_pid() } == 0 { return; }
                    let status = unsafe { info.si_status() };
                    state.outcome = match info.si_code {
                        libc::CLD_EXITED => Some(ExitOutcome::Exited(status)),
                        libc::CLD_KILLED | libc::CLD_DUMPED => Some(ExitOutcome::Exited(-status)),
                        _ => return,
                    };
                }
                self.changed.notify_all();
                return;
            }
        }
        loop {
            let mut status = 0;
            let rc = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if rc == 0 { return; }
            if rc < 0 {
                let errno = io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO);
                if errno == libc::EINTR { continue; }
                state.outcome = Some(ExitOutcome::Lost(errno));
            } else if libc::WIFEXITED(status) {
                state.outcome = Some(ExitOutcome::Exited(libc::WEXITSTATUS(status)));
            } else if libc::WIFSIGNALED(status) {
                state.outcome = Some(ExitOutcome::Exited(-libc::WTERMSIG(status)));
            } else { return; }
            self.changed.notify_all();
            return;
        }
    }

    /// SIGKILL only; false means a terminal outcome was already observed.
    /// A pidfd signaling failure is returned, never retried via a raw PID.
    pub fn terminate(&self) -> io::Result<bool> {
        let mut state = self.lock();
        self.refresh(&mut state);
        if state.outcome.is_some() { return Ok(false); }
        loop {
            let rc = unsafe {
                match &self.pidfd {
                    Some(fd) => libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        fd.as_raw_fd(), libc::SIGKILL,
                        std::ptr::null::<libc::siginfo_t>(), 0u32,
                    ) as i32,
                    None => libc::kill(self.pid, libc::SIGKILL),
                }
            };
            if rc == 0 { return Ok(true); }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EINTR) {
                return Err(error);
            }
        }
    }

    /// Blocking worker/test API. Multiple callers return the same cached result.
    pub fn wait(&self) -> ExitOutcome {
        loop {
            let mut state = self.lock();
            self.refresh(&mut state);
            if let Some(outcome) = state.outcome { return outcome; }
            if let Some(fd) = &self.pidfd {
                // Never retain the state lock while blocking in poll. A bounded
                // poll also lets concurrent waiters observe cached Lost outcomes.
                drop(state);
                let mut pfd = libc::pollfd {
                    fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0,
                };
                let rc = unsafe { libc::poll(&mut pfd, 1, 100) };
                if rc < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    // Poll failure is not lost child ownership. Retain waitpid
                    // authority and throttle retry without changing signal route.
                    let guard = self.lock();
                    if let Some(outcome) = guard.outcome { return outcome; }
                    drop(self.changed.wait_timeout(guard, Duration::from_millis(100))
                        .unwrap_or_else(|e| e.into_inner()));
                }
            } else {
                drop(self.changed.wait_timeout(state, Duration::from_millis(100))
                    .unwrap_or_else(|e| e.into_inner()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn denied_pidfd_wait_retains_child_and_uses_serialized_waitpid() {
        let mut child = Command::new("sh").args(["-c", "read -r value; exit 29"])
            .stdin(Stdio::piped()).spawn().unwrap();
        WAIT_PIDFD_ERROR.with(|error| error.set(Some(libc::EPERM)));
        let result = ProcessOwner::claim(child.id() as i32);
        WAIT_PIDFD_ERROR.with(|error| error.set(None));
        if result.is_err() {
            drop(child.stdin.take());
            child.wait().unwrap();
        }
        let owner = result.expect("EPERM on pidfd wait must retain a waitable child owner");
        if !owner.has_pidfd() {
            eprintln!("SKIP pidfd-denial injection: pidfd_open unavailable");
            drop(child.stdin.take()); owner.wait(); return;
        }
        assert!(!owner.has_pidfd_wait());
        assert!(owner.is_running());
        drop(child.stdin.take());
        assert_eq!(owner.wait(), ExitOutcome::Exited(29));
        assert!(!owner.terminate().unwrap());
        assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
        eprintln!("PASS injected EPERM: pidfd identity retained, waitpid reaped exactly once");
    }

    #[test]
    fn pidfd_wait_does_not_follow_a_reassigned_numeric_label() {
        let mut original = Command::new("sh").args(["-c", "read -r value; exit 31"])
            .stdin(Stdio::piped()).spawn().unwrap();
        let mut owner = ProcessOwner::claim(original.id() as i32).unwrap();
        if !owner.has_pidfd_wait() {
            eprintln!("SKIP pidfd label-injection: pidfd unavailable; fallback tested separately");
            drop(original.stdin.take()); owner.wait(); return;
        }
        let mut unrelated = Command::new("sh").args(["-c", "sleep 0.1; exit 77"]).spawn().unwrap();
        // Model a stale/reassigned numeric PID without forcing OS PID reuse.
        // The owned pidfd must remain the authoritative reaping identity.
        Arc::get_mut(&mut owner).unwrap().pid = unrelated.id() as i32;
        drop(original.stdin.take());
        assert_eq!(owner.wait(), ExitOutcome::Exited(31));
        assert_eq!(unrelated.wait().unwrap().code(), Some(77));
        eprintln!("PASS pidfd identity ignores reassigned numeric label");
    }
}
