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

// Simple logging to home directory
fn debug_log(msg: &str) {
    let prefix = get_current_prefix();
    let log_path = format!("{}/files/home/termux_exec_debug.log", prefix);
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path) 
    {
        Ok(f) => f,
        Err(_) => return,
    };
    let _ = writeln!(file, "{}", msg);
}

fn get_current_prefix() -> &'static str {
    CURRENT_PREFIX.get_or_init(|| {
        // Detect current data directory dynamically
        let cwd = std::env::current_dir().unwrap_or_default();
        let cwd_str = cwd.to_string_lossy();
        
        if cwd_str.contains("/data/user/0/com.termux") {
            "/data/user/0/com.termux".to_string()
        } else if cwd_str.contains("/data/data/com.termux") {
            "/data/data/com.termux".to_string()
        } else {
            // Fallback to most likely candidate if unknown
            "/data/user/0/com.termux".to_string()
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execve(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    unsafe {
        let mut real_ptr = REAL_EXECVE.load(Ordering::Relaxed);
        if real_ptr.is_null() {
            let name = CStr::from_bytes_with_nul(b"execve\0").unwrap();
            real_ptr = libc::dlsym(RTLD_NEXT, name.as_ptr());
            if real_ptr.is_null() {
                return -1;
            }
            REAL_EXECVE.store(real_ptr, Ordering::Relaxed);
        }
        let real_execve: unsafe extern "C" fn(*const c_char, *const *const c_char, *const *const c_char) -> c_int = std::mem::transmute(real_ptr);

        if path.is_null() {
             return real_execve(path, argv, envp);
        }

        let path_str = CStr::from_ptr(path).to_string_lossy().to_string();
        
        // 1. W^X Bypass / Script Interception
        // Check for ANY com.termux path
        if path_str.contains("com.termux") && !path_str.contains("/applib/") {
             if let Some((final_cmd, new_argv_vec)) = transform_exec(&path_str, argv) {
                 debug_log(&format!("[EXECVE] Intercepted: {}", path_str));
                 debug_log(&format!("[EXECVE] Transformed to: {} with argv: {:?}", final_cmd, new_argv_vec));
                 let c_cmd = CString::new(final_cmd).unwrap();
                 let c_ptrs: Vec<*const c_char> = new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                 return real_execve(c_cmd.as_ptr(), c_ptrs.as_ptr(), envp);
             }
        }

        real_execve(path, argv, envp)
    }
}

fn transform_exec(path: &str, orig_argv: *const *const c_char) -> Option<(String, Vec<CString>)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = [0u8; 256];
    let n = file.read(&mut buffer).ok()?;

    let current_prefix = get_current_prefix();

    let linker = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
        // --- ELF File ---
        let mut new_argv = Vec::new();
        let arg0 = if unsafe { !orig_argv.is_null() && !(*orig_argv).is_null() } {
            unsafe { CStr::from_ptr(*orig_argv).to_owned() }
        } else {
            CString::new(path).unwrap()
        };
        new_argv.push(arg0);
        new_argv.push(CString::new(path).unwrap()); // argv[1] is the target binary for linker64
        
        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }
        return Some((linker.to_string(), new_argv));
    } else if let Some((interpreter, shebang_args)) = parse_shebang_internal(&buffer[..n], current_prefix) {
        // --- Shebang Script ---
        let mut new_argv = Vec::new();
        
        // If the interpreter is in Termux dir, it's an ELF that needs wrapping
        if interpreter.contains("com.termux") {
             new_argv.push(CString::new(interpreter.clone()).unwrap()); // process name
             new_argv.push(CString::new(interpreter.clone()).unwrap()); // linker target
             
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
        } else {
             // System interpreter (e.g. /system/bin/sh)
             new_argv.push(CString::new(interpreter.clone()).unwrap());
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
             return Some((interpreter, new_argv));
        }
    } else {
        // --- Neither ELF nor Shebang ---
        let interpreter = format!("{}/files/usr/bin/sh", current_prefix);
        let mut new_argv = Vec::new();
        new_argv.push(CString::new("sh").unwrap());
        new_argv.push(CString::new(interpreter.clone()).unwrap()); 
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
}

fn parse_shebang_internal(buffer: &[u8], current_prefix: &str) -> Option<(String, Option<String>)> {
    if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' {
        return None;
    }
    let line_end = buffer.iter().position(|&b| b == b'\n').unwrap_or(buffer.len());
    let line = String::from_utf8_lossy(&buffer[2..line_end]);
    let trimmed = line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() { return None; }
    
    let mut interpreter = tokens[0].to_string();
    if interpreter.starts_with("/usr/bin/env") {
        interpreter = format!("{}/files/usr/bin/env", current_prefix);
    } else if interpreter.starts_with("/bin/") || interpreter.starts_with("/usr/bin/") {
        let binary = interpreter.rsplit('/').next().unwrap_or("sh");
        interpreter = format!("{}/files/usr/bin/{}", current_prefix, binary);
    }
    
    let args = if tokens.len() > 1 { Some(tokens[1..].join(" ")) } else { None };
    Some((interpreter, args))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execv(path: *const c_char, argv: *const *const c_char) -> c_int {
    unsafe {
        execve(path, argv, environ as *const *const c_char)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int {
    unsafe {
        execvpe(file, argv, environ as *const *const c_char)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvpe(file: *const c_char, argv: *const *const c_char, envp: *const *const c_char, ) -> c_int {
    unsafe {
        if !file.is_null() && *file == b'/' as c_char {
            return execve(file, argv, envp);
        }
        
        let file_str = CStr::from_ptr(file).to_string_lossy();
        if file_str.contains('/') {
            return execve(file, argv, envp);
        }

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
}
