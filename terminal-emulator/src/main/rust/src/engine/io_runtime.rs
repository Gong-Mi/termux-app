//! Single-owner, cancellable Linux/Android PTY transport (std + libc only).
//!
//! `submit` acknowledges admission, not delivery. EOF/cancellation/errors discard
//! unsent output. Capacity includes in-flight unwritten bytes and parser replies.
//! The callback runs without runtime locks, but must return for shutdown to finish:
//! cancellation interrupts OS IO waits, not arbitrary callback code.
//! O_NONBLOCK changes the open file description: callers must remove all other
//! readers/writers, including duplicated descriptors, before transferring ownership.
use std::{
    collections::VecDeque,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    Closed,
    Full,
}

#[derive(Debug)]
pub enum StopOutcome {
    Eof,
    Cancelled,
    IoError(io::Error),
    ResponseOverflow,
}

struct Pending {
    queue: VecDeque<Vec<u8>>,
    // Includes the worker-local head's unwritten suffix.
    bytes: usize,
    resize: Option<libc::winsize>,
}
struct Shared {
    pending: Mutex<Pending>,
    capacity: usize,
    closed: AtomicBool,
    stopped: AtomicBool,
    wake: OwnedFd,
}
impl Shared {
    fn wake(&self) {
        let one = 1u64;
        loop {
            let n = unsafe { libc::write(self.wake.as_raw_fd(), (&one as *const u64).cast(), 8) };
            if n >= 0 {
                return;
            }
            if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                // EAGAIN means eventfd already readable. Sole owned fd is valid.
                return;
            }
        }
    }
}

pub struct IoRuntime {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<StopOutcome>>,
}
impl IoRuntime {
    pub fn start(
        fd: OwnedFd,
        capacity: usize,
        on_bytes: impl FnMut(&[u8]) -> Vec<Vec<u8>> + Send + 'static,
    ) -> io::Result<Self> {
        Self::start_with_callbacks(fd, capacity, on_bytes, || {}, |_| {})
    }

    /// Enqueue parser responses before after_read callbacks may re-enter submit.
    /// on_stop observes the terminal outcome after fd closure, not only at join.
    pub fn start_with_callbacks(
        fd: OwnedFd,
        capacity: usize,
        on_bytes: impl FnMut(&[u8]) -> Vec<Vec<u8>> + Send + 'static,
        after_read: impl FnMut() + Send + 'static,
        on_stop: impl FnOnce(&StopOutcome) + Send + 'static,
    ) -> io::Result<Self> {
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let wake = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if wake < 0 {
            return Err(io::Error::last_os_error());
        }
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending {
                queue: VecDeque::new(),
                bytes: 0,
                resize: None,
            }),
            capacity,
            closed: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            wake: unsafe { OwnedFd::from_raw_fd(wake) },
        });
        let state = shared.clone();
        let worker = thread::Builder::new()
            .name("pty-io".into())
            .spawn(move || {
                let owner = WorkerOwner {
                    fd: Some(fd),
                    shared: state,
                };
                let outcome = run(&owner, on_bytes, after_read);
                drop(owner);
                on_stop(&outcome);
                outcome
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn submit(&self, data: &[u8]) -> Result<(), SubmitError> {
        let mut pending = self.shared.pending.lock().unwrap();
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(SubmitError::Closed);
        }
        if data.len() > self.shared.capacity - pending.bytes {
            return Err(SubmitError::Full);
        }
        if !data.is_empty() {
            pending.queue.push_back(data.to_vec());
            pending.bytes += data.len();
        }
        drop(pending);
        self.shared.wake();
        Ok(())
    }

    /// Last pending resize wins. Only the worker touches the PTY descriptor.
    pub fn resize(
        &self,
        rows: u16,
        cols: u16,
        xpixel: u16,
        ypixel: u16,
    ) -> Result<(), SubmitError> {
        let mut pending = self.shared.pending.lock().unwrap();
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(SubmitError::Closed);
        }
        pending.resize = Some(libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: xpixel,
            ws_ypixel: ypixel,
        });
        drop(pending);
        self.shared.wake();
        Ok(())
    }

    /// Revokes admission and wakes IO; never waits for worker/callback completion.
    pub fn cancel(&self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.wake();
    }
    /// True only after the worker has closed its PTY, including callback unwind.
    pub fn is_stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Acquire)
    }
    /// Background/test use only. A second join is a programming error and panics.
    pub fn join(&mut self) -> thread::Result<StopOutcome> {
        self.worker
            .take()
            .expect("PTY worker already joined")
            .join()
    }
}
impl Drop for IoRuntime {
    fn drop(&mut self) {
        self.cancel();
    }
}

// This guard also closes/publishes stopped on callback panic. No Arc cycle:
// worker owns Shared, not IoRuntime or its JoinHandle.
struct WorkerOwner {
    fd: Option<OwnedFd>,
    shared: Arc<Shared>,
}
impl Drop for WorkerOwner {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        drop(self.fd.take());
        // Reclaim accepted but undelivered bytes even if IoRuntime is retained.
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        pending.queue.clear();
        pending.bytes = 0;
        pending.resize = None;
        drop(pending);
        self.shared.stopped.store(true, Ordering::Release);
    }
}
fn retryable(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::EINTR) | Some(libc::EAGAIN))
}
fn run(
    owner: &WorkerOwner,
    mut on_bytes: impl FnMut(&[u8]) -> Vec<Vec<u8>>,
    mut after_read: impl FnMut(),
) -> StopOutcome {
    let fd = owner.fd.as_ref().unwrap().as_raw_fd();
    let shared = &owner.shared;
    let tty = unsafe { libc::isatty(fd) } == 1;
    let mut head: Option<(Vec<u8>, usize)> = None;
    let mut input = [0u8; 16 * 1024];
    loop {
        if shared.closed.load(Ordering::Acquire) {
            return StopOutcome::Cancelled;
        }
        let resize = shared.pending.lock().unwrap().resize.take();
        if let Some(size) = resize
            && unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) } < 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                // Preserve interrupted command unless a newer one superseded it.
                shared.pending.lock().unwrap().resize.get_or_insert(size);
                continue;
            }
            return StopOutcome::IoError(error);
        }
        if head.is_none() {
            head = shared
                .pending
                .lock()
                .unwrap()
                .queue
                .pop_front()
                .map(|v| (v, 0));
        }
        let mut polls = [
            libc::pollfd {
                fd,
                events: libc::POLLIN | if head.is_some() { libc::POLLOUT } else { 0 },
                revents: 0,
            },
            libc::pollfd {
                fd: shared.wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let result = unsafe { libc::poll(polls.as_mut_ptr(), 2, -1) };
        if shared.closed.load(Ordering::Acquire) {
            return StopOutcome::Cancelled;
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return StopOutcome::IoError(error);
        }
        if polls[1].revents & libc::POLLIN != 0 {
            let mut value = 0u64;
            // Single bounded attempt: EINTR leaves event readable for next turn.
            unsafe {
                libc::read(shared.wake.as_raw_fd(), (&mut value as *mut u64).cast(), 8);
            }
        }
        let ready = polls[0].revents;
        if ready & libc::POLLNVAL != 0 {
            return StopOutcome::IoError(io::Error::from_raw_os_error(libc::EBADF));
        }
        // One read and one <=16KiB write per turn, including EINTR/EAGAIN.
        // HUP still drains readable input before terminal EOF, never retries EOF.
        if ready & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            let n = unsafe { libc::read(fd, input.as_mut_ptr().cast(), input.len()) };
            if n == 0 {
                return StopOutcome::Eof;
            }
            if n < 0 {
                let error = io::Error::last_os_error();
                if tty && error.raw_os_error() == Some(libc::EIO) {
                    return StopOutcome::Eof;
                }
                if !retryable(&error) {
                    return StopOutcome::IoError(error);
                }
            } else {
                if shared.closed.load(Ordering::Acquire) {
                    return StopOutcome::Cancelled;
                }
                let responses = on_bytes(&input[..n as usize]);
                let mut pending = shared.pending.lock().unwrap();
                if shared.closed.load(Ordering::Acquire) {
                    return StopOutcome::Cancelled;
                }
                let total = responses
                    .iter()
                    .try_fold(0usize, |sum, v| sum.checked_add(v.len()));
                let Some(total) = total.filter(|n| *n <= shared.capacity - pending.bytes) else {
                    return StopOutcome::ResponseOverflow;
                };
                pending.bytes += total;
                pending
                    .queue
                    .extend(responses.into_iter().filter(|v| !v.is_empty()));
                drop(pending);
                after_read();
            }
        }
        if shared.closed.load(Ordering::Acquire) {
            return StopOutcome::Cancelled;
        }
        if ready & libc::POLLOUT != 0
            && ready & libc::POLLHUP == 0
            && let Some((bytes, offset)) = head.as_mut()
        {
            let count = (bytes.len() - *offset).min(16 * 1024);
            let n = unsafe { libc::write(fd, bytes[*offset..].as_ptr().cast(), count) };
            if n < 0 {
                let error = io::Error::last_os_error();
                if !retryable(&error) {
                    return StopOutcome::IoError(error);
                }
            } else if n == 0 {
                return StopOutcome::IoError(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "PTY write returned zero",
                ));
            } else {
                *offset += n as usize;
                shared.pending.lock().unwrap().bytes -= n as usize;
                if *offset == bytes.len() {
                    head = None;
                }
            }
        }
    }
}
