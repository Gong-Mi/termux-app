use nix::errno::Errno;
use nix::sys::socket::{
    AddressFamily, Backlog, SockFlag, SockType, UnixAddr, accept, bind, getsockopt, listen, socket,
    sockopt,
};
use nix::unistd::close;
use std::fs::File;
use std::io::Read;
use std::os::unix::io::{AsRawFd, BorrowedFd, IntoRawFd, RawFd};
use std::time::Instant;

#[derive(Debug, Default)]
pub struct PeerCred {
    pub pid: i32,
    pub uid: i32,
    pub gid: i32,
    pub pname: String,
    pub cmdline: String,
}

pub fn get_process_cmdline(pid: i32) -> String {
    let path = format!("/proc/{}/cmdline", pid);
    if let Ok(mut file) = File::open(path) {
        let mut buf = Vec::new();
        if let Ok(_) = file.read_to_end(&mut buf) {
            return String::from_utf8_lossy(&buf).to_string();
        }
    }
    String::new()
}

pub fn get_process_name_from_cmdline(cmdline: &str) -> String {
    cmdline.split('\0').next().unwrap_or("").to_string()
}

pub fn replace_null_with_space(cmdline: &str) -> String {
    cmdline.replace('\0', " ").trim().to_string()
}

pub fn create_server_socket(path: &[u8], backlog: i32) -> Result<RawFd, Errno> {
    let fd = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )?;

    let addr = UnixAddr::new(path)?;

    if let Err(e) = bind(fd.as_raw_fd(), &addr) {
        let _ = close(fd.as_raw_fd());
        return Err(e);
    }

    if let Err(e) = listen(&fd, Backlog::new(backlog).unwrap()) {
        let _ = close(fd.as_raw_fd());
        return Err(e);
    }

    Ok(fd.into_raw_fd())
}

pub fn accept_client(server_fd: RawFd) -> Result<RawFd, Errno> {
    let fd = unsafe { BorrowedFd::borrow_raw(server_fd) };
    accept(fd.as_raw_fd()).map(|fd| fd.into_raw_fd())
}

pub fn read_socket(fd: RawFd, buf: &mut [u8], deadline_ms: i64) -> Result<usize, Errno> {
    let start = Instant::now();
    let mut total_read = 0;
    let b_fd = unsafe { BorrowedFd::borrow_raw(fd) };

    while total_read < buf.len() {
        if deadline_ms > 0 {
            let elapsed = start.elapsed().as_millis() as i64;
            if elapsed > deadline_ms {
                return Err(Errno::ETIMEDOUT);
            }
        }

        match nix::unistd::read(&b_fd, &mut buf[total_read..]) {
            Ok(0) => break, // EOF
            Ok(n) => total_read += n,
            Err(Errno::EAGAIN) | Err(Errno::EINTR) => continue,
            Err(e) => return Err(e),
        }
    }

    Ok(total_read)
}

pub fn send_socket(fd: RawFd, buf: &[u8], deadline_ms: i64) -> Result<(), Errno> {
    let start = Instant::now();
    let mut total_sent = 0;

    while total_sent < buf.len() {
        if deadline_ms > 0 {
            let elapsed = start.elapsed().as_millis() as i64;
            if elapsed > deadline_ms {
                return Err(Errno::ETIMEDOUT);
            }
        }

        match nix::sys::socket::send(
            fd,
            &buf[total_sent..],
            nix::sys::socket::MsgFlags::MSG_NOSIGNAL,
        ) {
            Ok(n) => total_sent += n,
            Err(Errno::EAGAIN) | Err(Errno::EINTR) => continue,
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

pub fn get_peer_cred(fd: RawFd) -> Result<PeerCred, Errno> {
    let b_fd = unsafe { BorrowedFd::borrow_raw(fd) };
    let ucred = getsockopt(&b_fd, sockopt::PeerCredentials)?;

    let mut cred = PeerCred {
        pid: ucred.pid(),
        uid: ucred.uid() as i32,
        gid: ucred.gid() as i32,
        ..Default::default()
    };

    let cmdline = get_process_cmdline(cred.pid);
    if !cmdline.is_empty() {
        cred.pname = get_process_name_from_cmdline(&cmdline);
        cred.cmdline = replace_null_with_space(&cmdline);
    }

    Ok(cred)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_local_socket_communication() {
        let socket_path_str = "/data/data/com.termux/files/home/.gemini/tmp/test-socket-rust";
        let _ = std::fs::remove_file(socket_path_str);
        let socket_path = socket_path_str.as_bytes();
        let server_fd = create_server_socket(socket_path, 5).expect("Failed to create server");

        let handle = thread::spawn(move || {
            let client_fd = accept_client(server_fd).expect("Failed to accept");
            let mut buf = [0u8; 5];
            read_socket(client_fd, &mut buf, 1000).expect("Read failed");
            assert_eq!(&buf, b"hello");
            send_socket(client_fd, b"world", 1000).expect("Write failed");
            let _ = close(client_fd);
            let _ = close(server_fd);
        });

        let client_fd = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::empty(),
            None,
        )
        .unwrap();
        nix::sys::socket::connect(client_fd.as_raw_fd(), &UnixAddr::new(socket_path).unwrap())
            .expect("Connect failed");

        send_socket(client_fd.as_raw_fd(), b"hello", 1000).unwrap();
        let mut buf = [0u8; 5];
        read_socket(client_fd.as_raw_fd(), &mut buf, 1000).unwrap();
        assert_eq!(&buf, b"world");

        let _ = close(client_fd.into_raw_fd());
        handle.join().unwrap();
    }
}
