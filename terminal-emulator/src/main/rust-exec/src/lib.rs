use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;

static REAL_EXECVE: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static CURRENT_PREFIX: OnceLock<String> = OnceLock::new();

const RTLD_NEXT: *mut libc::c_void = -1i64 as *mut libc::c_void;

unsafe extern "C" {
    static environ: *const *const c_char;
}

fn debug_log(msg: &str) {
    let prefix = get_current_prefix();
    let log_path = format!("{}/files/home/termux_exec_debug.log", prefix);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(file, "[{}] {}", std::process::id(), msg);
    }
}

fn get_current_prefix() -> &'static str {
    CURRENT_PREFIX.get_or_init(|| {
        // Find our real prefix by looking at /proc/self/maps or current_dir
        if let Ok(maps) = std::fs::read_to_string("/proc/self/maps") {
            for line in maps.lines() {
                if line.contains("com.termux") && line.contains("/lib/") {
                    if let Some(start) = line.find("/data/") {
                        if let Some(end) = line[start..].find("/files/") {
                            return line[start..start+end].to_string();
                        }
                    }
                }
            }
        }
        // Fallback
        let cwd = std::env::current_dir().unwrap_or_default();
        let cwd_str = cwd.to_string_lossy();
        if cwd_str.contains("/data/user/0/com.termux") {
            "/data/user/0/com.termux".to_string()
        } else {
            "/data/data/com.termux".to_string()
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execve(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    let real_execve: unsafe extern "C" fn(*const c_char, *const *const c_char, *const *const c_char) -> c_int;
    unsafe {
        let mut real_ptr = REAL_EXECVE.load(Ordering::Relaxed);
        if real_ptr.is_null() {
            let name = CStr::from_bytes_with_nul(b"execve\0").unwrap();
            real_ptr = libc::dlsym(RTLD_NEXT, name.as_ptr());
            REAL_EXECVE.store(real_ptr, Ordering::Relaxed);
        }
        real_execve = std::mem::transmute(real_ptr);
    }

    if path.is_null() { return real_execve(path, argv, envp); }
    let path_str = unsafe { CStr::from_ptr(path).to_string_lossy().to_string() };
    
    // Robust check: Is this in a Termux directory?
    if (path_str.contains("com.termux/files/usr/") || path_str.starts_with("/bin/") || path_str.starts_with("/usr/bin/")) 
       && !path_str.contains("/applib/") 
    {
         if let Some((final_cmd, new_argv_vec)) = transform_exec(&path_str, argv) {
             debug_log(&format!("EXECVE: {} -> {}", path_str, final_cmd));
             let c_cmd = CString::new(final_cmd).unwrap();
             let c_ptrs: Vec<*const c_char> = new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
             return real_execve(c_cmd.as_ptr(), c_ptrs.as_ptr(), envp);
         }
    }

    real_execve(path, argv, envp)
}

fn transform_exec(path: &str, orig_argv: *const *const c_char) -> Option<(String, Vec<CString>)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = [0u8; 256];
    let n = file.read(&mut buffer).ok()?;
    let current_prefix = get_current_prefix();
    let linker = if std::path::Path::new("/system/bin/linker64").exists() { "/system/bin/linker64" } else { "/system/bin/linker" };

    if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
        let mut new_argv = Vec::new();
        let arg0 = if unsafe { !orig_argv.is_null() && !(*orig_argv).is_null() } {
            unsafe { CStr::from_ptr(*orig_argv).to_owned() }
        } else { CString::new(path).unwrap() };
        new_argv.push(arg0);
        new_argv.push(CString::new(path).unwrap());
        let mut i = 1;
        unsafe { while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
            new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
            i += 1;
        } }
        return Some((linker.to_string(), new_argv));
    } else if let Some((interpreter, shebang_args)) = parse_shebang_internal(&buffer[..n], current_prefix) {
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(interpreter.clone()).unwrap());
        if let Some(args) = shebang_args { for arg in args.split_whitespace() { new_argv.push(CString::new(arg).unwrap()); } }
        new_argv.push(CString::new(path).unwrap());
        let mut i = 1;
        unsafe { while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
            new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
            i += 1;
        } }
        if interpreter.contains("com.termux") {
             let mut wrapped_argv = Vec::new();
             wrapped_argv.push(new_argv[0].clone());
             wrapped_argv.push(CString::new(interpreter.clone()).unwrap());
             wrapped_argv.extend(new_argv[1..].iter().cloned());
             return Some((linker.to_string(), wrapped_argv));
        }
        return Some((interpreter, new_argv));
    } else {
        let interpreter = format!("{}/files/usr/bin/sh", current_prefix);
        let mut new_argv = Vec::new();
        new_argv.push(CString::new("sh").unwrap());
        new_argv.push(CString::new(interpreter.clone()).unwrap());
        new_argv.push(CString::new(path).unwrap());
        let mut i = 1;
        unsafe { while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
            new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
            i += 1;
        } }
        return Some((linker.to_string(), new_argv));
    }
}

fn parse_shebang_internal(buffer: &[u8], current_prefix: &str) -> Option<(String, Option<String>)> {
    if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' { return None; }
    let line_end = buffer.iter().position(|&b| b == b'\n').unwrap_or(buffer.len());
    let line = String::from_utf8_lossy(&buffer[2..line_end]);
    let trimmed = line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() { return None; }
    let mut interpreter = tokens[0].to_string();
    if interpreter.starts_with("/usr/bin/env") { interpreter = format!("{}/files/usr/bin/env", current_prefix); }
    else if interpreter.starts_with("/bin/") || interpreter.starts_with("/usr/bin/") {
        let binary = interpreter.rsplit('/').next().unwrap_or("sh");
        interpreter = format!("{}/files/usr/bin/{}", current_prefix, binary);
    }
    let args = if tokens.len() > 1 { Some(tokens[1..].join(" ")) } else { None };
    Some((interpreter, args))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execv(path: *const c_char, argv: *const *const c_char) -> c_int { execve(path, argv, environ) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int { execvpe(file, argv, environ) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvpe(file: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int {
    if file.is_null() { return -1; }
    let file_str = unsafe { CStr::from_ptr(file).to_string_lossy() };
    if file_str.contains('/') { return execve(file, argv, envp); }
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(':') {
            let full_path = format!("{}/{}", dir, file_str);
            if std::path::Path::new(&full_path).exists() {
                let c_full_path = CString::new(full_path).unwrap();
                return execve(c_full_path.as_ptr(), argv, envp);
            }
        }
    }
    execve(file, argv, envp)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn(
    pid: *mut libc::pid_t,
    path: *const c_char,
    file_actions: *const libc::c_void,
    attrp: *const libc::c_void,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    if path.is_null() { return libc::EFAULT; }
    let path_str = unsafe { CStr::from_ptr(path).to_string_lossy().to_string() };
    
    if (path_str.contains("com.termux/files/usr/") || path_str.starts_with("/bin/") || path_str.starts_with("/usr/bin/")) 
       && !path_str.contains("/applib/") 
    {
         if let Some((final_cmd, new_argv_vec)) = transform_exec(&path_str, argv) {
             debug_log(&format!("SPAWN: {} -> {}", path_str, final_cmd));
             let c_cmd = CString::new(final_cmd).unwrap();
             let c_ptrs: Vec<*const c_char> = new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
             let name = CStr::from_bytes_with_nul(b"posix_spawn\0").unwrap();
             let real_spawn: unsafe extern "C" fn(*mut libc::pid_t, *const c_char, *const libc::c_void, *const libc::c_void, *const *const c_char, *const *const c_char) -> c_int = 
                std::mem::transmute(libc::dlsym(RTLD_NEXT, name.as_ptr()));
             return real_spawn(pid, c_cmd.as_ptr(), file_actions, attrp, c_ptrs.as_ptr(), envp);
         }
    }

    let name = CStr::from_bytes_with_nul(b"posix_spawn\0").unwrap();
    let real_spawn: unsafe extern "C" fn(*mut libc::pid_t, *const c_char, *const libc::c_void, *const libc::c_void, *const *const c_char, *const *const c_char) -> c_int = 
        std::mem::transmute(libc::dlsym(RTLD_NEXT, name.as_ptr()));
    real_spawn(pid, path, file_actions, attrp, argv, envp)
}
