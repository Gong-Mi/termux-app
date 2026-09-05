//! Production TerminalContext parsing/response/admission/reaper integration.
//! Host sockets exercise real syscalls; PTY termios/resize is in pty_io_runtime.
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termux_rust::engine::{TerminalContext, TerminalEngine};
use termux_rust::engine::context::INPUT_CAPACITY;
use termux_rust::engine::io_runtime::SubmitError;

fn context() -> Arc<TerminalContext> {
    Arc::new(TerminalContext::new(TerminalEngine::new(0, 80, 24, 2000, 8, 16)))
}

fn joined(context: &Arc<TerminalContext>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !context.io_is_joined() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(context.io_is_joined(), "background join did not complete");
}

#[test]
fn real_parser_responses_and_user_input_use_worker_and_cancel_silent_peer() {
    let context = context();
    let (master, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    TerminalContext::start_io_owned(Arc::clone(&context), master.into()).unwrap();
    peer.write_all(b"hello\x1b[6n").unwrap();
    let mut reply = [0; 6];
    peer.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"\x1b[1;6R");
    let mut row = [0u16; 80];
    context.lock.read().unwrap().state.copy_row_text(0, &mut row);
    assert!(String::from_utf16_lossy(&row).starts_with("hello"));
    assert_eq!(context.submit_input(&vec![b'x'; INPUT_CAPACITY + 1]), Err(SubmitError::Full));
    context.submit_input(b"exact-input").unwrap();
    let mut bytes = [0; 11];
    peer.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"exact-input");
    // Peer stays open and silent: closing the UI owner must wake poll.
    TerminalContext::stop_io(&context);
    assert_eq!(context.submit_input(b"after-close"), Err(SubmitError::Closed));
    joined(&context);
    assert_eq!(peer.read(&mut bytes).unwrap(), 0);
}

#[test]
fn repeated_start_rejects_and_closes_new_owner_without_replacing_original() {
    let context = context();
    let (master, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    TerminalContext::start_io_owned(Arc::clone(&context), master.into()).unwrap();
    let (other, mut other_peer) = UnixStream::pair().unwrap();
    let fd = other.as_raw_fd();
    assert!(TerminalContext::start_io_owned(Arc::clone(&context), other.into()).is_err());
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
    assert_eq!(other_peer.read(&mut [0; 1]).unwrap(), 0);
    context.submit_input(b"a").unwrap();
    let mut byte = [0];
    peer.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [b'a']);
    TerminalContext::stop_io(&context);
    TerminalContext::stop_io(&context);
    joined(&context);
    let (late, mut late_peer) = UnixStream::pair().unwrap();
    assert!(TerminalContext::start_io_owned(Arc::clone(&context), late.into()).is_err());
    assert_eq!(late_peer.read(&mut [0; 1]).unwrap(), 0);
}

#[test]
fn dropping_context_cancels_worker_without_a_strong_reference_cycle() {
    let context = context();
    let weak = Arc::downgrade(&context);
    let (master, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    TerminalContext::start_io_owned(Arc::clone(&context), master.into()).unwrap();
    drop(context);
    assert!(weak.upgrade().is_none());
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
}

#[test]
fn startup_fd_configuration_failure_closes_owner_and_allows_retry() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let context = context();
    let raw = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    assert!(raw >= 0);
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    assert!(TerminalContext::start_io_owned(Arc::clone(&context), fd).is_err());
    assert!(context.io_is_joined());
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
    let (master, _peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(Arc::clone(&context), master.into()).unwrap();
    TerminalContext::stop_io(&context); joined(&context);
}
