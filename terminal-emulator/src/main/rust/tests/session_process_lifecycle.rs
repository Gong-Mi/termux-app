//! Real process ownership regression tests, isolated from other test children.
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use termux_rust::coordinator::{SessionCoordinator, SessionState};

fn isolated(name: &str) -> bool {
    if std::env::var_os("TERMUX_PROCESS_TEST_CHILD").is_some() { return false; }
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--test-threads=1"])
        .env("TERMUX_PROCESS_TEST_CHILD", "1").output().unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    true
}
fn shell(command: &str) -> Command {
    let path = if std::path::Path::new("/system/bin/sh").exists() { "/system/bin/sh" } else { "/bin/sh" };
    let mut cmd = Command::new(path);
    cmd.args(["-c", command]).stdout(Stdio::null()).stderr(Stdio::null());
    cmd
}
fn finished(coordinator: &SessionCoordinator, session: usize) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while coordinator.get_session_state(session) != Some(SessionState::Finished) {
        assert!(Instant::now() < deadline, "registered process exit was lost");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn unrelated_child_remains_waitable_by_its_real_owner() {
    if isolated("unrelated_child_remains_waitable_by_its_real_owner") { return; }
    let mut child = shell("exit 7").spawn().unwrap();
    SessionCoordinator::get();
    // Give the historical wildcard monitor a chance to reap the existing child.
    thread::sleep(Duration::from_millis(600));
    assert_eq!(child.wait().expect("coordinator stole an unrelated child").code(), Some(7));
}

#[test]
fn exit_before_bind_is_retained_not_overwritten_with_running() {
    if isolated("exit_before_bind_is_retained_not_overwritten_with_running") { return; }
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let child = shell("exit 23").spawn().unwrap();
    thread::sleep(Duration::from_millis(650));
    let _ = coordinator.bind_pid(session, child.id() as i32);
    finished(coordinator, session);
    coordinator.unregister_session(session);
}

#[test]
fn late_bind_cannot_resurrect_unregistered_session() {
    if isolated("late_bind_cannot_resurrect_unregistered_session") { return; }
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    coordinator.unregister_session(session);
    let mut child = shell("exit 0").spawn().unwrap();
    let _ = coordinator.bind_pid(session, child.id() as i32);
    assert_eq!(coordinator.get_session_state(session), None);
    let _ = child.wait();
}

#[test]
fn pkg_release_cannot_turn_exited_process_back_into_running() {
    if isolated("pkg_release_cannot_turn_exited_process_back_into_running") { return; }
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let mut child = shell("read -r value; exit 0").stdin(Stdio::piped()).spawn().unwrap();
    let _ = coordinator.bind_pid(session, child.id() as i32);
    assert!(coordinator.try_acquire_pkg_lock(session));
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    finished(coordinator, session);
    coordinator.release_pkg_lock(session);
    assert_eq!(coordinator.get_session_state(session), Some(SessionState::Finished));
    coordinator.unregister_session(session);
}

#[test]
fn terminate_before_bind_is_applied_to_the_owned_child() {
    if isolated("terminate_before_bind_is_applied_to_the_owned_child") { return; }
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    assert_eq!(coordinator.process_status(session), Some([0, 0, 0]));
    assert!(coordinator.terminate_session(session).unwrap());
    let child = shell("read -r value; exit 0").stdin(Stdio::piped()).spawn().unwrap();
    let owner = coordinator.bind_pid(session, child.id() as i32).unwrap();
    assert_eq!(owner.wait(), termux_rust::process_owner::ExitOutcome::Exited(-libc::SIGKILL));
    finished(coordinator, session);
    assert_eq!(coordinator.process_status(session), Some([2, -1, -libc::SIGKILL]));
    assert!(!coordinator.terminate_session(session).unwrap());
    coordinator.unregister_session(session);
    assert!(!coordinator.terminate_session(session).unwrap());
}

#[test]
fn unregister_does_not_allow_a_second_owner_to_steal_the_live_child() {
    if isolated("unregister_does_not_allow_a_second_owner_to_steal_the_live_child") { return; }
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let other = coordinator.register_session();
    let child = shell("read -r value; exit 0").stdin(Stdio::piped()).spawn().unwrap();
    let pid = child.id() as i32;
    let owner = coordinator.bind_pid(session, pid).unwrap();
    coordinator.unregister_session(session);
    assert!(coordinator.bind_pid(other, pid).is_err());
    assert!(owner.terminate().unwrap());
    assert_eq!(owner.wait(), termux_rust::process_owner::ExitOutcome::Exited(-libc::SIGKILL));
    assert_eq!(coordinator.get_session_state(session), None);
    assert_eq!(coordinator.get_session_state(other), Some(SessionState::Idle));
    coordinator.unregister_session(other);
}

#[test]
fn legacy_managed_waits_return_cached_status_without_double_reaping() {
    if isolated("legacy_managed_waits_return_cached_status_without_double_reaping") { return; }
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let mut child = shell("read -r value; exit 19").stdin(Stdio::piped()).spawn().unwrap();
    let pid = child.id() as i32;
    let owner = coordinator.bind_pid(session, pid).unwrap();
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    assert_eq!(termux_rust::pty::wait_for(pid), 19);
    assert_eq!(termux_rust::pty::wait_for(pid), 19);
    assert_eq!(owner.wait(), termux_rust::process_owner::ExitOutcome::Exited(19));
    finished(coordinator, session);
    assert!(!coordinator.try_acquire_pkg_lock(session));
    coordinator.unregister_session(session);
}

#[test]
fn known_exit_rejects_new_input_but_reader_keeps_parsing_tail() {
    if isolated("known_exit_rejects_new_input_but_reader_keeps_parsing_tail") { return; }
    use termux_rust::engine::{TerminalContext, TerminalEngine};
    use termux_rust::engine::io_runtime::SubmitError;
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    let child = shell("exit 0").spawn().unwrap();
    let process = termux_rust::process_owner::ProcessOwner::claim(child.id() as i32).unwrap();
    let context = Arc::new(TerminalContext::with_process(TerminalEngine::new(0,80,24,2000,8,16), process.clone()));
    let (master, mut peer) = UnixStream::pair().unwrap();
    TerminalContext::start_io_owned(context.clone(), master.into()).unwrap();
    assert_eq!(process.wait(), termux_rust::process_owner::ExitOutcome::Exited(0));
    assert_eq!(context.submit_input(b"late"), Err(SubmitError::Closed));
    peer.write_all(b"tail-after-exit").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut row = [0u16; 80];
        context.lock.read().unwrap().state.copy_row_text(0, &mut row);
        if String::from_utf16_lossy(&row).starts_with("tail-after-exit") { break; }
        assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(1));
    }
    assert!(!context.io_is_joined(), "process exit is not IO shutdown");
    TerminalContext::stop_io(&context);
    while !context.io_is_joined() { assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(1)); }
}

#[test]
fn late_polling_delivery_reclaims_owner_instead_of_resurrecting_session() {
    if isolated("late_polling_delivery_reclaims_owner_instead_of_resurrecting_session") { return; }
    use termux_rust::engine::{TerminalContext, TerminalEngine, ENGINE_HANDLES};
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    coordinator.unregister_session(session);
    let context = std::sync::Arc::new(TerminalContext::new(TerminalEngine::new(0,80,24,2000,8,16)));
    let handle = ENGINE_HANDLES.insert(context).unwrap();
    coordinator.set_engine_data(session, termux_rust::coordinator::SessionEngineData { ptr: handle, pty_fd: -1, pid: -1 });
    assert!(ENGINE_HANDLES.acquire(handle).is_none());
    assert!(coordinator.take_engine_data(session).is_none());
    assert_eq!(coordinator.get_session_state(session), None);
}
