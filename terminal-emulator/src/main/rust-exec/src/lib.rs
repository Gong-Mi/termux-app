use std::ffi::{CStr, CString};
use std::io::Read;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

static REAL_DLSYM: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());

#[repr(C)]
struct sock_filter { code: u16, jt: u8, jf: u8, k: u32 }
#[repr(C)]
struct sock_fprog { len: u16, _pad: [u16; 3], filter: *const sock_filter }

const SECCOMP_RET_TRAP: u32 = 0x00030000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;
const RTLD_NEXT: *mut libc::c_void = -1i64 as *mut libc::c_void;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".init_array")]
pub static LOAD_HOOK: unsafe extern "C" fn() = {
    unsafe extern "C" fn init() {
        unsafe { setup_universal_interceptor() };
    }
    init
};

unsafe fn setup_universal_interceptor() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigsys_handler as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigaction(libc::SIGSYS, &sa, ptr::null_mut());

        let filter = [
            sock_filter { code: 0x20, jt: 0, jf: 0, k: 0 }, // BPF_LD | BPF_W | BPF_ABS (load syscall nr)
            sock_filter { code: 0x15, jt: 1, jf: 0, k: libc::SYS_execve as u32 }, // BPF_JMP | BPF_JEQ (if execve jump to TRAP)
            sock_filter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW }, // BPF_RET (ALLOW)
            sock_filter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_TRAP }, // BPF_RET (TRAP)
        ];

        let prog = sock_fprog { len: filter.len() as u16, _pad: [0; 3], filter: filter.as_ptr() };
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &prog as *const _);
    }
}

unsafe extern "C" fn sigsys_handler(_sig: c_int, _info: *mut libc::siginfo_t, void_context: *mut c_void) {
    let ctx_ptr = void_context as *mut u64;
    
    // aarch64 ucontext layout: x0 is at index 23, x1 at 24, x2 at 25, syscall nr at 31
    #[cfg(target_arch = "aarch64")]
    let (path_ptr, argv_ptr, envp_ptr) = unsafe { (
        *ctx_ptr.offset(23) as *const c_char,
        *ctx_ptr.offset(24) as *const *const c_char,
        *ctx_ptr.offset(25) as *const *const c_char,
    ) };

    #[cfg(not(target_arch = "aarch64"))]
    return; // Not implemented for other archs in this snippet

    if path_ptr.is_null() { return; }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_string_lossy().to_string();

    // Skip system binaries
    if path_str.starts_with("/system/") || path_str.starts_with("/vendor/") || path_str.contains("/linker") {
        return;
    }

    // Map paths and handle shebangs
    if let Some((final_path, new_argv)) = transform_exec(&path_str, argv_ptr) {
        let c_path = CString::new(final_path).unwrap();
        let c_argv_ptrs: Vec<*const c_char> = new_argv.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
        
        // Execute using execveat to bypass our own SECCOMP filter (which only traps execve)
        unsafe { libc::syscall(
            libc::SYS_execveat,
            libc::AT_FDCWD,
            c_path.as_ptr(),
            c_argv_ptrs.as_ptr(),
            envp_ptr,
            0
        ) };
    } else {
        // Fallback for non-termux binaries if any
        unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, path_ptr, argv_ptr, envp_ptr, 0) };
    }

    // If we reach here, execveat failed
    unsafe { libc::_exit(1) };
}

fn transform_exec(path: &str, orig_argv: *const *const c_char) -> Option<(String, Vec<CString>)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = [0u8; 256];
    let n = file.read(&mut buffer).ok()?;
    
    let linker = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
        // ELF: Prepend linker
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(path).unwrap());
        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }
        return Some((linker.to_string(), new_argv));
    } else if let Some((interpreter, shebang_args)) = parse_shebang(&buffer[..n]) {
        // Shebang script
        let mut new_argv = Vec::new();
        let resolved_interp = map_path(&interpreter);
        
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(resolved_interp.clone()).unwrap());
        
        if let Some(args) = shebang_args {
            for arg in args.split_whitespace() {
                new_argv.push(CString::new(arg).unwrap());
            }
        }
        new_argv.push(CString::new(path).unwrap());
        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }
        return Some((linker.to_string(), new_argv));
    }
    
    None
}

fn parse_shebang(buffer: &[u8]) -> Option<(String, Option<String>)> {
    if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' {
        return None;
    }
    let line_end = buffer.iter().position(|&b| b == b'\n').unwrap_or(buffer.len());
    let line = String::from_utf8_lossy(&buffer[2..line_end]);
    let trimmed = line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() { return None; }

    let interpreter = tokens[0].to_string();
    let args = if tokens.len() > 1 { Some(tokens[1..].join(" ")) } else { None };
    Some((interpreter, args))
}

fn map_path(path: &str) -> String {
    if path.starts_with("/usr/bin/") {
        format!("/data/data/com.termux/files/usr/bin/{}", &path[9..])
    } else if path.starts_with("/bin/") {
        format!("/data/data/com.termux/files/usr/bin/{}", &path[5..])
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shebang_simple() {
        let content = b"#!/usr/bin/python\nprint('hello')";
        let (interp, args) = parse_shebang(content).unwrap();
        assert_eq!(interp, "/usr/bin/python");
        assert_eq!(args, None);
    }

    #[test]
    fn test_parse_shebang_with_args() {
        let content = b"#!/usr/bin/env python -u\n...";
        let (interp, args) = parse_shebang(content).unwrap();
        assert_eq!(interp, "/usr/bin/env");
        assert_eq!(args, Some("python -u".to_string()));
    }

    #[test]
    fn test_map_path_usr_bin() {
        assert_eq!(
            map_path("/usr/bin/ls"),
            "/data/data/com.termux/files/usr/bin/ls"
        );
    }

    #[test]
    fn test_map_path_bin() {
        assert_eq!(
            map_path("/bin/sh"),
            "/data/data/com.termux/files/usr/bin/sh"
        );
    }

    #[test]
    fn test_map_path_untouched() {
        assert_eq!(
            map_path("/system/bin/linker64"),
            "/system/bin/linker64"
        );
    }
}
