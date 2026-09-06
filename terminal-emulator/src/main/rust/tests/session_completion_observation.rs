//! Real context/process/IO observations; no UI completion or final-present claim.
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use termux_rust::engine::{TerminalContext, TerminalEngine};
use termux_rust::process_owner::{ExitOutcome, ProcessOwner};

fn wait_for(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "observation did not arrive");
        thread::sleep(Duration::from_millis(1));
    }
}
fn context(process: Arc<ProcessOwner>) -> Arc<TerminalContext> {
    Arc::new(TerminalContext::with_process(TerminalEngine::new(0, 80, 24, 2000, 8, 16), process))
}
fn child() -> std::process::Child {
    Command::new("sh").args(["-c", "read -r value; exit 23"])
        .stdin(Stdio::piped()).spawn().unwrap()
}

#[test]
fn no_process_no_io_is_not_reported_as_completed() {
    let context = TerminalContext::new(TerminalEngine::new(0, 80, 24, 2000, 8, 16));
    assert_eq!(context.completion_status(), [0, 0, 0, 0]);
}

#[test]
fn process_exit_does_not_finish_io_and_eof_preserves_utf8_tail() {
    let mut child = child();
    let owner = ProcessOwner::claim(child.id() as i32).unwrap();
    let context = context(owner.clone());
    let (master, mut peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(context.clone(), master.into()).unwrap();
    assert_eq!(context.completion_status(), [1, 0, 1, 0]);
    drop(child.stdin.take());
    assert_eq!(owner.wait(), ExitOutcome::Exited(23));
    assert_eq!(context.completion_status(), [2, 23, 1, 0]);
    let text = "tail-中文".as_bytes();
    peer.write_all(&text[..6]).unwrap();
    peer.write_all(&text[6..]).unwrap();
    drop(peer);
    wait_for(|| context.completion_status()[2] == 2);
    assert_eq!(context.completion_status(), [2, 23, 2, 0]);
    let mut row = [0u16; 80];
    context.lock.read().unwrap().state.copy_row_text(0, &mut row);
    assert!(String::from_utf16_lossy(&row).starts_with("tail-中文"));
    TerminalContext::stop_io(&context);
    wait_for(|| context.io_is_joined());
    assert_eq!(context.completion_status(), [2, 23, 2, 0]);
    assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
}

#[test]
fn io_eof_does_not_invent_process_exit() {
    let mut child = child();
    let owner = ProcessOwner::claim(child.id() as i32).unwrap();
    let context = context(owner.clone());
    let (master, peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(context.clone(), master.into()).unwrap();
    drop(peer);
    wait_for(|| context.completion_status()[2] == 2);
    assert_eq!(context.completion_status(), [1, 0, 2, 0]);
    drop(child.stdin.take());
    assert_eq!(owner.wait(), ExitOutcome::Exited(23));
    assert_eq!(context.completion_status(), [2, 23, 2, 0]);
    TerminalContext::stop_io(&context);
    wait_for(|| context.io_is_joined());
    assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
}

#[test]
fn cancelled_io_stays_cancelled_after_join_and_process_exit() {
    let mut child = child();
    let owner = ProcessOwner::claim(child.id() as i32).unwrap();
    let context = context(owner.clone());
    let (master, _peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(context.clone(), master.into()).unwrap();
    TerminalContext::stop_io(&context);
    wait_for(|| context.io_is_joined());
    assert_eq!(context.completion_status(), [1, 0, 3, 0]);
    drop(child.stdin.take());
    assert_eq!(owner.wait(), ExitOutcome::Exited(23));
    assert_eq!(context.completion_status(), [2, 23, 3, 0]);
    assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
}
