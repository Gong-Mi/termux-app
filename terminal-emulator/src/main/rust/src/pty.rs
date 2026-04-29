use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::unistd::{ForkResult, fork, setsid, chdir};
use std::ffi::CString;
use std::io::Read;

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
    // Ensure HOME is set to Termux home, not Android root (/).
    // Java layer passes ENV_HOME = "/" which breaks ~ expansion.
    let termux_home = "/data/data/com.termux/files/home";
    if has_home {
        if let Some(idx) = final_env.iter().position(|s| s.starts_with("HOME=")) {
            final_env[idx] = format!("HOME={}", termux_home);
        } else {
            final_env.push(format!("HOME={}", termux_home));
        }
    } else {
        final_env.push(format!("HOME={}", termux_home));
    }
    // Ensure PATH always includes Termux bin directories.
    // Java layer passes System.getenv("PATH") which is the Android system PATH
    // and lacks Termux paths. We prepend Termux paths to maintain priority.
    let termux_paths = "/data/data/com.termux/files/usr/bin:/data/data/com.termux/files/usr/bin/applets";
    let new_path = if has_path {
        if let Some(existing) = final_env.iter().find(|s| s.starts_with("PATH=")) {
            let existing_val = existing.strip_prefix("PATH=").unwrap_or("");
            if existing_val.contains(termux_paths.split(':').next().unwrap_or("")) {
                existing.clone()
            } else {
                format!("PATH={}:{}:{}", termux_paths, existing_val, "/system/bin")
            }
        } else {
            format!("PATH={}:/system/bin", termux_paths)
        }
    } else {
        format!("PATH={}:/system/bin", termux_paths)
    };
    if has_path {
        if let Some(idx) = final_env.iter().position(|s| s.starts_with("PATH=")) {
            final_env[idx] = new_path;
        } else {
            final_env.push(new_path);
        }
    } else {
        final_env.push(new_path);
    }
    if !has_ld_preload {
        // Use the real file instead of symlink to avoid preload resolution issues
        final_env.push("LD_PRELOAD=/data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so".to_string());
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
    // Prefer ELF binaries over shebang scripts to avoid linker wrapper issues
    if real_cmd.is_empty() {
        // Prefer ELF binaries over shebang scripts for faster startup
        // and to reduce linker-wrapper surface area.
        let default_shells = [
            "/data/data/com.termux/files/usr/bin/bash",
            "/data/data/com.termux/files/usr/bin/dash",
            "/data/data/com.termux/files/usr/bin/sh",
            "/system/bin/sh",
            "/data/data/com.termux/files/usr/bin/login",
        ];
        
        for shell in &default_shells {
            if std::path::Path::new(shell).exists() {
                real_cmd = shell.to_string();
                // Replace argv entirely; Java layer may have passed a stale
                // process name (e.g. "-login") that doesn't match the shell
                // we actually selected.
                let process_name = if shell.ends_with("login") {
                    "-login".to_string()
                } else {
                    shell.to_string()
                };
                real_argv = vec![process_name];
                android_log(LogPriority::INFO, &format!("[TRACE_SESSION] No command provided, selected default shell: {}", real_cmd));
                break;
            }
        }
    }

    android_log(LogPriority::INFO, &format!("[TRACE_SESSION] Preparing to exec: {} with argv: {:?}", real_cmd, real_argv));

    let cmd_log = real_cmd.clone();

    // Parse a shebang line into (interpreter_path, optional_args).
    // Handles spaces in the shebang, e.g. "#!/usr/bin/env bash" -> ("/usr/bin/env", Some("bash"))
    fn parse_shebang(buffer: &[u8]) -> Option<(String, Option<String>)> {
        if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' {
            return None;
        }
        // Shebang line ends at first newline (or end of buffer)
        let line_end = buffer.iter().position(|&b| b == b'\n').unwrap_or(buffer.len());
        let line = String::from_utf8_lossy(&buffer[2..line_end]);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() {
            return None;
        }
        let interpreter = tokens[0].to_string();
        let args = if tokens.len() > 1 {
            Some(tokens[1..].join(" "))
        } else {
            None
        };
        Some((interpreter, args))
    }

    // Map an interpreter path to the Termux prefix, matching upstream logic.
    fn map_interpreter(interp: &str, normalize: &dyn Fn(String) -> String) -> String {
        if interp.starts_with("/usr/bin/env") {
            "/data/data/com.termux/files/usr/bin/env".to_string()
        } else if interp.starts_with("/bin/") || interp.starts_with("/usr/bin/") {
            let binary = interp.rsplit('/').next().unwrap_or("sh");
            format!("/data/data/com.termux/files/usr/bin/{}", binary)
        } else if interp.starts_with("/data/data/com.termux/") || interp.starts_with("/data/user/0/com.termux/") {
            normalize(interp.to_string())
        } else {
            interp.to_string()
        }
    }

    // Read the first 256 bytes of the target file to determine ELF / shebang / plain script.
    let (final_cmd, final_argv) = if let Ok(mut file) = std::fs::File::open(&real_cmd) {
        use std::io::Read;
        let mut buffer = [0u8; 256];
        if let Ok(n) = file.read(&mut buffer) {
            if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
                // ELF file - execute directly.
                // argv[0] is already set (e.g. "-login" or the binary name).
                (real_cmd, real_argv)
            } else if let Some((raw_interpreter, shebang_args)) = parse_shebang(&buffer[..n]) {
                // Shebang detected.  The interpreter becomes the real executable;
                // the original script path is passed as an argument.
                let interpreter = map_interpreter(&raw_interpreter, &normalize_path);

                // argv[0] = process name (already in real_argv[0], e.g. "-login")
                // argv[1..] = shebang args (if any, e.g. "bash")
                // argv[...] = original script path
                // argv[...] = user-supplied args (real_argv[1..])
                let mut new_argv = Vec::new();
                if !real_argv.is_empty() {
                    new_argv.push(real_argv[0].clone()); // process name
                }
                if let Some(ref args) = shebang_args {
                    new_argv.push(args.clone());
                }
                new_argv.push(real_cmd.clone()); // script path
                if real_argv.len() > 1 {
                    new_argv.extend(real_argv[1..].iter().cloned());
                }

                android_log(LogPriority::INFO, &format!(
                    "[PTY] Shebang detected: interpreter={}, args={:?}, script={}, new_argv={:?}",
                    interpreter, shebang_args, real_cmd, new_argv
                ));
                (interpreter, new_argv)
            } else {
                // No shebang and no ELF - default to $PREFIX/bin/sh.
                let interpreter = "/data/data/com.termux/files/usr/bin/sh".to_string();
                let mut new_argv = Vec::new();
                if !real_argv.is_empty() {
                    new_argv.push(real_argv[0].clone()); // process name
                }
                new_argv.push(real_cmd.clone()); // script path
                if real_argv.len() > 1 {
                    new_argv.extend(real_argv[1..].iter().cloned());
                }
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
    // Only wrap actual ELF binaries with the system linker; skip shebang scripts
    let is_elf = std::fs::File::open(&final_cmd)
        .and_then(|mut f| {
            let mut buf = [0u8; 4];
            f.read_exact(&mut buf)?;
            Ok(buf[0] == 0x7F && buf[1] == b'E' && buf[2] == b'L' && buf[3] == b'F')
        })
        .unwrap_or(false);
    let use_linker_wrapper = is_elf && final_cmd.starts_with("/data/data/com.termux/");
    let linker_path = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    let (exec_cmd, exec_argv) = if use_linker_wrapper {
        // linker64 loads argv[1] as the ELF target, NOT argv[0].
        // Correct argv layout: [process_name, target_elf, ...remaining_args]
        let mut wrapped_argv = vec![];
        if !final_argv.is_empty() {
            wrapped_argv.push(final_argv[0].clone()); // process name (e.g. "-login")
        } else {
            wrapped_argv.push(final_cmd.clone()); // fallback process name
        }
        wrapped_argv.push(final_cmd.clone()); // target ELF as argv[1] for linker64
        if final_argv.len() > 1 {
            wrapped_argv.extend(final_argv[1..].iter().cloned());
        }
        android_log(LogPriority::INFO, &format!("[PTY] W^X Bypass: execvp({}, {:?})", linker_path, wrapped_argv));
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
