//! Native delivery ownership only: no JVM/push callback or Android UI coverage.
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Barrier};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use termux_rust::coordinator::{SessionCoordinator, SessionEngineData};
use termux_rust::engine::{ENGINE_HANDLES, TerminalContext, TerminalEngine, destroy_engine};

// Unique sessions/handles, no global renderer publication, and unconditional
// cleanup keep fixtures isolated even with Rust's parallel test runner.
struct Offer {
    session: usize,
    data: SessionEngineData,
    context: Arc<TerminalContext>,
    peer: UnixStream,
}
impl Offer {
    fn new() -> Self {
        let session = SessionCoordinator::get().register_session();
        let context = Arc::new(TerminalContext::new(TerminalEngine::new(
            session as i32, 80, 24, 100, 8, 16,
        )));
        let (master, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let fd = master.as_raw_fd();
        TerminalContext::start_io_owned(Arc::clone(&context), master.into()).unwrap();
        let handle = ENGINE_HANDLES.insert(Arc::clone(&context)).unwrap();
        let data = SessionEngineData { ptr: handle, pty_fd: fd, pid: -1 };
        SessionCoordinator::get().set_engine_data(session, data);
        Self { session, data, context, peer }
    }
    fn assert_stopped(&mut self) {
        assert!(ENGINE_HANDLES.acquire(self.data.ptr).is_none());
        assert!(!self.context.running.load(Ordering::SeqCst));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.context.io_is_joined() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(self.context.io_is_joined(), "silent reader was not joined");
        assert_eq!(self.peer.read(&mut [0; 1]).unwrap(), 0);
    }
    fn assert_live(&self) {
        assert!(ENGINE_HANDLES.acquire(self.data.ptr).is_some());
        assert!(self.context.running.load(Ordering::SeqCst));
    }
}
impl Drop for Offer {
    fn drop(&mut self) {
        SessionCoordinator::get().unregister_session(self.session);
        destroy_engine(self.data.ptr);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.context.io_is_joined() && Instant::now() < deadline {
            std::thread::yield_now();
        }
    }
}

#[test]
fn unregister_cancels_pending_and_claimed_readers() {
    let c = SessionCoordinator::get();
    for claimed in [false, true] {
        let mut offer = Offer::new();
        if claimed {
            assert_eq!(c.claim_engine_data(offer.session, offer.data.ptr).unwrap().ptr, offer.data.ptr);
        }
        c.unregister_session(offer.session);
        offer.assert_stopped();
        assert!(c.claim_engine_data(offer.session, offer.data.ptr).is_none());
        assert!(!c.ack_engine_data(offer.session, offer.data.ptr));
        assert!(!c.reject_engine_data(offer.session, offer.data.ptr));
        assert!(c.take_engine_data(offer.session).is_none());
    }
}

#[test]
fn ack_transfers_once_and_unregister_leaves_external_owner_live() {
    let c = SessionCoordinator::get();
    let mut offer = Offer::new();
    assert!(!c.ack_engine_data(offer.session, offer.data.ptr));
    let claimed = c.claim_engine_data(offer.session, offer.data.ptr).unwrap();
    assert_eq!((claimed.pty_fd, claimed.pid), (offer.data.pty_fd, offer.data.pid));
    assert!(c.claim_engine_data(offer.session, offer.data.ptr).is_none());
    assert!(c.take_engine_data(offer.session).is_none());
    assert!(c.ack_engine_data(offer.session, offer.data.ptr));
    assert!(!c.ack_engine_data(offer.session, offer.data.ptr));
    assert!(!c.reject_engine_data(offer.session, offer.data.ptr));
    c.unregister_session(offer.session);
    offer.assert_live();
    destroy_engine(offer.data.ptr);
    offer.assert_stopped();
}

#[test]
fn reject_checks_identity_and_reclaims_pending_or_claimed_only_once() {
    let c = SessionCoordinator::get();
    for claimed in [false, true] {
        let mut offer = Offer::new();
        let other = Offer::new();
        assert!(c.claim_engine_data(offer.session, other.data.ptr).is_none());
        if claimed { assert!(c.claim_engine_data(offer.session, offer.data.ptr).is_some()); }
        assert!(!c.ack_engine_data(offer.session, other.data.ptr));
        assert!(!c.reject_engine_data(offer.session, other.data.ptr));
        offer.assert_live();
        other.assert_live();
        assert!(c.reject_engine_data(offer.session, offer.data.ptr));
        assert!(!c.reject_engine_data(offer.session, offer.data.ptr));
        assert!(!c.ack_engine_data(offer.session, offer.data.ptr));
        offer.assert_stopped();
        other.assert_live();
    }
}

#[test]
fn direct_destroy_discards_pending_and_claimed_tokens() {
    let c = SessionCoordinator::get();
    for claimed in [false, true] {
        let mut offer = Offer::new();
        if claimed { assert!(c.claim_engine_data(offer.session, offer.data.ptr).is_some()); }
        destroy_engine(offer.data.ptr);
        offer.assert_stopped();
        assert!(c.claim_engine_data(offer.session, offer.data.ptr).is_none());
        assert!(!c.ack_engine_data(offer.session, offer.data.ptr));
        assert!(!c.reject_engine_data(offer.session, offer.data.ptr));
        assert!(c.take_engine_data(offer.session).is_none());
    }
}

#[test]
fn duplicate_set_preserves_claim_and_replacement_reclaims_old_offer() {
    let c = SessionCoordinator::get();
    let mut old = Offer::new();
    c.set_engine_data(old.session, old.data);
    assert!(c.claim_engine_data(old.session, old.data.ptr).is_some());
    c.set_engine_data(old.session, old.data);
    assert!(c.claim_engine_data(old.session, old.data.ptr).is_none());
    assert!(c.take_engine_data(old.session).is_none());
    old.assert_live();
    let replacement = Offer::new();
    let data = c.take_engine_data(replacement.session).unwrap();
    c.set_engine_data(old.session, data);
    old.assert_stopped();
    assert!(!c.reject_engine_data(old.session, old.data.ptr));
    assert!(!c.ack_engine_data(old.session, old.data.ptr));
    replacement.assert_live();
    assert_eq!(c.take_engine_data(old.session).unwrap().ptr, data.ptr);
}

#[test]
fn late_set_after_unregister_is_reclaimed() {
    let c = SessionCoordinator::get();
    let mut offer = Offer::new();
    let data = c.take_engine_data(offer.session).unwrap();
    assert!(c.take_engine_data(offer.session).is_none());
    assert!(c.claim_engine_data(offer.session, data.ptr).is_none());
    assert!(!c.ack_engine_data(offer.session, data.ptr));
    assert!(!c.reject_engine_data(offer.session, data.ptr));
    offer.assert_live();
    c.unregister_session(offer.session);
    c.set_engine_data(offer.session, data);
    offer.assert_stopped();
    assert!(c.claim_engine_data(offer.session, data.ptr).is_none());
    assert!(!c.ack_engine_data(offer.session, data.ptr));
}

#[test]
fn unregister_and_ack_linearize_cleanup_responsibility() {
    let c = SessionCoordinator::get();
    for _ in 0..32 {
        let mut offer = Offer::new();
        assert!(c.claim_engine_data(offer.session, offer.data.ptr).is_some());
        let gate = Arc::new(Barrier::new(2));
        let worker_gate = Arc::clone(&gate);
        let session = offer.session;
        let unregister = std::thread::spawn(move || {
            worker_gate.wait();
            SessionCoordinator::get().unregister_session(session);
        });
        gate.wait();
        let acked = c.ack_engine_data(session, offer.data.ptr);
        unregister.join().unwrap();
        assert!(c.claim_engine_data(session, offer.data.ptr).is_none());
        assert!(!c.ack_engine_data(session, offer.data.ptr));
        if acked { offer.assert_live(); } else { offer.assert_stopped(); }
    }
}

#[test]
fn poll_and_claim_have_exactly_one_winner() {
    let c = SessionCoordinator::get();
    // Barrier races exercise production synchronization; deterministic serial
    // cases above separately guarantee both legal winner states.
    for _ in 0..32 {
        let offer = Offer::new();
        let gate = Arc::new(Barrier::new(2));
        let session = offer.session;
        let handle = offer.data.ptr;
        let worker_gate = Arc::clone(&gate);
        let poll = std::thread::spawn(move || {
            worker_gate.wait();
            SessionCoordinator::get().take_engine_data(session)
        });
        gate.wait();
        let claimed = c.claim_engine_data(session, handle);
        let polled = poll.join().unwrap();
        assert_ne!(claimed.is_some(), polled.is_some());
        assert!(c.take_engine_data(session).is_none());
        assert!(c.claim_engine_data(session, handle).is_none());
        assert_eq!(c.ack_engine_data(session, handle), claimed.is_some());
        c.unregister_session(session);
        offer.assert_live();
    }
}

#[test]
fn unadopted_cleanup_terminates_owned_process_without_relying_on_pty_hup() {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    use termux_rust::process_owner::ExitOutcome;
    let c = SessionCoordinator::get();
    for mode in ["reject", "unregister", "late_offer", "acked"] {
        let session = c.register_session();
        let mut child = Command::new("sh")
            .args(["-c", "trap '' HUP; printf 'ready\n'; read -r hold; exit 23"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut ready = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut ready).unwrap();
        assert_eq!(ready, "ready\n");
        let process = c.bind_pid(session, child.id() as i32).unwrap();
        let context = Arc::new(TerminalContext::with_process(
            TerminalEngine::new(session as i32, 80, 24, 100, 8, 16), process.clone(),
        ));
        // A distinct socket makes PTY HUP unavailable as an accidental kill path.
        let (master, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        TerminalContext::start_io_owned(context.clone(), master.into()).unwrap();
        let handle = ENGINE_HANDLES.insert(context.clone()).unwrap();
        let data = SessionEngineData { ptr: handle, pty_fd: -1, pid: child.id() as i32 };
        if mode == "late_offer" { c.unregister_session(session); }
        c.set_engine_data(session, data);
        if mode == "acked" {
            assert!(c.claim_engine_data(session, handle).is_some());
            assert!(c.ack_engine_data(session, handle));
            c.unregister_session(session);
            destroy_engine(handle); // normal display disposal must not kill
            drop(child.stdin.take());
            assert_eq!(process.wait(), ExitOutcome::Exited(23));
            assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
            assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
            continue;
        }
        if mode == "reject" { assert!(c.reject_engine_data(session, handle)); }
        if mode == "unregister" { c.unregister_session(session); }
        let deadline = Instant::now() + Duration::from_secs(3);
        while process.outcome().is_none() && Instant::now() < deadline { std::thread::yield_now(); }
        let observed = process.outcome();
        // RED must not leave a live child behind after reporting the failure.
        if observed.is_none() { process.terminate().unwrap(); }
        process.wait();
        c.unregister_session(session);
        destroy_engine(handle);
        assert_eq!(child.wait().unwrap_err().raw_os_error(), Some(libc::ECHILD));
        assert_eq!(observed, Some(ExitOutcome::Exited(-libc::SIGKILL)), "{mode} left an unadopted child running");
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    }
}
