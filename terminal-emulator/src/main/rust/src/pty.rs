use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;
use nix::unistd::{ForkResult, chdir, close, fork, setsid};
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicI32, Ordering};

// 全局活跃进程计数器
static ACTIVE_CHILD_COUNT: AtomicI32 = AtomicI32::new(0);

// 安卓 14/15 的 Phantom Killer 阈值为 32，我们预留余量，限制在 28。
const MAX_CONCURRENT_SUBPROCESSES: i32 = 28;

// Android 上的 PTY 辅助函数
// ... (rest of extern C)
unsafe extern "C" {
    fn grantpt(fd: i32) -> i32;
    fn unlockpt(fd: i32) -> i32;
    fn ptsname_r(fd: i32, buf: *mut libc::c_char, buflen: usize) -> i32;
}

/// # Safety
///
/// This function performs low-level process creation and PTY operations.
#[allow(clippy::too_many_arguments)]
pub unsafe fn create_subprocess(
    env_ptr: *mut *const JNINativeInterface_,
    cmd: jstring,
    cwd: jstring,
    args: jobjectArray,
    env_vars: jobjectArray,
    process_id_array: jintArray,
    rows: jint,
    columns: jint,
    cell_width: jint,
    cell_height: jint,
) -> jint {
    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return -1,
    };

    let cmd_str = if !cmd.is_null() {
        let js = unsafe { JString::from_raw(cmd) };
        env.get_string(&js).map(|s| s.into()).unwrap_or_default()
    } else {
        String::new()
    };

    let cwd_str = if !cwd.is_null() {
        let js = unsafe { JString::from_raw(cwd) };
        env.get_string(&js).map(|s| s.into()).unwrap_or_default()
    } else {
        String::new()
    };

    let mut argv = Vec::new();
    let args_obj = unsafe { JObjectArray::from_raw(args) };
    if !args_obj.is_null() {
        if let Ok(len) = env.get_array_length(&args_obj) {
            for i in 0..len {
                if let Ok(arg_obj) = env.get_object_array_element(&args_obj, i) {
                    let arg_java: JString = arg_obj.into();
                    if let Ok(s) = env.get_string(&arg_java) {
                        argv.push(String::from(s));
                    }
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
                    if let Ok(s) = env.get_string(&env_java) {
                        envp.push(String::from(s));
                    }
                }
            }
        }
    }

    let (ptm, pid) = match create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, columns, cell_width, cell_height) {
        Ok(res) => res,
        Err(_) => return -1,
    };

    let pid_buf = [pid];
    let j_pid_array = unsafe { JIntArray::from_raw(process_id_array) };
    let _ = env.set_int_array_region(&j_pid_array, 0, &pid_buf);
    ptm as jint
}

/// 获取当前 UID 下的所有进程总数 (通过扫描 /proc)
fn get_total_uid_process_count() -> i32 {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir("/proc") {
        let my_uid = unsafe { libc::getuid() };
        for entry in entries.flatten() {
            if let Ok(file_name) = entry.file_name().into_string() {
                if file_name.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(metadata) = std::fs::metadata(entry.path()) {
                        use std::os::unix::fs::MetadataExt;
                        if metadata.uid() == my_uid {
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    count
}

pub fn create_subprocess_with_data(
    cmd_str: String,
    cwd_str: String,
    argv: Vec<String>,
    envp: Vec<String>,
    rows: jint,
    columns: jint,
    cell_width: jint,
    cell_height: jint,
) -> Result<(i32, i32), ()> {
    // 1. 进程流控 (Governor)
    let current_count = ACTIVE_CHILD_COUNT.load(Ordering::SeqCst);
    let total_uid_count = get_total_uid_process_count();
    
    // 如果总进程数接近 32 (Phantom Killer 阈值)，或者 Termux 自身产生的进程过多，强行限流排队
    if total_uid_count >= MAX_CONCURRENT_SUBPROCESSES || current_count >= (MAX_CONCURRENT_SUBPROCESSES - 4) {
        crate::utils::android_log(
            crate::utils::LogPriority::WARN, 
            &format!("GOVERNOR: UID PIDs: {}, Termux PIDs: {}. Throttling fork (limit {})...", total_uid_count, current_count, MAX_CONCURRENT_SUBPROCESSES)
        );
        // 睡眠等待，给系统喘息机会
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // 2. 打开 PTM
    use std::os::fd::IntoRawFd;
    let ptm = match open("/dev/ptmx", OFlag::O_RDWR, Mode::empty()) {
        Ok(fd) => fd.into_raw_fd(),
        Err(_) => return Err(()),
    };

    unsafe {
        if grantpt(ptm) != 0 || unlockpt(ptm) != 0 {
            let _ = close(ptm);
            return Err(());
        }

        let mut devname_buf = [0u8; 64];
        if ptsname_r(ptm, devname_buf.as_mut_ptr() as *mut libc::c_char, devname_buf.len()) != 0 {
            let _ = close(ptm);
            return Err(());
        }
        let devname = match CStr::from_ptr(devname_buf.as_ptr() as *const libc::c_char).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => { let _ = close(ptm); return Err(()); }
        };

        // 2. 设置初始 winsize
        let sz = libc::winsize {
            ws_row: rows as u16,
            ws_col: columns as u16,
            ws_xpixel: (columns * cell_width) as u16,
            ws_ypixel: (rows * cell_height) as u16,
        };
        libc::ioctl(ptm, libc::TIOCSWINSZ, &sz);

        // 3. Fork
        match fork() {
            Ok(ForkResult::Parent { child }) => {
                ACTIVE_CHILD_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                let _ = setsid();
                
                // 降低子进程优先级 (Nice 19)，减少对系统负载的冲击，从而规避 Phantom Killer
                libc::setpriority(libc::PRIO_PROCESS, 0, 19);

                let c_devname = CString::new(devname).unwrap();
                let pts = libc::open(c_devname.as_ptr(), libc::O_RDWR);
                if pts < 0 { libc::_exit(1); }

                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);

                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);
                if pts > 2 { libc::close(pts); }
                libc::close(ptm);

                // 彻底确保 Termux 的核心环境变量
                let termux_data = "/data/data/com.termux";
                let termux_files = format!("{}/files", termux_data);
                let termux_prefix = format!("{}/usr", termux_files);
                let termux_bin = format!("{}/bin", termux_prefix);
                let termux_lib = format!("{}/lib", termux_prefix);
                
                let mut final_envp = envp.clone();
                
                // 1. PATH
                let default_path = format!("PATH={}:/system/bin:/system/xbin", termux_bin);
                if let Some(pos) = final_envp.iter().position(|s| s.starts_with("PATH=")) {
                    let old_path = final_envp[pos].split('=').nth(1).unwrap_or("");
                    if !old_path.contains(&termux_bin) {
                        final_envp[pos] = format!("PATH={}:{}", termux_bin, old_path);
                    }
                } else {
                    final_envp.push(default_path);
                }

                // 2. LD_PRELOAD (关键：确保子进程 W^X 绕过)
                let preload_val = format!("{}/libtermux-exec.so", termux_lib);
                if !final_envp.iter().any(|s| s.starts_with("LD_PRELOAD=")) {
                    final_envp.push(format!("LD_PRELOAD={}", preload_val));
                }

                // 3. libtermux-exec 必须的上下文变量
                if !final_envp.iter().any(|s| s.starts_with("TERMUX_APP__DATA_DIR=")) {
                    final_envp.push(format!("TERMUX_APP__DATA_DIR={}", termux_data));
                }
                if !final_envp.iter().any(|s| s.starts_with("TERMUX__PREFIX=")) {
                    final_envp.push(format!("TERMUX__PREFIX={}", termux_prefix));
                }
                if !final_envp.iter().any(|s| s.starts_with("LD_LIBRARY_PATH=")) {
                    final_envp.push(format!("LD_LIBRARY_PATH={}", termux_lib));
                }

                // 4. 基础变量
                if !final_envp.iter().any(|s| s.starts_with("TERM=")) { final_envp.push("TERM=xterm-256color".to_string()); }
                if !final_envp.iter().any(|s| s.starts_with("HOME=")) { final_envp.push(format!("HOME={}/home", termux_files)); }
                if !final_envp.iter().any(|s| s.starts_with("PREFIX=")) { final_envp.push(format!("PREFIX={}", termux_prefix)); }

                libc::clearenv();
                for env_var in final_envp {
                    if let Ok(c_env) = CString::new(env_var) {
                        libc::putenv(c_env.into_raw());
                    }
                }

                if !cwd_str.is_empty() {
                    let c_cwd = CString::new(cwd_str.clone()).unwrap();
                    let _ = chdir(c_cwd.as_c_str());
                }

                let termux_data = "/data/data/com.termux";
                let termux_files = format!("{}/files", termux_data);
                let termux_prefix = format!("{}/usr", termux_files);
                let termux_bin = format!("{}/bin", termux_prefix);
                let termux_lib = format!("{}/lib", termux_prefix);

                let mut final_cmd = cmd_str.clone();
                // 关键修复：确保 final_cmd 是绝对路径
                if !final_cmd.starts_with('/') {
                    let resolved = format!("{}/{}", termux_bin, final_cmd);
                    if std::path::Path::new(&resolved).exists() {
                        final_cmd = resolved;
                    }
                }
                
                let mut final_args = argv.clone();
                if !final_args.is_empty() && !final_args[0].starts_with('/') && !final_args[0].starts_with('-') {
                    let resolved = format!("{}/{}", termux_bin, final_args[0]);
                    if std::path::Path::new(&resolved).exists() {
                        final_args[0] = resolved;
                    }
                }
                
                // 只有目标是常见的 shell 时，才自动纠正为 Login Shell (argv[0] 带 -)
                let is_shell = ["sh", "bash", "zsh", "dash", "fish"].iter().any(|&s| final_cmd.ends_with(s));
                if is_shell {
                    if final_args.is_empty() {
                        let name = std::path::Path::new(&final_cmd)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("sh");
                        final_args.push(format!("-{}", name));
                    } else if !final_args[0].starts_with('-') {
                        final_args[0] = format!("-{}", final_args[0]);
                    }
                }

                // Parse ELF / Shebang
                if let Ok(mut f) = std::fs::File::open(&final_cmd) {
                    use std::io::Read;
                    let mut buf = [0u8; 256];
                    if let Ok(bytes_read) = f.read(&mut buf) {
                        if bytes_read > 4 && buf[0] == 0x7F && buf[1] == b'E' && buf[2] == b'L' && buf[3] == b'F' {
                            // ELF file, do nothing
                        } else if bytes_read > 2 && buf[0] == b'#' && buf[1] == b'!' {
                            // Parse shebang
                            if let Ok(shebang) = std::str::from_utf8(&buf[2..bytes_read]) {
                                let interpreter_line = shebang.lines().next().unwrap_or("").trim();
                                if !interpreter_line.is_empty() {
                                    let parts: Vec<&str> = interpreter_line.split_whitespace().collect();
                                    if !parts.is_empty() {
                                        let mut interpreter = parts[0].to_string();
                                        
                                        // 关键修复：将相对路径或系统路径转换为 Termux 绝对路径
                                        if interpreter.starts_with("/usr/bin/") || interpreter.starts_with("/bin/") {
                                            let binary_name = std::path::Path::new(&interpreter).file_name().and_then(|s| s.to_str()).unwrap_or("");
                                            interpreter = format!("{}/bin/{}", termux_prefix, binary_name);
                                        }

                                        let old_cmd = final_cmd.clone();
                                        final_cmd = interpreter;
                                        
                                        // 构造新的 argv: [interpreter, interpreter_args..., script_path, original_script_args...]
                                        let mut new_argv = Vec::new();
                                        new_argv.push(final_cmd.clone()); // argv[0] for interpreter
                                        for p in parts.iter().skip(1) { new_argv.push(p.to_string()); }
                                        new_argv.push(old_cmd); // script path
                                        if argv.len() > 1 {
                                            new_argv.extend(argv.iter().skip(1).cloned());
                                        }
                                        final_args = new_argv;
                                    }
                                }
                            }
                        } else {
                            // Not ELF and no shebang, default to Termux sh
                            let old_cmd = final_cmd.clone();
                            final_cmd = format!("{}/bin/sh", termux_prefix);
                            let mut new_argv = Vec::new();
                            new_argv.push(final_cmd.clone());
                            new_argv.push(old_cmd);
                            if argv.len() > 1 {
                                new_argv.extend(argv.iter().skip(1).cloned());
                            }
                            final_args = new_argv;
                        }
                    }
                }

                // W^X Bypass for Android 10+
                // Standard Termux pattern: linker [prog_name] [abs_path] [args...]
                // We must ensure abs_path is ALWAYS at argv[1] for the linker to load it.
                
                // 增强：获取规范化路径以处理软链接
                let canonical_cmd = std::fs::canonicalize(&final_cmd).unwrap_or_else(|_| std::path::PathBuf::from(&final_cmd));
                let canonical_str = canonical_cmd.to_string_lossy();
                
                let needs_linker = final_cmd.contains("/com.termux/") || 
                                  final_cmd.starts_with("/data/data/") ||
                                  canonical_str.contains("/com.termux/") ||
                                  canonical_str.starts_with("/data/data/");

                if needs_linker {
                    #[cfg(target_pointer_width = "64")]
                    let linker = "/system/bin/linker64";
                    #[cfg(target_pointer_width = "32")]
                    let linker = "/system/bin/linker";
                    
                    if std::path::Path::new(linker).exists() {
                        let mut linker_argv = Vec::new();
                        
                        // 核心修复：Android Linker 报错 "expected absolute path: sh" 
                        // 说明它看到的 argv[1] 是逻辑名 "sh"。
                        // 我们将 argv[0] 和 argv[1] 全部强制设为绝对路径。
                        linker_argv.push(final_cmd.clone()); // argv[0]
                        linker_argv.push(final_cmd.clone()); // argv[1] (Linker 加载路径，必须绝对)
                        
                        // 透传剩余参数
                        if final_args.len() > 1 {
                            linker_argv.extend(final_args.iter().skip(1).cloned());
                        }
                        
                        final_args = linker_argv;
                        final_cmd = linker.to_string();
                    }
                }

                let mut c_args = Vec::new();
                for arg in &final_args {
                    if let Ok(ca) = CString::new(arg.clone()) { c_args.push(ca); }
                }
                
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
                if !final_cmd.is_empty() {
                    let c_cmd = CString::new(final_cmd.clone()).unwrap();
                    
                    // 调试：打印所有参数以定位 "sh" 的来源
                    for (i, arg) in final_args.iter().enumerate() {
                        crate::utils::android_log(crate::utils::LogPriority::DEBUG, &format!("[PTY_DEBUG] argv[{}] = '{}'", i, arg));
                    }

                    crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY_EXEC] Final Linker Exec: {} -> argv[0]={}, argv[1]={}", 
                        final_cmd, 
                        final_args.get(0).unwrap_or(&"NONE".to_string()),
                        final_args.get(1).unwrap_or(&"NONE".to_string())));
                    
                    libc::execv(c_cmd.as_ptr(), ptr_args.as_ptr());
                    
                    // --- 救命逻辑：如果 Linker 方式也失败，尝试最后的 Fallback ---
                    let err = nix::errno::Errno::last_raw();
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("[PTY_EXEC] execv FAILED! errno: {}. Fallback to /system/bin/sh", err));
                    
                    let fallback_sh = CString::new("/system/bin/sh").unwrap();
                    let sh_name = CString::new("sh").unwrap();
                    let fallback_args = [sh_name.as_ptr(), std::ptr::null()];
                    libc::execv(fallback_sh.as_ptr(), fallback_args.as_ptr());
                }
                libc::_exit(1);
            }
            Err(_) => Err(()),
        }
    }
}

pub fn set_pty_window_size(fd: jint, rows: jint, cols: jint, cell_width: jint, cell_height: jint) {
    if fd < 0 { return; }
    let sz = libc::winsize {
        ws_row: rows as u16,
        ws_col: cols as u16,
        ws_xpixel: (cols * cell_width) as u16,
        ws_ypixel: (rows * cell_height) as u16,
    };
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, &sz);
    }
}

pub fn wait_for(pid: jint) -> jint {
    let mut status: i32 = 0;
    unsafe {
        let res = libc::waitpid(pid, &mut status, 0);
        if res < 0 {
            crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("CHECKPOINT: waitpid failed for PID: {}", pid));
            return -1;
        }

        if libc::WIFEXITED(status) {
            ACTIVE_CHILD_COUNT.fetch_sub(1, Ordering::SeqCst);
            let exit_code = libc::WEXITSTATUS(status);
            crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("CHECKPOINT: Process PID: {} EXITED normally with code: {}", pid, exit_code));
            exit_code
        } else if libc::WIFSIGNALED(status) {
            ACTIVE_CHILD_COUNT.fetch_sub(1, Ordering::SeqCst);
            let sig = libc::WTERMSIG(status);
            crate::utils::android_log(crate::utils::LogPriority::WARN, &format!("CHECKPOINT: Process PID: {} TERMINATED by signal: {} (If 9, likely Phantom Killer)", pid, sig));
            -sig
        } else {
            crate::utils::android_log(crate::utils::LogPriority::DEBUG, &format!("CHECKPOINT: Process PID: {} changed state (other)", pid));
            0
        }
    }
}
