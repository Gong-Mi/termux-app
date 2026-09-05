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
