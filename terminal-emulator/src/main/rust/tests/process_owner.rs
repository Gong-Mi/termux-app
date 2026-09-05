#[path = "../src/process_owner.rs"]
mod process_owner;

use process_owner::{ExitOutcome, ProcessOwner};
use std::process::{Child, Command};
use std::sync::{Arc, Barrier};
use std::thread;

fn child(script: &str) -> Child {
    Command::new("sh").arg("-c").arg(script).spawn().unwrap()
}

fn zombie(pid: i32) {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    loop {
        let rc = unsafe {
            libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT)
        };
        if rc == 0 { return; }
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
    }
}

#[test]
fn exit_before_claim_is_retained() {
    let c = child("exit 37");
    zombie(c.id() as i32);
    let owner = ProcessOwner::claim(c.id() as i32).unwrap();
    assert_eq!(owner.pid(), c.id() as i32);
    eprintln!("pidfd available: {}", owner.has_pidfd());
    assert_eq!(owner.outcome(), Some(ExitOutcome::Exited(37)));
    assert!(!owner.is_running());
    assert_eq!(owner.wait(), ExitOutcome::Exited(37));
    assert!(!owner.terminate().unwrap());
}

#[test]
fn running_exit_and_multiple_waiters_share_status() {
    for fallback in [false, true] { multiple_waiters(fallback); }
}

fn multiple_waiters(fallback: bool) {
    let mut c = Command::new("sh").args(["-c", "read line; exit 23"])
        .stdin(std::process::Stdio::piped()).spawn().unwrap();
    let owner = if fallback { ProcessOwner::claim_fallback(c.id() as i32) }
        else { ProcessOwner::claim(c.id() as i32) }.unwrap();
    assert!(owner.is_running());
    assert_eq!(owner.outcome(), None);
    let barrier = Arc::new(Barrier::new(5));
    let workers: Vec<_> = (0..4).map(|_| {
        let o = owner.clone();
        let b = barrier.clone();
        thread::spawn(move || { b.wait(); o.wait() })
    }).collect();
    barrier.wait();
    drop(c.stdin.take());
    for worker in workers { assert_eq!(worker.join().unwrap(), ExitOutcome::Exited(23)); }
    assert!(!owner.terminate().unwrap());
}

#[test]
fn signal_status_and_forced_fallback() {
    for fallback in [false, true] {
        let mut c = Command::new("sh").args(["-c", "read line"])
            .stdin(std::process::Stdio::piped()).spawn().unwrap();
        let owner = if fallback { ProcessOwner::claim_fallback(c.id() as i32) }
            else { ProcessOwner::claim(c.id() as i32) }.unwrap();
        if fallback { assert!(!owner.has_pidfd()); }
        assert!(owner.terminate().unwrap());
        assert_eq!(owner.wait(), ExitOutcome::Exited(-libc::SIGKILL));
        assert!(!owner.terminate().unwrap());
        drop(c.stdin.take());
    }
}

#[test]
fn concurrent_exit_wait_and_terminate_keep_one_outcome() {
    for fallback in [false, true] {
        for _ in 0..24 {
            let mut c = Command::new("sh").args(["-c", "read line; exit 17"])
                .stdin(std::process::Stdio::piped()).spawn().unwrap();
            let owner = if fallback { ProcessOwner::claim_fallback(c.id() as i32) }
                else { ProcessOwner::claim(c.id() as i32) }.unwrap();
            let barrier = Arc::new(Barrier::new(3));
            let waiter = { let o = owner.clone(); let b = barrier.clone();
                thread::spawn(move || { b.wait(); o.wait() }) };
            let killer = { let o = owner.clone(); let b = barrier.clone();
                thread::spawn(move || { b.wait(); o.terminate() }) };
            barrier.wait();
            drop(c.stdin.take());
            let killed = killer.join().unwrap();
            // pidfd may report ESRCH when the child exits during signaling.
            if let Err(error) = killed {
                assert_eq!(error.raw_os_error(), Some(libc::ESRCH));
            }
            let outcome = waiter.join().unwrap();
            assert!(matches!(outcome, ExitOutcome::Exited(17) | ExitOutcome::Exited(-9)));
            assert_eq!(owner.wait(), outcome);
            assert!(!owner.terminate().unwrap());
        }
    }
}

#[test]
fn invalid_and_nonchild_pids_are_rejected() {
    for pid in [0, -1, std::process::id() as i32] {
        assert!(ProcessOwner::claim(pid).is_err());
        assert!(ProcessOwner::claim_fallback(pid).is_err());
    }
}

#[test]
fn external_reap_becomes_lost_without_signaling() {
    for fallback in [false, true] {
        let mut c = Command::new("sh").args(["-c", "read line; exit 0"])
            .stdin(std::process::Stdio::piped()).spawn().unwrap();
        let owner = if fallback { ProcessOwner::claim_fallback(c.id() as i32) }
            else { ProcessOwner::claim(c.id() as i32) }.unwrap();
        drop(c.stdin.take());
        c.wait().unwrap(); // Deliberately violate coordinator's sole-reaper contract.
        assert!(!owner.terminate().unwrap());
        assert_eq!(owner.wait(), ExitOutcome::Lost(libc::ECHILD));
    }
}

#[test]
fn terminate_refreshes_exit_before_signaling() {
    let mut c = Command::new("sh").args(["-c", "read line; exit 19"])
        .stdin(std::process::Stdio::piped()).spawn().unwrap();
    let owner = ProcessOwner::claim_fallback(c.id() as i32).unwrap();
    drop(c.stdin.take());
    zombie(c.id() as i32);
    assert!(owner.is_running()); // Intentionally cached until refresh.
    assert!(!owner.terminate().unwrap());
    assert_eq!(owner.wait(), ExitOutcome::Exited(19));
}
