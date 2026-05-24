use std::ffi::{CStr, CString};
use std::io::Read;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

static REAL_DLSYM: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
struct sock_filter { code: u16, jt: u8, jf: u8, k: u32 }
#[repr(C)]
struct sock_fprog { len: u16, _pad: [u16; 3], filter: *const sock_filter }

const SECCOMP_RET_TRAP: u32 = 0x00030000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;
const RTLD_NEXT: *mut libc::c_void = -1i64 as *mut libc::c_void;
const AUDIT_ARCH_AARCH64: u32 = 0xc00000b7;
const AUDIT_ARCH_X86_64: u32 = 0xc000003e;

unsafe extern "C" {
    fn get_execve_path(ucontext: *mut c_void) -> usize;
    fn get_execve_argv(ucontext: *mut c_void) -> usize;
    fn get_execve_envp(ucontext: *mut c_void) -> usize;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".init_array")]
pub static LOAD_HOOK: unsafe extern "C" fn() = {
    unsafe extern "C" fn init() {
        unsafe { init_logging() };
        ensure_ld_preload_is_exported();
        unsafe { setup_universal_interceptor() };
    }
    init
};

#[cfg(target_os = "android")]
unsafe fn init_logging() {
    if LOGGER_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("TermuxExec"),
    );
}

#[cfg(not(target_os = "android"))]
unsafe fn init_logging() {
    // noop on non-Android targets
}

fn ensure_ld_preload_is_exported() {
    let Some(path) = current_library_path() else {
        log::warn!("unable to restore LD_PRELOAD: libtermux-exec.so not found in maps");
        return;
    };

    unsafe {
        if let Ok(key) = CString::new("LD_PRELOAD") {
            if let Ok(value) = CString::new(path.as_str()) {
                libc::setenv(key.as_ptr(), value.as_ptr(), 1);
            }
        }
    }
    log::info!("restored LD_PRELOAD to {}", path);
}

fn current_library_path() -> Option<String> {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return None;
    };

    for line in maps.lines() {
        let Some(path_start) = line.find('/') else {
            continue;
        };
        let path = &line[path_start..];
        if !path.ends_with("libtermux-exec.so") {
            continue;
        }

        return Some(path.to_string());
    }

    None
}

fn envp_with_ld_preload(envp: *const *const c_char) -> (Vec<CString>, Vec<*const c_char>) {
    let Some(ld_preload) = current_library_path().or_else(|| {
        std::env::var("LD_PRELOAD")
            .ok()
            .filter(|value| !value.is_empty())
    }) else {
        return (Vec::new(), vec![ptr::null()]);
    };

    let mut entries = Vec::new();
    let mut has_ld_preload = false;

    unsafe {
        let mut i = 0;
        while !envp.is_null() && !(*envp.offset(i)).is_null() {
            let entry = CStr::from_ptr(*envp.offset(i)).to_string_lossy();
            if entry.starts_with("LD_PRELOAD=") {
                has_ld_preload = true;
                entries.push(CString::new(format!("LD_PRELOAD={}", ld_preload)).unwrap());
            } else {
                entries.push(CString::new(entry.as_bytes()).unwrap_or_else(|_| {
                    CString::new("").unwrap()
                }));
            }
            i += 1;
        }
    }

    if !has_ld_preload {
        entries.push(CString::new(format!("LD_PRELOAD={}", ld_preload)).unwrap());
    }

    let mut ptrs: Vec<*const c_char> = entries.iter().map(|entry| entry.as_ptr()).collect();
    ptrs.push(ptr::null());
    (entries, ptrs)
}

unsafe fn setup_universal_interceptor() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigsys_handler as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigaction(libc::SIGSYS, &sa, ptr::null_mut());

        let expected_arch = if cfg!(target_arch = "aarch64") {
            AUDIT_ARCH_AARCH64
        } else if cfg!(target_arch = "x86_64") {
            AUDIT_ARCH_X86_64
        } else {
            0
        };
        if expected_arch == 0 {
            log::warn!("seccomp interceptor disabled on unsupported architecture");
            return;
        }

        let filter = [
            sock_filter { code: 0x20, jt: 0, jf: 4, k: 4 }, // BPF_LD | BPF_W | BPF_ABS seccomp_data.arch
            sock_filter { code: 0x15, jt: 0, jf: 3, k: expected_arch }, // if arch != expected, allow
            sock_filter { code: 0x20, jt: 0, jf: 0, k: 0 }, // load seccomp_data.nr
            sock_filter { code: 0x15, jt: 0, jf: 1, k: libc::SYS_execve as u32 }, // if execve, trap
            sock_filter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_TRAP },
            sock_filter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW },
        ];

        let prog = sock_fprog { len: filter.len() as u16, _pad: [0; 3], filter: filter.as_ptr() };
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &prog as *const _);
    }
}

unsafe extern "C" fn sigsys_handler(_sig: c_int, _info: *mut libc::siginfo_t, void_context: *mut c_void) {
    let path_ptr = unsafe { get_execve_path(void_context) } as *const c_char;
    let argv_ptr = unsafe { get_execve_argv(void_context) } as *const *const c_char;
    let envp_ptr = unsafe { get_execve_envp(void_context) } as *const *const c_char;

    if path_ptr.is_null() {
        log::warn!("sigsys_handler: path_ptr is null, ignoring");
        return;
    }
    let path_str = unsafe { CStr::from_ptr(path_ptr) }.to_string_lossy().to_string();
    log::debug!("sigsys: execve(\"{}\")", path_str);

    // For system binaries: bypass our own SECCOMP filter by using execveat
    if path_str.starts_with("/system/") || path_str.starts_with("/vendor/") || path_str.contains("/linker") {
        log::debug!("sigsys: passing through system binary \"{}\"", path_str);
        let (_env_entries, env_ptrs) = envp_with_ld_preload(envp_ptr);
        let final_envp = if env_ptrs.len() > 1 { env_ptrs.as_ptr() } else { envp_ptr };
        unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, path_ptr, argv_ptr, final_envp, 0) };
        unsafe { libc::_exit(1) };
    }

    // Map paths and handle shebangs
    if let Some((final_path, new_argv)) = transform_exec(&path_str, argv_ptr, 0) {
        let argv_display: Vec<String> = new_argv.iter().map(|s| s.to_string_lossy().to_string()).collect();
        log::info!("sigsys: transformed \"{}\" -> \"{}\" with argv {:?}", path_str, final_path, argv_display);

        let c_path = CString::new(final_path).unwrap();
        let c_argv_ptrs: Vec<*const c_char> = new_argv.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
        let (_env_entries, env_ptrs) = envp_with_ld_preload(envp_ptr);
        let final_envp = if env_ptrs.len() > 1 { env_ptrs.as_ptr() } else { envp_ptr };
        
        // Execute using execveat to bypass our own SECCOMP filter (which only traps execve)
        unsafe { libc::syscall(
            libc::SYS_execveat,
            libc::AT_FDCWD,
            c_path.as_ptr(),
            c_argv_ptrs.as_ptr(),
            final_envp,
            0
        ) };
    } else {
        log::debug!("sigsys: no transform for \"{}\", falling back to execveat", path_str);
        let (_env_entries, env_ptrs) = envp_with_ld_preload(envp_ptr);
        let final_envp = if env_ptrs.len() > 1 { env_ptrs.as_ptr() } else { envp_ptr };
        unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, path_ptr, argv_ptr, final_envp, 0) };
    }

    // If we reach here, execveat failed
    log::error!("sigsys: execveat failed for \"{}\", exiting", path_str);
    unsafe { libc::_exit(1) };
}

fn transform_exec(path: &str, orig_argv: *const *const c_char, depth: u32) -> Option<(String, Vec<CString>)> {
    if depth > 3 {
        log::warn!("transform_exec: shebang chain too deep for \"{}\", giving up", path);
        return None;
    }

    let path = resolve_exec_path(path)?;
    let mut file = std::fs::File::open(&path).ok()?;
    let mut buffer = [0u8; 256];
    let n = file.read(&mut buffer).ok()?;
    
    let linker = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
        log::debug!("transform_exec: \"{}\" is ELF, prepending linker", path);
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(path.clone()).unwrap());
        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }
        return Some((linker.to_string(), new_argv));
    } else if let Some((interpreter, shebang_args)) = parse_shebang(&buffer[..n]) {
        log::debug!("transform_exec: \"{}\" has shebang interpreter=\"{}\" args={:?}", path, interpreter, shebang_args);
        // Shebang script — resolve interpreter path and check if it is ALSO a script
        let resolved_interp = map_path(&interpreter);
        log::debug!("transform_exec: mapped interpreter \"{}\" -> \"{}\"", interpreter, resolved_interp);

        // If the interpreter itself is a shebang script, recurse to find the real ELF loader
        if let Some((real_linker, mut real_argv)) = transform_exec(&resolved_interp, orig_argv, depth + 1) {
            // Insert the script path into the recursive result (after the interpreter)
            // real_argv layout: [linker, interp, ...orig_args...]
            // we need: [linker, interp, script_path, ...orig_args...]
            if real_argv.len() >= 2 {
                real_argv.insert(2, CString::new(path.clone()).unwrap());
            } else {
                real_argv.push(CString::new(path.clone()).unwrap());
            }
            return Some((real_linker, real_argv));
        }

        // Interpreter is ELF (or direct exec). Build argv for linker -> interp -> script
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(resolved_interp.clone()).unwrap());
        
        if let Some(args) = shebang_args {
            for arg in args.split_whitespace() {
                new_argv.push(CString::new(arg).unwrap());
            }
        }
        new_argv.push(CString::new(path.clone()).unwrap());
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

fn resolve_exec_path(path: &str) -> Option<String> {
    if path.contains('/') {
        return Some(map_path(path));
    }

    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = format!("{}/{}", dir, path);
            if std::path::Path::new(&candidate).exists() {
                return Some(map_path(&candidate));
            }
        }
    }

    let prefix_candidate = format!("{}/bin/{}", get_termux_prefix(), path);
    if std::path::Path::new(&prefix_candidate).exists() {
        return Some(prefix_candidate);
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

fn get_termux_prefix() -> String {
    // 1. Try PREFIX env var (set by Termux shell)
    if let Ok(prefix) = std::env::var("PREFIX") {
        return prefix;
    }
    // 2. Try infer from HOME env var (HOME=/data/.../files/home)
    if let Ok(home) = std::env::var("HOME") {
        if home.ends_with("/files/home") {
            return format!("{}/usr", &home[..home.len() - 4]);
        }
    }
    // 3. Fallback to standard single-user path
    "/data/data/com.termux/files/usr".to_string()
}

fn map_path(path: &str) -> String {
    let prefix = get_termux_prefix();
    let mapped = if path.starts_with("/usr/bin/") {
        format!("{}/bin/{}", prefix, &path[9..])
    } else if path.starts_with("/bin/") {
        format!("{}/bin/{}", prefix, &path[5..])
    } else {
        path.to_string()
    };
    if mapped != path {
        log::debug!("map_path: \"{}\" -> \"{}\" (prefix={})", path, mapped, prefix);
    }
    mapped
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
