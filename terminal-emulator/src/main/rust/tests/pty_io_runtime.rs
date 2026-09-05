use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::{Duration, Instant},
};
use termux_rust::engine::io_runtime::{IoRuntime, StopOutcome, SubmitError};

fn pty() -> (OwnedFd, OwnedFd) {
    let (mut master, mut slave) = (-1, -1);
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    unsafe {
        let mut term = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave, &mut term), 0);
        libc::cfmakeraw(&mut term);
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &term), 0);
        (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave))
    }
}
fn stopped(runtime: &IoRuntime) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !runtime.is_stopped() {
        assert!(Instant::now() < deadline, "IO worker failed to stop");
        std::thread::yield_now();
    }
}
fn socket_pair() -> (OwnedFd, OwnedFd) {
    let mut fds = [-1; 2];
    assert_eq!(
        unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        },
        0
    );
    unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
}
fn write_small(fd: &OwnedFd, bytes: &[u8]) {
    assert_eq!(
        unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) },
        bytes.len() as isize
    );
}
fn read_exact(fd: &OwnedFd, len: usize) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    while output.len() < len {
        assert!(
            Instant::now() < deadline,
            "read timeout: {} of {len}",
            output.len()
        );
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut poll, 1, 50) };
        if result <= 0 {
            continue;
        }
        let mut buffer = [0u8; 4096];
        let want = buffer.len().min(len - output.len());
        let n = unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), want) };
        assert!(
            n > 0,
            "unexpected EOF/error: {:?}",
            io::Error::last_os_error()
        );
        output.extend_from_slice(&buffer[..n as usize]);
    }
    output
}
fn gate() -> (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>) {
    std::sync::mpsc::channel()
}

#[test]
fn cancel_silent_slave_is_idempotent_and_closes_admission() {
    let (master, _slave) = pty();
    let mut runtime = IoRuntime::start(master, 4096, |_| vec![]).unwrap();
    runtime.cancel();
    runtime.cancel();
    assert_eq!(runtime.submit(b"x"), Err(SubmitError::Closed));
    stopped(&runtime);
    assert!(matches!(runtime.join().unwrap(), StopOutcome::Cancelled));
}

#[test]
fn queued_and_in_flight_capacity_is_all_or_none() {
    let (master, slave) = pty();
    let (entered_tx, entered) = gate();
    let (release, released) = gate();
    let mut runtime = IoRuntime::start(master, 8, move |_| {
        entered_tx.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(3)).unwrap();
        vec![]
    })
    .unwrap();
    write_small(&slave, b"x");
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    runtime.submit(b"12345678").unwrap();
    assert_eq!(runtime.submit(b"9"), Err(SubmitError::Full));
    release.send(()).unwrap();
    assert_eq!(read_exact(&slave, 8), b"12345678");
    runtime.cancel();
    stopped(&runtime);
    runtime.join().unwrap();
}

#[test]
fn large_user_payload_and_parser_response_remain_exact_and_ordered() {
    let (master, slave) = pty();
    let (entered_tx, entered) = gate();
    let (release, released) = gate();
    let mut runtime = IoRuntime::start(master, 1024 * 1024, move |_| {
        entered_tx.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(3)).unwrap();
        vec![b"parser-tail".to_vec()]
    })
    .unwrap();
    write_small(&slave, b"query");
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut expected: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
    runtime.submit(&expected).unwrap();
    release.send(()).unwrap();
    expected.extend_from_slice(b"parser-tail");
    assert_eq!(read_exact(&slave, expected.len()), expected);
    runtime.cancel();
    stopped(&runtime);
    runtime.join().unwrap();
}

#[test]
fn saturated_real_pty_writer_can_be_cancelled_without_slave_reading() {
    let (master, slave) = pty();
    let fd = master.as_raw_fd();
    let mut runtime = IoRuntime::start(master, 1024 * 1024, |_| vec![]).unwrap();
    runtime.submit(&vec![b'x'; 1024 * 1024]).unwrap();
    let mut poll = libc::pollfd {
        fd: slave.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(unsafe { libc::poll(&mut poll, 1, 3000) }, 1);
    // Do not consume: a pending payload larger than PTY buffering remains.
    runtime.cancel();
    stopped(&runtime);
    assert!(matches!(runtime.join().unwrap(), StopOutcome::Cancelled));
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
}

#[test]
fn eof_delivers_final_read_to_callback_before_stopping() {
    let (master, peer) = socket_pair();
    let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = received.clone();
    let mut runtime = IoRuntime::start(master, 4096, move |bytes| {
        sink.lock().unwrap().extend_from_slice(bytes);
        vec![]
    })
    .unwrap();
    write_small(&peer, b"final-\xf0\x9f\x8c\x99");
    assert_eq!(
        unsafe { libc::shutdown(peer.as_raw_fd(), libc::SHUT_WR) },
        0
    );
    stopped(&runtime);
    assert!(matches!(runtime.join().unwrap(), StopOutcome::Eof));
    assert_eq!(&*received.lock().unwrap(), b"final-\xf0\x9f\x8c\x99");
    assert_eq!(runtime.submit(b"late"), Err(SubmitError::Closed));
}

#[test]
fn parser_response_overflow_is_a_terminal_failure_not_silent_loss() {
    let (master, slave) = pty();
    let (notified, observed) = std::sync::mpsc::channel();
    let mut runtime = IoRuntime::start_with_callbacks(
        master,
        4,
        |_| vec![b"12345".to_vec()],
        || {},
        move |outcome| {
            notified
                .send(matches!(outcome, StopOutcome::ResponseOverflow))
                .unwrap();
        },
    )
    .unwrap();
    write_small(&slave, b"query");
    assert!(
        observed.recv_timeout(Duration::from_secs(3)).unwrap(),
        "failure reason must be visible before join"
    );
    stopped(&runtime);
    assert!(matches!(
        runtime.join().unwrap(),
        StopOutcome::ResponseOverflow
    ));
    assert_eq!(runtime.submit(b"x"), Err(SubmitError::Closed));
}

#[test]
fn callback_unwind_closes_fd_and_old_cancel_never_closes_reused_number() {
    // Exact dup2 reuse belongs in a private process: another concurrently run
    // test must never have its newly allocated descriptor replaced by us.
    if std::env::var_os("TERMUX_PTY_FD_REUSE_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "callback_unwind_closes_fd_and_old_cancel_never_closes_reused_number",
                "--test-threads=1",
            ])
            .env("TERMUX_PTY_FD_REUSE_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let (master, slave) = pty();
    let fd = master.as_raw_fd();
    let mut runtime =
        IoRuntime::start(master, 4096, |_| panic!("injected callback panic")).unwrap();
    write_small(&slave, b"x");
    stopped(&runtime);
    assert!(runtime.join().is_err());
    let replacement = std::fs::File::open("/dev/null").unwrap();
    // dup2 exercises exact descriptor reuse without double-owning replacement.
    let replacement_fd = if replacement.as_raw_fd() == fd {
        None
    } else {
        assert_eq!(unsafe { libc::dup2(replacement.as_raw_fd(), fd) }, fd);
        Some(unsafe { OwnedFd::from_raw_fd(fd) })
    };
    runtime.cancel();
    runtime.cancel();
    drop(runtime);
    assert!(unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0);
    drop(replacement_fd);
    drop(replacement);
}

#[test]
fn resize_is_executed_by_worker_on_real_pty() {
    let (master, slave) = pty();
    let mut runtime = IoRuntime::start(master, 4096, |_| vec![]).unwrap();
    runtime.resize(25, 81, 648, 400).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCGWINSZ, &mut size) },
            0
        );
        if (size.ws_row, size.ws_col, size.ws_xpixel, size.ws_ypixel) == (25, 81, 648, 400) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime.cancel();
    stopped(&runtime);
    runtime.join().unwrap();
    assert_eq!(runtime.resize(1, 1, 1, 1), Err(SubmitError::Closed));
}

#[test]
fn invalid_resize_target_reports_io_error_and_closes_admission() {
    let (master, _peer) = socket_pair();
    let mut runtime = IoRuntime::start(master, 4096, |_| vec![]).unwrap();
    runtime.resize(1, 1, 1, 1).unwrap();
    stopped(&runtime);
    match runtime.join().unwrap() {
        StopOutcome::IoError(error) => assert_eq!(error.raw_os_error(), Some(libc::ENOTTY)),
        other => panic!("wrong outcome: {other:?}"),
    }
}

#[test]
fn fresh_master_waits_for_first_slave_open_without_eof_debounce() {
    use std::ffi::CStr;
    let raw = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_CLOEXEC) };
    assert!(raw >= 0);
    let master = unsafe { OwnedFd::from_raw_fd(raw) };
    assert_eq!(unsafe { libc::grantpt(raw) }, 0);
    assert_eq!(unsafe { libc::unlockpt(raw) }, 0);
    let mut name = [0 as libc::c_char; 128];
    assert_eq!(
        unsafe { libc::ptsname_r(raw, name.as_mut_ptr(), name.len()) },
        0
    );
    let (sent, received) = std::sync::mpsc::channel();
    let mut runtime = IoRuntime::start(master, 4096, move |bytes| {
        sent.send(bytes.to_vec()).unwrap();
        vec![]
    })
    .unwrap();
    // Mirror create_subprocess's parent-return before child opens the slave.
    std::thread::sleep(Duration::from_millis(20));
    assert!(
        !runtime.is_stopped(),
        "master falsely finished before first slave open"
    );
    let name = unsafe { CStr::from_ptr(name.as_ptr()) };
    let slave_fd = unsafe { libc::open(name.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    assert!(slave_fd >= 0);
    let slave = unsafe { OwnedFd::from_raw_fd(slave_fd) };
    write_small(&slave, b"first-open");
    assert_eq!(
        received.recv_timeout(Duration::from_secs(3)).unwrap(),
        b"first-open"
    );
    runtime.cancel();
    stopped(&runtime);
    runtime.join().unwrap();
}

#[test]
fn cancel_revokes_admission_but_does_not_pretend_to_interrupt_callback() {
    let (master, slave) = pty();
    let (entered_tx, entered) = gate();
    let (release, released) = gate();
    let mut runtime = IoRuntime::start(master, 4096, move |_| {
        entered_tx.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(3)).unwrap();
        vec![b"cancelled-response".to_vec()]
    })
    .unwrap();
    write_small(&slave, b"x");
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    runtime.cancel();
    assert!(!runtime.is_stopped(), "callback has not returned yet");
    assert_eq!(runtime.submit(b"x"), Err(SubmitError::Closed));
    release.send(()).unwrap();
    stopped(&runtime);
    assert!(matches!(runtime.join().unwrap(), StopOutcome::Cancelled));
}

#[test]
fn replies_enter_fifo_before_notification_reentrant_input() {
    let (master, slave) = pty();
    let (entered_tx, entered) = gate();
    let (release, released) = gate();
    let mut runtime = IoRuntime::start_with_callbacks(
        master,
        4096,
        |_| vec![b"reply".to_vec()],
        move || {
            entered_tx.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(3)).unwrap();
        },
        |_| {},
    )
    .unwrap();
    write_small(&slave, b"query");
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    runtime.submit(b"callback-input").unwrap();
    release.send(()).unwrap();
    assert_eq!(
        read_exact(&slave, b"replycallback-input".len()),
        b"replycallback-input"
    );
    runtime.cancel();
    stopped(&runtime);
    runtime.join().unwrap();
}
