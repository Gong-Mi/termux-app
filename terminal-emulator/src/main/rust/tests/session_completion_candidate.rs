use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use termux_rust::coordinator::{CompletionCandidate, SessionCoordinator};
use termux_rust::engine::{TerminalContext, TerminalEngine};

fn wait_for_candidate(coordinator: &SessionCoordinator, session: usize) -> CompletionCandidate {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(candidate) = coordinator.take_completion_candidate(session) {
            return candidate;
        }
        assert!(Instant::now() < deadline, "completion candidate was not published");
        thread::yield_now();
    }
}

#[test]
fn real_process_owner_and_real_io_publish_one_candidate_in_either_order() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let child = Command::new("sh").arg("-c").arg("exit 23").spawn().unwrap();
    let process = coordinator.bind_pid(session, child.id() as i32).unwrap();

    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        session as i32, 80, 24, 2000, 8, 16,
    )));
    let (master, peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(Arc::clone(&context), OwnedFd::from(master)).unwrap();
    drop(peer);

    let candidate = wait_for_candidate(coordinator, session);
    assert_eq!(candidate.process, process.outcome().unwrap());
    assert_eq!(candidate.io, termux_rust::engine::io_runtime::IoOutcome::Eof);
    assert!(coordinator.take_completion_candidate(session).is_none());
    coordinator.unregister_session(session);
}

#[test]
fn unregister_makes_late_io_fact_a_no_op() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    coordinator.unregister_session(session);
    assert!(coordinator.take_completion_candidate(session).is_none());
}

#[test]
fn real_context_cancellation_preserves_cancelled_io_outcome() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let child = Command::new("sh").arg("-c").arg("sleep 1").spawn().unwrap();
    let process = coordinator.bind_pid(session, child.id() as i32).unwrap();
    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        session as i32, 80, 24, 2000, 8, 16,
    )));
    let (master, _peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(Arc::clone(&context), OwnedFd::from(master)).unwrap();
    TerminalContext::stop_io(&context);
    let deadline = Instant::now() + Duration::from_secs(3);
    while coordinator.completion_facts(session).is_none()
        && Instant::now() < deadline
    {
        thread::yield_now();
    }
    assert_eq!(
        coordinator.completion_facts(session),
        Some((
            process.outcome().unwrap(),
            termux_rust::engine::io_runtime::IoOutcome::Cancelled,
        ))
    );
    let candidate = wait_for_candidate(coordinator, session);
    assert_eq!(candidate.io, termux_rust::engine::io_runtime::IoOutcome::Cancelled);
    coordinator.unregister_session(session);
}

#[test]
fn panic_on_stop_is_reported_without_changing_join_panic_semantics() {
    use std::sync::mpsc::channel;
    use termux_rust::engine::io_runtime::{IoOutcome, IoRuntime};

    let (master, mut peer) = UnixStream::pair().unwrap();
    let (sender, receiver) = channel();
    let mut runtime = IoRuntime::start_with_callbacks(
        OwnedFd::from(master),
        4096,
        |_| panic!("injected callback panic"),
        || {},
        move |outcome| sender.send(IoOutcome::from(outcome)).unwrap(),
    )
    .unwrap();
    peer.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    peer.write_all(b"panic").unwrap();
    let join = runtime.join();
    assert!(join.is_err());
    assert_eq!(receiver.recv_timeout(Duration::from_secs(3)).unwrap(), IoOutcome::Panicked);
    assert_eq!(runtime.observer().outcome(), Some(IoOutcome::Panicked));
}
