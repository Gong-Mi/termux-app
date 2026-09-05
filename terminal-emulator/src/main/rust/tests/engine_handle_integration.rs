//! Production global registry, real TerminalContext, fd ownership and RenderFrame.
//! No fake JNI, Surface or GPU: JVM JNI coverage is a separate harness.
use std::os::fd::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use termux_rust::coordinator::{SessionCoordinator, SessionEngineData};
use termux_rust::engine::{ENGINE_HANDLES, TerminalContext, TerminalEngine, destroy_engine};
use termux_rust::renderer::RenderFrame;

#[test]
fn real_context_revocation_preserves_in_flight_frame_and_forgets_delivery() {
    let coordinator = SessionCoordinator::get();
    let session = coordinator.register_session();
    let context = Arc::new(TerminalContext::new(TerminalEngine::new(
        session as i32, 80, 24, 2000, 8, 16,
    )));
    let weak = Arc::downgrade(&context);
    let (master, peer) = UnixStream::pair().unwrap();
    let duplicate = master.try_clone().unwrap();
    let fd = master.into_raw_fd();
    context.pty_fd.store(fd, Ordering::SeqCst);
    let handle = ENGINE_HANDLES.insert(context).unwrap();
    coordinator.set_engine_data(session, SessionEngineData { ptr: handle, pty_fd: fd, pid: -1 });
    assert!(ENGINE_HANDLES.publish(handle));
    let (frame_handle, lease) = ENGINE_HANDLES.current().unwrap();
    assert_eq!(frame_handle, handle);
    lease.lock.write().unwrap().process_bytes(b"last-frame");
    destroy_engine(handle);
    assert!(ENGINE_HANDLES.acquire(handle).is_none());
    assert!(ENGINE_HANDLES.current().is_none());
    assert!(!ENGINE_HANDLES.publish(handle));
    assert!(coordinator.take_engine_data(session).is_none());
    assert!(!lease.running.load(Ordering::SeqCst));
    assert_eq!(lease.pty_fd.load(Ordering::SeqCst), -1);
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
    // Revocation is not reader cancellation: a duplicate remains independently
    // owned. The test deliberately does not claim a blocked read was awakened.
    assert!(unsafe { libc::fcntl(duplicate.as_raw_fd(), libc::F_GETFD) } >= 0);
    let frame = {
        let engine = lease.lock.read().unwrap();
        RenderFrame::from_engine(&engine, 24, 80, 0)
    };
    drop(frame);
    assert!(weak.upgrade().is_some());
    drop(lease);
    assert!(weak.upgrade().is_none());
    destroy_engine(handle);
    drop(duplicate);
    drop(peer);
    coordinator.unregister_session(session);

    let pending_session = coordinator.register_session();
    let pending = Arc::new(TerminalContext::new(TerminalEngine::new(0, 20, 10, 100, 8, 16)));
    let pending_weak = Arc::downgrade(&pending);
    let pending_handle = ENGINE_HANDLES.insert(pending).unwrap();
    coordinator.set_engine_data(pending_session, SessionEngineData { ptr: pending_handle, pty_fd: -1, pid: -1 });
    coordinator.unregister_session(pending_session);
    assert!(ENGINE_HANDLES.acquire(pending_handle).is_none());
    assert!(pending_weak.upgrade().is_none());
}
