use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::unistd::{ForkResult, fork, setsid, chdir};
use std::ffi::CString;

use crate::utils::{android_log, LogPriority};

pub unsafe fn create_subprocess(
    env: &mut JNIEnv,
    cmd: jstring,
    cwd: jstring,
    args: jobjectArray,
    env_vars: jobjectArray,
    process_id_array: jintArray,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
) -> jint {
    let cmd_str = if !cmd.is_null() {
        let js = unsafe { JString::from_raw(cmd) };
        env.get_string(&js).map(|s| s.into()).unwrap_or_default()
    } else { String::new() };

    let cwd_str = if !cwd.is_null() {
        let js = unsafe { JString::from_raw(cwd) };
        env.get_string(&js).map(|s| s.into()).unwrap_or_default()
    } else { String::new() };

    let mut argv = Vec::new();
    let args_obj = unsafe { JObjectArray::from_raw(args) };
    if !args_obj.is_null() {
        if let Ok(len) = env.get_array_length(&args_obj) {
            for i in 0..len {
                if let Ok(arg_obj) = env.get_object_array_element(&args_obj, i) {
                    let arg_java: JString = arg_obj.into();
                    if let Ok(s) = env.get_string(&arg_java) { argv.push(String::from(s)); }
                }
            }
        }
    }

    let mut envp = Vec::new();
    let env_vars_obj = unsafe { JObjectArray::from_raw(env_vars) };
    if !env_vars_obj.is_null() {
        if let Ok(len) = env.get_array_length(&env_vars_obj) {
            for i in 0..len {
                if let Ok(env_obj) = env.get_object_array_element(&env_vars_obj, i) {
                    let env_java: JString = env_obj.into();
                    if let Ok(s) = env.get_string(&env_java) { envp.push(String::from(s)); }
                }
            }
        }
    }

    match create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, cols, cw, ch) {
        Ok((fd, pid)) => {
            let pid_val = [pid as jint];
            let j_pid_array = unsafe { JIntArray::from_raw(process_id_array) };
            let _ = env.set_int_array_region(&j_pid_array, 0, &pid_val);
            fd
        }
        Err(_) => -1,
    }
}

pub fn create_subprocess_with_data(
    cmd_str: String,
    cwd_str: String,
    argv: Vec<String>,
    envp: Vec<String>,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
) -> Result<(jint, i32), ()> {
    let normalize_path = |path: String| -> String {
        if path.starts_with("/data/user/0/com.termux") {
            path.replace("/data/user/0/com.termux", "/data/data/com.termux")
        } else {
            path
        }
    };

    let cmd_str = normalize_path(cmd_str);
    let cwd_str = normalize_path(cwd_str);
    let argv: Vec<String> = argv.into_iter().map(normalize_path).collect();
    let envp: Vec<String> = envp.into_iter().map(normalize_path).collect();

    let mut final_env = envp.clone();
    
    // Ensure critical Termux environment variables are set if missing
    let has_prefix = final_env.iter().any(|s| s.starts_with("PREFIX="));
    let has_home = final_env.iter().any(|s| s.starts_with("HOME="));
    let has_path = final_env.iter().any(|s| s.starts_with("PATH="));
    let has_ld_preload = final_env.iter().any(|s| s.starts_with("LD_PRELOAD="));

    if !has_prefix {
        final_env.push("PREFIX=/data/data/com.termux/files/usr".to_string());
    }
    if !has_home {
        final_env.push("HOME=/data/data/com.termux/files/home".to_string());
    }
    if !has_path {
        final_env.push("PATH=/data/data/com.termux/files/usr/bin".to_string());
    }
    if !has_ld_preload {
        // Essential for termux-exec to work
        final_env.push("LD_PRELOAD=/data/data/com.termux/files/usr/lib/libtermux-exec.so".to_string());
    }
    
    // Add other defaults
    if !final_env.iter().any(|s| s.starts_with("TERM=")) {
        final_env.push("TERM=xterm-256color".to_string());
    }
    if !final_env.iter().any(|s| s.starts_with("COLORTERM=")) {
        final_env.push("COLORTERM=truecolor".to_string());
    }
    if !final_env.iter().any(|s| s.starts_with("LANG=")) {
        final_env.push("LANG=en_US.UTF-8".to_string());
    }
    if !final_env.iter().any(|s| s.starts_with("TMPDIR=")) {
        final_env.push("TMPDIR=/data/data/com.termux/files/usr/tmp".to_string());
    }

    let mut real_cmd = cmd_str.clone();
    let mut real_argv = argv.clone();

    // Default shell selection logic moved to Rust to reduce Java code
    if real_cmd.is_empty() {
        let default_shells = [
            "/data/data/com.termux/files/usr/bin/login",
            "/data/data/com.termux/files/usr/bin/bash",
            "/data/data/com.termux/files/usr/bin/sh",
            "/system/bin/sh",
        ];
        
        for shell in &default_shells {
            if std::path::Path::new(shell).exists() {
                real_cmd = shell.to_string();
                // If it's a login shell, prefix argv[0] with '-'
                if shell.ends_with("login") {
                    real_argv.insert(0, "-login".to_string());
                } else {
                    real_argv.insert(0, shell.to_string());
                }
                android_log(LogPriority::INFO, &format!("[TRACE_SESSION] No command provided, selected default shell: {}", real_cmd));
                break;
            }
        }
    }

    android_log(LogPriority::INFO, &format!("[TRACE_SESSION] Preparing to exec: {} with argv: {:?}", real_cmd, real_argv));

    let cmd_log = real_cmd.clone();
    
    // Shebang and ELF parsing logic in Rust (matches upstream TermuxShellUtils)
    let (final_cmd, final_argv) = if let Ok(mut file) = std::fs::File::open(&real_cmd) {
        use std::io::Read;
        let mut buffer = [0u8; 256];
        if let Ok(n) = file.read(&mut buffer) {
            if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
                // ELF file - execute directly
                (real_cmd, real_argv)
            } else if n > 2 && buffer[0] == b'#' && buffer[1] == b'!' {
                // Shebang detected
                let line = String::from_utf8_lossy(&buffer[2..n]);
                if let Some(first_line) = line.lines().next() {
                    let shebang = first_line.trim();
                    if !shebang.is_empty() {
                        let mut new_argv = vec![real_cmd.clone()];
                        new_argv.extend(real_argv.clone());
                        
                        let interpreter = if shebang.starts_with("/usr/bin/env") {
                            "/data/data/com.termux/files/usr/bin/env".to_string()
                        } else if shebang.starts_with("/bin/") || shebang.starts_with("/usr/bin/") {
                            let parts: Vec<&str> = shebang.split('/').collect();
                            format!("/data/data/com.termux/files/usr/bin/{}", parts.last().unwrap_or(&"sh"))
                        } else {
                            normalize_path(shebang.to_string())
                        };
                        
                        android_log(LogPriority::INFO, &format!("[PTY] Shebang detected, using interpreter: {}", interpreter));
                        (interpreter, new_argv)
                    } else {
                        (real_cmd, real_argv)
                    }
                } else {
                    (real_cmd, real_argv)
                }
            } else {
                // No shebang and no ELF - default to shell
                let interpreter = "/data/data/com.termux/files/usr/bin/sh".to_string();
                let mut new_argv = vec![real_cmd.clone()];
                new_argv.extend(real_argv.clone());
                android_log(LogPriority::INFO, &format!("[PTY] No shebang/ELF, defaulting to shell: {}", interpreter));
                (interpreter, new_argv)
            }
        } else {
            (real_cmd, real_argv)
        }
    } else {
        (real_cmd, real_argv)
    };

    let c_envs: Vec<CString> = final_env.iter().map(|e| CString::new(e.clone()).unwrap()).collect();

    // Linker Wrapper Bypass for Android 10+ (W^X)
    // If the command is in /data/data/, we must execute it via the system linker
    let use_linker_wrapper = final_cmd.starts_with("/data/data/com.termux/");
    let linker_path = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    let (exec_cmd, exec_argv) = if use_linker_wrapper {
        let mut wrapped_argv = vec![final_cmd.clone()];
        wrapped_argv.extend(final_argv);
        android_log(LogPriority::INFO, &format!("[PTY] W^X Bypass: Wrapping {} with {}", final_cmd, linker_path));
        (linker_path.to_string(), wrapped_argv)
    } else {
        (final_cmd, final_argv)
    };

    let c_exec_cmd = CString::new(exec_cmd).unwrap();
    let c_exec_args: Vec<CString> = exec_argv.iter().map(|a| CString::new(a.clone()).unwrap()).collect();

    unsafe {
        let ptm = libc::open("/dev/ptmx\0".as_ptr() as *const _, libc::O_RDWR | libc::O_CLOEXEC);
        if ptm < 0 { return Err(()); }

        let _ = libc::grantpt(ptm);
        let _ = libc::unlockpt(ptm);
        let devname = libc::ptsname(ptm);
        if devname.is_null() { return Err(()); }
        let devname_str = std::ffi::CStr::from_ptr(devname).to_string_lossy().into_owned();

        // Set initial winsize.
        let sz = libc::winsize {
            ws_row: rows as u16,
            ws_col: cols as u16,
            ws_xpixel: (cols as u32 * cw as u32) as u16,
            ws_ypixel: (rows as u32 * ch as u32) as u16,
        };
        libc::ioctl(ptm, libc::TIOCSWINSZ, &sz);

        // Enable UTF-8 mode and disable flow control.
        let mut tios: libc::termios = std::mem::zeroed();
        libc::tcgetattr(ptm, &mut tios);
        tios.c_iflag |= libc::IUTF8;
        tios.c_iflag &= !(libc::IXON | libc::IXOFF);
        libc::tcsetattr(ptm, libc::TCSANOW, &tios);

        match fork() {
            Ok(ForkResult::Parent { child }) => Ok((ptm, child.as_raw())),
            Ok(ForkResult::Child) => {
                // Clear signals.
                let mut signals_to_unblock: libc::sigset_t = std::mem::zeroed();
                libc::sigfillset(&mut signals_to_unblock);
                libc::sigprocmask(libc::SIG_UNBLOCK, &signals_to_unblock, std::ptr::null_mut());

                libc::close(ptm);
                let _ = setsid();

                let c_pts = CString::new(devname_str).unwrap();
                let pts = libc::open(c_pts.as_ptr(), libc::O_RDWR);
                if pts < 0 { libc::_exit(-1); }

                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);
                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);

                if pts > 2 { libc::close(pts); }

                // Close inherited file descriptors (except stdio) to match upstream behavior.
                // Use raw libc::opendir/readdir/closedir (not Rust std::fs::read_dir) so we
                // can call dirfd() to exclude the directory's own fd from being closed.
                let self_dir = libc::opendir(b"/proc/self/fd\0".as_ptr() as *const _);
                if !self_dir.is_null() {
                    let self_dir_fd = libc::dirfd(self_dir);
                    loop {
                        let entry = libc::readdir(self_dir);
                        if entry.is_null() { break; }
                        let name_ptr = (*entry).d_name.as_ptr();
                        let name = std::ffi::CStr::from_ptr(name_ptr).to_string_lossy();
                        if let Ok(fd) = name.parse::<i32>() {
                            if fd > 2 && fd != self_dir_fd {
                                libc::close(fd);
                            }
                        }
                    }
                    libc::closedir(self_dir);
                }

                // Clear environment and rebuild.
                libc::clearenv();
                for env_str in &c_envs {
                    libc::putenv(env_str.as_ptr() as *mut _);
                }

                // Change directory.
                if !cwd_str.is_empty() {
                    if let Ok(c_cwd) = CString::new(cwd_str) {
                        let _ = chdir(c_cwd.as_c_str());
                    }
                }

                let ptr_args: Vec<_> = c_exec_args.iter().map(|s| s.as_ptr())
                    .chain(std::iter::once(std::ptr::null())).collect();

                // Use execvp to search PATH and match upstream behavior
                libc::execvp(c_exec_cmd.as_ptr(), ptr_args.as_ptr());

                // --- If we reach here, execvp failed ---
                let err = std::io::Error::last_os_error();
                let errno = err.raw_os_error().unwrap_or(0);
                let err_msg = format!("\r\n[Termux] execvp(\"{}\") failed: {} (errno={})\r\n", 
                    cmd_log, 
                    err,
                    errno
                );
                libc::write(2, err_msg.as_ptr() as *const _, err_msg.len());

                // Fallback to /system/bin/sh as last resort
                android_log(LogPriority::WARN, &format!("[TRACE_SESSION] execvp failed (errno={}), falling back to /system/bin/sh", errno));
                
                let fallback_cmd = CString::new("/system/bin/sh").unwrap();
                let fallback_arg0 = CString::new("sh").unwrap();
                let fallback_args = [fallback_arg0.as_ptr(), std::ptr::null()];
                
                libc::execvp(fallback_cmd.as_ptr(), fallback_args.as_ptr());
                libc::_exit(1);
            }
            Err(_) => Err(()),
        }
    }
}

pub fn write_to_fd(fd: jint, data: &[u8]) -> jint {
    if fd < 0 { return -1; }
    let res = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
    res as jint
}

pub fn spawn_waiter(pid: i32, callback: jni::objects::GlobalRef) {
    std::thread::spawn(move || {
        let exit_code = wait_for(pid);
        android_log(LogPriority::INFO, &format!("[PTY Waiter] Process {} exited with status {}", pid, exit_code));
        
        if let Some(vm) = crate::JAVA_VM.get() {
            if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                // Call Java callback to notify about exit
                // Assuming callback is the RustEngineCallback or TerminalSession
                let _ = env.call_method(
                    callback.as_obj(),
                    "onProcessExited",
                    "(I)V",
                    &[jni::objects::JValue::Int(exit_code)]
                );
            }
        }
    });
}

pub fn set_pty_window_size(fd: jint, rows: jint, cols: jint, cell_width: jint, cell_height: jint) {
    if fd < 0 { return; }
    let sz = libc::winsize {
        ws_row: rows as u16,
        ws_col: cols as u16,
        ws_xpixel: (cols as u32 * cell_width as u32) as u16,
        ws_ypixel: (rows as u32 * cell_height as u32) as u16,
    };
    unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &sz); }
}

pub fn wait_for(pid: i32) -> jint {
    let mut status: i32 = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
        if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) }
        else if libc::WIFSIGNALED(status) { -libc::WTERMSIG(status) }
        else { 0 }
    }
}
