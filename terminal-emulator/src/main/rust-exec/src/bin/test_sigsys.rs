//! 诊断测试：验证 sigsys_handler 中 ucontext 寄存器读取是否正确
//!
//! 使用 /proc/self/mem 安全地读取可能无效的指针，避免 segfault。


use std::sync::atomic::{AtomicBool, Ordering};

static CAUGHT: AtomicBool = AtomicBool::new(false);
static mut DUMP: [u64; 64] = [0; 64];

#[repr(C)]
struct sock_filter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}
#[repr(C)]
struct sock_fprog {
    len: u16,
    _pad: [u16; 3],
    filter: *const sock_filter,
}

const SECCOMP_RET_TRAP: u32 = 0x00030000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;

/// 信号 handler：只做无分配的原始数据拷贝
unsafe extern "C" fn sigsys_handler(
    _sig: libc::c_int,
    _info: *mut libc::siginfo_t,
    void_context: *mut libc::c_void,
) {
    let src = void_context as *const u64;
    let dst = std::ptr::addr_of_mut!(DUMP) as *mut u64;
    for i in 0..64 {
        unsafe {
            *dst.add(i) = *src.add(i);
        }
    }
    CAUGHT.store(true, Ordering::SeqCst);
}

/// 通过 /proc/self/mem 安全读取任意地址（不会 segfault）
fn safe_read_mem(addr: u64, buf: &mut [u8]) -> isize {
    let path = b"/proc/self/mem\0";
    let fd = unsafe { libc::open(path.as_ptr() as *const libc::c_char, libc::O_RDONLY) };
    if fd < 0 {
        return -1;
    }
    let _ = unsafe { libc::lseek64(fd, addr as i64, libc::SEEK_SET) };
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    unsafe { libc::close(fd); }
    n
}

/// 尝试安全地读取一个 C 字符串（通过 /proc/self/mem）
fn safe_str(addr: u64) -> Option<String> {
    // 允许带 tag 的高位指针（Android ARM64 MTE/TBI tag 在 bit 56+）
    if addr == 0 {
        return None;
    }
    let mut buf = [0u8; 512];
    let n = safe_read_mem(addr, &mut buf);
    if n <= 0 {
        return None;
    }
    let len = n as usize;
    // 找到第一个 \0
    let end = buf[..len].iter().position(|&b| b == 0).unwrap_or(len);
    let s = String::from_utf8_lossy(&buf[..end]);
    if !s.is_empty()
        && s.len() < 512
        && s.chars().all(|c| {
            c.is_ascii_graphic() || c == ' ' || c == '/' || c == '.' || c == '-' || c == '_'
        })
    {
        Some(s.to_string())
    } else {
        None
    }
}

/// 尝试读取一个字符串数组（argv/envp），最多 max 个元素
fn safe_str_array(addr: u64, max: usize) -> Vec<String> {
    let mut result = Vec::new();
    for i in 0..max {
        let mut ptr_buf = [0u8; 8];
        let n = safe_read_mem(addr + (i * 8) as u64, &mut ptr_buf);
        if n != 8 {
            break;
        }
        let ptr = u64::from_le_bytes(ptr_buf);
        if ptr == 0 {
            break;
        }
        if let Some(s) = safe_str(ptr) {
            result.push(s);
        } else {
            result.push(format!("<unreadable: 0x{:x}>", ptr));
        }
    }
    result
}

fn main() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigsys_handler as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigaction(libc::SIGSYS, &sa, std::ptr::null_mut());

        let filter = [
            sock_filter { code: 0x20, jt: 0, jf: 0, k: 0 },
            sock_filter {
                code: 0x15,
                jt: 1,
                jf: 0,
                k: libc::SYS_execve as u32,
            },
            sock_filter {
                code: 0x06,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ALLOW,
            },
            sock_filter {
                code: 0x06,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_TRAP,
            },
        ];
        let prog = sock_fprog {
            len: filter.len() as u16,
            _pad: [0; 3],
            filter: filter.as_ptr(),
        };
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            eprintln!("prctl(NO_NEW_PRIVS) failed");
            std::process::exit(1);
        }
        if libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &prog as *const _,
        ) != 0
        {
            eprintln!("prctl(SECCOMP) failed");
            std::process::exit(1);
        }

        let path = std::ffi::CString::new("/system/bin/echo").unwrap();
        let argv0 = std::ffi::CString::new("echo").unwrap();
        let argv1 = std::ffi::CString::new("hello").unwrap();
        let argv: [*const libc::c_char; 3] =
            [argv0.as_ptr(), argv1.as_ptr(), std::ptr::null()];

        println!(
            "[MAIN] Triggering execve(\"{}\", [\"{}\", \"{}\"])",
            path.to_str().unwrap(),
            argv0.to_str().unwrap(),
            argv1.to_str().unwrap()
        );
        println!("[MAIN] path ptr  = 0x{:016x}", path.as_ptr() as u64);
        println!("[MAIN] argv ptr  = 0x{:016x}", argv.as_ptr() as u64);
        println!("[MAIN] argv0 ptr = 0x{:016x}", argv0.as_ptr() as u64);
        println!("[MAIN] argv1 ptr = 0x{:016x}", argv1.as_ptr() as u64);
        libc::syscall(
            libc::SYS_execve,
            path.as_ptr(),
            argv.as_ptr(),
            std::ptr::null::<libc::c_char>(),
        );

        if CAUGHT.load(Ordering::SeqCst) {
            println!("[MAIN] ✅ SIGSYS caught\n");
        } else {
            println!("[MAIN] ❌ SIGSYS was NOT caught");
            std::process::exit(1);
        }
    }

    let dump = unsafe { DUMP };
    println!("========== ucontext raw dump (first 32 u64) ==========");
    for i in 0..32 {
        println!("  offset({:2}) = 0x{:016x}", i, dump[i]);
    }

    println!("\n========== probing offsets 20..30 as potential regs ==========");
    for i in 20..=30 {
        let val = dump[i];
        if let Some(s) = safe_str(val) {
            println!("offset({:2}) -> string: \"{}\"", i, s);
        } else {
            println!("offset({:2}) -> 0x{:016x} (not a readable string)", i, val);
        }
    }

    println!("\n========== checking offset 23/24/25 as x0/x1/x2 ==========");
    println!("offset(23) x0 (path): 0x{:016x}", dump[23]);
    if let Some(s) = safe_str(dump[23]) {
        println!("  -> \"{}\"", s);
    }
    println!("offset(24) x1 (argv): 0x{:016x}", dump[24]);
    let argv = safe_str_array(dump[24], 8);
    println!("  -> {:?}", argv);
    println!("offset(25) x2 (envp): 0x{:016x}", dump[25]);
    if dump[25] != 0 {
        let envp = safe_str_array(dump[25], 16);
        println!("  -> {:?}", envp);
    } else {
        println!("  -> null (as expected for our test)");
    }
}
