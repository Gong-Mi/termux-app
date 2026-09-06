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
        assert!(
            Instant::now() < deadline,
            "completion candidate was not published"
        );
        thread::yield_now();
    }
}

#[test]
fn real_process_exit_then_io_eof_publish_one_candidate() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let observer = coordinator.completion_observer(session).unwrap();
    let child = Command::new("sh").arg("-c").arg("exit 23").spawn().unwrap();
    let process = coordinator.bind_pid(session, child.id() as i32).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while process.outcome().is_none() && Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(
        process.outcome(),
        Some(termux_rust::process_owner::ExitOutcome::Exited(23))
    );

    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        session as i32,
        80,
        24,
        2000,
        8,
        16,
    )));
    let (master, peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned_for_session(
        Arc::clone(&context),
        OwnedFd::from(master),
        observer,
    )
    .unwrap();
    drop(peer);

    let candidate = wait_for_candidate(coordinator, session);
    assert_eq!(candidate.process, process.outcome().unwrap());
    assert_eq!(
        candidate.io,
        termux_rust::engine::io_runtime::IoOutcome::Eof
    );
    assert!(coordinator.take_completion_candidate(session).is_none());
    coordinator.unregister_session(session);
}

#[test]
fn real_io_eof_then_process_exit_publish_one_candidate() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let observer = coordinator.completion_observer(session).unwrap();
    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        session as i32,
        80,
        24,
        2000,
        8,
        16,
    )));
    let (master, peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned_for_session(
        Arc::clone(&context),
        OwnedFd::from(master),
        observer,
    )
    .unwrap();
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(3);
    while context.completion_status()[2] != 2 && Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(context.completion_status()[2], 2);

    let child = Command::new("sh").arg("-c").arg("exit 29").spawn().unwrap();
    let process = coordinator.bind_pid(session, child.id() as i32).unwrap();
    let candidate = wait_for_candidate(coordinator, session);
    assert_eq!(candidate.process, process.outcome().unwrap());
    assert_eq!(
        candidate.io,
        termux_rust::engine::io_runtime::IoOutcome::Eof
    );
    assert!(coordinator.take_completion_candidate(session).is_none());
    coordinator.unregister_session(session);
}

#[test]
fn unregister_makes_late_real_io_producer_a_no_op() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let observer = coordinator.completion_observer(session).unwrap();
    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        session as i32,
        80,
        24,
        2000,
        8,
        16,
    )));
    let (master, peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned_for_session(
        Arc::clone(&context),
        OwnedFd::from(master),
        observer,
    )
    .unwrap();
    coordinator.unregister_session(session);
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(3);
    while context.completion_status()[2] != 2 && Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(context.completion_status()[2], 2);
    assert!(coordinator.take_completion_candidate(session).is_none());
}

#[test]
fn standalone_engine_zero_cannot_publish_to_managed_session_zero() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    if session != 0 {
        coordinator.unregister_session(session);
        return;
    }
    let child = Command::new("sh").arg("-c").arg("exit 17").spawn().unwrap();
    let process = coordinator.bind_pid(session, child.id() as i32).unwrap();
    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        0, 80, 24, 2000, 8, 16,
    )));
    let (master, peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(Arc::clone(&context), OwnedFd::from(master)).unwrap();
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(3);
    while (process.outcome().is_none() || context.completion_status()[2] != 2)
        && Instant::now() < deadline
    {
        thread::yield_now();
    }
    assert!(process.outcome().is_some());
    assert_eq!(context.completion_status()[2], 2);
    assert!(coordinator.take_completion_candidate(session).is_none());
    coordinator.unregister_session(session);
}

#[test]
fn real_context_cancellation_preserves_cancelled_io_outcome() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let observer = coordinator.completion_observer(session).unwrap();
    let child = Command::new("sh").arg("-c").arg("sleep 1").spawn().unwrap();
    let process = coordinator.bind_pid(session, child.id() as i32).unwrap();
    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        session as i32,
        80,
        24,
        2000,
        8,
        16,
    )));
    let (master, _peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned_for_session(
        Arc::clone(&context),
        OwnedFd::from(master),
        observer,
    )
    .unwrap();
    TerminalContext::stop_io(&context);
    let deadline = Instant::now() + Duration::from_secs(3);
    while coordinator.completion_facts(session).is_none() && Instant::now() < deadline {
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
    assert_eq!(
        candidate.io,
        termux_rust::engine::io_runtime::IoOutcome::Cancelled
    );
    coordinator.unregister_session(session);
}

#[test]
fn panic_on_bytes_reports_before_join_without_changing_join_panic_semantics() {
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
    peer.set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    peer.write_all(b"panic").unwrap();
    let join = runtime.join();
    assert!(join.is_err());
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        IoOutcome::Panicked
    );
    assert_eq!(runtime.observer().outcome(), Some(IoOutcome::Panicked));
}

#[test]
fn panic_after_read_reports_before_join_without_changing_join_panic_semantics() {
    use std::sync::mpsc::channel;
    use termux_rust::engine::io_runtime::{IoOutcome, IoRuntime};

    let (master, mut peer) = UnixStream::pair().unwrap();
    let (sender, receiver) = channel();
    let mut runtime = IoRuntime::start_with_callbacks(
        OwnedFd::from(master),
        4096,
        |_| vec![],
        || panic!("injected after_read panic"),
        move |outcome| sender.send(IoOutcome::from(outcome)).unwrap(),
    )
    .unwrap();
    peer.write_all(b"after-read-panic").unwrap();
    let join = runtime.join();
    assert!(join.is_err());
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        IoOutcome::Panicked
    );
    assert_eq!(runtime.observer().outcome(), Some(IoOutcome::Panicked));
}

#[test]
fn panic_in_on_stop_is_not_retried_or_reported_twice() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use termux_rust::engine::io_runtime::{IoOutcome, IoRuntime};

    let (master, peer) = UnixStream::pair().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    let mut runtime = IoRuntime::start_with_callbacks(
        OwnedFd::from(master),
        4096,
        |_| vec![],
        || {},
        move |_| {
            observed_calls.fetch_add(1, Ordering::SeqCst);
            panic!("injected on_stop panic");
        },
    )
    .unwrap();
    drop(peer);
    assert!(runtime.join().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.observer().outcome(), Some(IoOutcome::Eof));
}

#[test]
fn completion_bridge_replays_early_facts_once_outside_registry_lock() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let observer = coordinator.completion_observer(session).unwrap();
    let mut child = Command::new("sh").arg("-c").arg("exit 31").spawn().unwrap();
    coordinator.bind_pid(session, child.id() as i32).unwrap();
    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        0, 80, 24, 2000, 8, 16,
    )));
    let (master, peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned_for_session(
        Arc::clone(&context),
        OwnedFd::from(master),
        observer,
    )
    .unwrap();
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(3);
    while coordinator.completion_facts(session).is_none() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    assert!(coordinator.install_completion_sink(
        session,
        Arc::new(move |candidate| {
            assert!(SessionCoordinator::get().has_session(session)); // reentrant, no registry lock
            assert_eq!(
                candidate.process,
                termux_rust::process_owner::ExitOutcome::Exited(31)
            );
            seen.fetch_add(1, Ordering::SeqCst);
            true
        })
    ));
    assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(coordinator.completion_dispatch_status(session), Some(2));
    assert!(!coordinator.install_completion_sink(session, Arc::new(|_| panic!("duplicate sink"))));
    assert!(coordinator.take_completion_candidate(session).is_none());
    coordinator.unregister_session(session);
    assert_eq!(coordinator.completion_dispatch_status(session), None);
}

#[test]
fn completion_bridge_failure_is_not_retried_and_can_unregister_in_callback() {
    for remove in [false, true] {
        let coordinator = SessionCoordinator::get();
        let session = coordinator.register_session();
        let observer = coordinator.completion_observer(session).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(coordinator.install_completion_sink(
            session,
            Arc::new(move |_| {
                if remove {
                    SessionCoordinator::get().unregister_session(session);
                }
                tx.send(()).unwrap();
                false
            })
        ));
        assert!(coordinator.take_completion_candidate(session).is_none());
        let mut child = Command::new("sh").arg("-c").arg("exit 32").spawn().unwrap();
        coordinator.bind_pid(session, child.id() as i32).unwrap();
        let context = Arc::new(TerminalContext::new(TerminalEngine::new(
            0, 80, 24, 2000, 8, 16,
        )));
        let (master, peer) = UnixStream::pair().unwrap();
        TerminalContext::start_io_owned_for_session(
            Arc::clone(&context),
            OwnedFd::from(master),
            observer,
        )
        .unwrap();
        drop(peer);
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
        let deadline = Instant::now() + Duration::from_secs(3);
        while coordinator.completion_dispatch_status(session) == Some(1) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(
            coordinator.completion_dispatch_status(session),
            if remove { None } else { Some(3) }
        );
        assert!(coordinator.take_completion_candidate(session).is_none());
        assert!(!coordinator.install_completion_sink(session, Arc::new(|_| true)));
        assert!(rx.try_recv().is_err());
        coordinator.unregister_session(session);
    }
}
