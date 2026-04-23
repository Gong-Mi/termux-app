use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;
use nix::unistd::{ForkResult, chdir, close, fork, setsid};
use std::ffi::{CStr, CString};

// Android 上的 PTY 辅助函数
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
    // 1. 打开 PTM
    use std::os::fd::IntoRawFd;
    let ptm = match open("/dev/ptmx", OFlag::O_RDWR | OFlag::O_CLOEXEC, Mode::empty()) {
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
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                let _ = setsid();

                let c_devname = CString::new(devname).unwrap();
                let pts = libc::open(c_devname.as_ptr(), libc::O_RDWR);
                if pts < 0 { libc::_exit(1); }

                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);

                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);
                if pts > 2 { libc::close(pts); }
                libc::close(ptm);

                // === 环境变量兜底（从 termux-app-rust 迁移） ===
                // Java 层传过来的 envp 可能不完整，Rust 层必须做兜底补充。
                let termux_data = "/data/data/com.termux";
                let termux_files = format!("{}/files", termux_data);
                let termux_prefix = format!("{}/usr", termux_files);
                let termux_bin = format!("{}/bin", termux_prefix);
                let termux_lib = format!("{}/lib", termux_prefix);
                
                let mut final_envp = envp;
                
                // 1. PATH
                if let Some(pos) = final_envp.iter().position(|s| s.starts_with("PATH=")) {
                    let old_path = final_envp[pos].split('=').nth(1).unwrap_or("");
                    if !old_path.contains(&termux_bin) {
                        final_envp[pos] = format!("PATH={}:{}", termux_bin, old_path);
                    }
                } else {
                    final_envp.push(format!("PATH={}:/system/bin:/system/xbin", termux_bin));
                }
                
                // 2. LD_LIBRARY_PATH
                if !final_envp.iter().any(|s| s.starts_with("LD_LIBRARY_PATH=")) {
                    final_envp.push(format!("LD_LIBRARY_PATH={}", termux_lib));
                }
                
                // 3. LD_PRELOAD — 必须使用 linker-ld-preload 变体（含 W^X bypass）
                let termux_exec_candidates = [
                    "libtermux-exec-linker-ld-preload.so",
                    "libtermux-exec.so",
                    "libtermux-exec-ld-preload.so",
                ];
                let mut exec_path = String::new();
                for candidate in &termux_exec_candidates {
                    let path = format!("{}/{}", termux_lib, candidate);
                    if std::path::Path::new(&path).exists() {
                        exec_path = path;
                        break;
                    }
                }
                if !exec_path.is_empty() && !final_envp.iter().any(|s| s.starts_with("LD_PRELOAD=")) {
                    final_envp.push(format!("LD_PRELOAD={}", exec_path));
                }
                
                // 4. 基础变量兜底
                if !final_envp.iter().any(|s| s.starts_with("TERM=")) {
                    final_envp.push("TERM=xterm-256color".to_string());
                }
                if !final_envp.iter().any(|s| s.starts_with("HOME=")) {
                    final_envp.push(format!("HOME={}/home", termux_files));
                }
                if !final_envp.iter().any(|s| s.starts_with("PREFIX=")) {
                    final_envp.push(format!("PREFIX={}", termux_prefix));
                }
                
                libc::clearenv();
                for env_var in final_envp {
                    if let Ok(c_env) = CString::new(env_var) {
                        libc::putenv(c_env.into_raw());
                    }
                }
                
                if !cwd_str.is_empty() {
                    let c_cwd = CString::new(cwd_str).unwrap();
                    let _ = chdir(c_cwd.as_c_str());
                }
                
                // === 命令解析与 W^X Bypass（从 termux-app-rust 迁移） ===
                let mut final_cmd = cmd_str.clone();
                let mut final_args = argv.clone();
                
                // 相对路径解析：如果 cmd 不是绝对路径，在 $PREFIX/bin 下查找
                if !final_cmd.starts_with('/') {
                    let resolved = format!("{}/{}", termux_bin, final_cmd);
                    if std::path::Path::new(&resolved).exists() {
                        final_cmd = resolved;
                    }
                }
                
                // Login shell 处理：确保 argv[0] 以 '-' 开头（如 -bash）
                let is_shell = ["sh", "bash", "zsh", "dash", "fish"].iter().any(|&s| final_cmd.ends_with(s));
                if is_shell {
                    let shell_name = std::path::Path::new(&final_cmd)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("sh");
                    if final_args.is_empty() {
                        final_args.push(format!("-{}", shell_name));
                    } else if !final_args[0].starts_with('-') {
                        final_args[0] = format!("-{}", shell_name);
                    }
                }
                
                // Shebang 解析：读取脚本 shebang 行，找到解释器
                let mut is_elf = false;
                let mut has_shebang = false;
                let mut shebang_interpreter = String::new();
                let mut shebang_args: Vec<String> = Vec::new();
                
                if let Ok(mut f) = std::fs::File::open(&final_cmd) {
                    use std::io::Read;
                    let mut buf = [0u8; 256];
                    if let Ok(bytes_read) = f.read(&mut buf) {
                        if bytes_read > 4 && buf[0] == 0x7F && buf[1] == b'E' && buf[2] == b'L' && buf[3] == b'F' {
                            is_elf = true;
                        } else if bytes_read > 2 && buf[0] == b'#' && buf[1] == b'!' {
                            has_shebang = true;
                            if let Ok(shebang) = std::str::from_utf8(&buf[2..bytes_read]) {
                                let interpreter_line = shebang.lines().next().unwrap_or("").trim();
                                if !interpreter_line.is_empty() {
                                    let parts: Vec<&str> = interpreter_line.split_whitespace().collect();
                                    if !parts.is_empty() {
                                        shebang_interpreter = parts[0].to_string();
                                        for p in parts.iter().skip(1) {
                                            shebang_args.push(p.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                if has_shebang && !shebang_interpreter.is_empty() {
                    let mut interpreter = shebang_interpreter.clone();
                    // 路径转换：/usr/bin/xxx /bin/xxx -> $PREFIX/bin/xxx
                    if interpreter.starts_with("/usr/bin/") || interpreter.starts_with("/bin/") {
                        let binary_name = std::path::Path::new(&interpreter)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        interpreter = format!("{}/bin/{}", termux_prefix, binary_name);
                    }
                    
                    let old_cmd = final_cmd.clone();
                    final_cmd = interpreter;
                    
                    // 构造新的 argv: [prog_name, interpreter_args..., script_path, original_args...]
                    let mut new_argv = Vec::new();
                    new_argv.push(if !final_args.is_empty() && (final_args[0].starts_with('-') || final_args[0].starts_with('/')) {
                        final_args[0].clone()
                    } else {
                        final_cmd.clone()
                    });
                    for a in &shebang_args { new_argv.push(a.clone()); }
                    new_argv.push(old_cmd);
                    if argv.len() > 1 {
                        new_argv.extend(argv.iter().skip(1).cloned());
                    }
                    final_args = new_argv;
                } else if !is_elf && !has_shebang {
                    // 既不是 ELF 也没有 shebang，默认用 Termux sh 执行
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
                
                // W^X Bypass: 决定是否使用 system linker
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
                        let prog_name = if !final_args.is_empty() {
                            final_args[0].clone()
                        } else {
                            final_cmd.clone()
                        };
                        linker_argv.push(prog_name);
                        linker_argv.push(final_cmd.clone());
                        if final_args.len() > 1 {
                            linker_argv.extend(final_args.iter().skip(1).cloned());
                        }
                        final_args = linker_argv;
                        final_cmd = linker.to_string();
                    }
                }
                
                // 构建 C 字符串参数列表并 exec
                let mut c_args = Vec::new();
                for arg in &final_args {
                    if let Ok(ca) = CString::new(arg.clone()) { c_args.push(ca); }
                }
                
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
                
                if !final_cmd.is_empty() {
                    let c_cmd = CString::new(final_cmd.clone()).unwrap();
                    libc::execv(c_cmd.as_ptr(), ptr_args.as_ptr());
                    
                    // execv 失败，记录关键错误信息
                    let err = nix::errno::Errno::last_raw();
                    let err_name = match err {
                        1 => "EPERM", 2 => "ENOENT", 13 => "EACCES", 8 => "ENOEXEC", 14 => "EFAULT", _ => "UNKNOWN",
                    };
                    crate::utils::android_log(
                        crate::utils::LogPriority::ERROR,
                        &format!("[PTY] execv FAILED! errno={} ({}) cmd='{}' argv={:?}", err, err_name, final_cmd, final_args)
                    );
                    
                    // W^X 典型错误回退到系统 shell
                    if err == 13 || err == 8 {
                        crate::utils::android_log(
                            crate::utils::LogPriority::WARN,
                            "[PTY] Fallback to /system/bin/sh"
                        );
                        let fallback_sh = CString::new("/system/bin/sh").unwrap();
                        let sh_name = CString::new("sh").unwrap();
                        let fallback_args = [sh_name.as_ptr(), std::ptr::null()];
                        libc::execv(fallback_sh.as_ptr(), fallback_args.as_ptr());
                    }
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
        libc::waitpid(pid, &mut status, 0);
        if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else if libc::WIFSIGNALED(status) {
            -libc::WTERMSIG(status)
        } else {
            0
        }
    }
}
