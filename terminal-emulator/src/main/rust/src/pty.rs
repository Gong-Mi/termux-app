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
                
                // 降低子进程优先级 (Nice 19)，减少对系统负载的冲击
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

                // === 动态真实身份检测与路径重排 ===
                let mut real_pkg = String::from("com.termux");
                if let Ok(cmdline) = std::fs::read_to_string("/proc/self/cmdline") {
                    if let Some(pkg) = cmdline.split('\0').next() {
                        if !pkg.is_empty() && pkg.contains('.') {
                            real_pkg = pkg.to_string();
                        }
                    }
                }
                
                let real_data_root = format!("/data/data/{}", real_pkg);
                let termux_prefix = format!("{}/files/usr", real_data_root);
                let termux_bin = format!("{}/bin", termux_prefix);
                let termux_lib = format!("{}/lib", termux_prefix);
                let termux_home = format!("{}/files/home", real_data_root);
                let termux_tmp = format!("{}/tmp", termux_prefix);

                // 辅助函数：将任何路径中的 com.termux 修正为当前真实包名
                let fix_path = |s: &str| -> String {
                    s.replace("/data/data/com.termux", &real_data_root)
                     .replace("/data/user/0/com.termux", &real_data_root)
                };

                // 1. 准备注入的环境变量矩阵 (完全接管 Java 侧职责)
                let mut vars_to_set = Vec::new();
                
                // 核心路径
                let old_path = std::env::var("PATH").unwrap_or_else(|_| "/system/bin:/system/xbin".to_string());
                let fixed_old_path = fix_path(&old_path);
                vars_to_set.push(("PATH", format!("{}:{}", termux_bin, fixed_old_path)));
                
                vars_to_set.push(("PREFIX", termux_prefix.clone()));
                vars_to_set.push(("HOME", termux_home));
                vars_to_set.push(("TMPDIR", termux_tmp));
                
                // 运行库环境
                vars_to_set.push(("LD_LIBRARY_PATH", termux_lib.clone()));
                
                // 终端特性
                vars_to_set.push(("TERM", "xterm-256color".to_string()));
                vars_to_set.push(("COLORTERM", "truecolor".to_string()));
                vars_to_set.push(("LANG", "en_US.UTF-8".to_string()));
                
                // LD_PRELOAD (核心：SDK 36 W^X Bypass)
                let termux_exec_candidates = [
                    "libtermux-exec-direct-ld-preload.so",
                    "libtermux-exec-linker-ld-preload.so",
                    "libtermux-exec.so",
                    "libtermux-exec-ld-preload.so",
                ];
                for candidate in &termux_exec_candidates {
                    let path = format!("{}/{}", termux_lib, candidate);
                    if std::path::Path::new(&path).exists() {
                        vars_to_set.push(("LD_PRELOAD", path));
                        break;
                    }
                }
                
                // 批量设置（使用 setenv，确保 Android 系统必需变量不丢失）
                for (key, value) in vars_to_set {
                    if let (Ok(ck), Ok(cv)) = (CString::new(key), CString::new(value)) {
                        libc::setenv(ck.as_ptr(), cv.as_ptr(), 1);
                    }
                }
                
                for env_var in envp {
                    if let Some(pos) = env_var.find('=') {
                        let key = &env_var[..pos];
                        let value = fix_path(&env_var[pos+1..]);
                        if let (Ok(ck), Ok(cv)) = (CString::new(key), CString::new(value)) {
                            libc::setenv(ck.as_ptr(), cv.as_ptr(), 1);
                        }
                    }
                }

                if !cwd_str.is_empty() {
                    let fixed_cwd = fix_path(&cwd_str);
                    let c_cwd = CString::new(fixed_cwd).unwrap();
                    let _ = chdir(c_cwd.as_c_str());
                }
                
                // === 命令解析与路径修正 ===
                let mut final_cmd = fix_path(&cmd_str);
                let mut final_args: Vec<String> = argv.iter().map(|a| fix_path(a)).collect();
                
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
                    if final_args.len() > 1 {
                        new_argv.extend(final_args.iter().skip(1).cloned());
                    }
                    final_args = new_argv;
                } else if !is_elf && !has_shebang {
                    // 既不是 ELF 也没有 shebang，默认用 Termux sh 执行
                    let old_cmd = final_cmd.clone();
                    final_cmd = format!("{}/bin/sh", termux_prefix);
                    let mut new_argv = Vec::new();
                    new_argv.push(final_cmd.clone());
                    new_argv.push(old_cmd);
                    if final_args.len() > 1 {
                        new_argv.extend(final_args.iter().skip(1).cloned());
                    }
                    final_args = new_argv;
                }
                
                // W^X Bypass: 决定是否使用 system linker
                // 在 SDK 31+ (Android 12+) 以后，必须通过 linker 启动数据目录下的二进制
                let canonical_cmd = std::fs::canonicalize(&final_cmd).unwrap_or_else(|_| std::path::PathBuf::from(&final_cmd));
                let canonical_str = canonical_cmd.to_string_lossy();
                
                // 只要路径在 /data/data 下，或者包含当前包名，就必须强制走 linker
                let needs_linker = final_cmd.contains("/data/data/") || 
                                  final_cmd.contains("/data/user/0/") ||
                                  final_cmd.contains(&real_pkg) ||
                                  canonical_str.contains("/data/data/") ||
                                  canonical_str.contains(&real_pkg);
                
                if needs_linker {
                    #[cfg(target_pointer_width = "64")]
                    let linker = "/system/bin/linker64";
                    #[cfg(target_pointer_width = "32")]
                    let linker = "/system/bin/linker";
                    
                    if std::path::Path::new(linker).exists() {
                        let mut linker_argv = Vec::new();
                        
                        // 关键：保留原始 argv[0]（如 -bash），linker 把它传给子进程作为 progname
                        let prog_name = if !final_args.is_empty() {
                            final_args[0].clone()
                        } else {
                            final_cmd.clone()
                        };
                        
                        linker_argv.push(prog_name);        // argv[0] - 程序名 (Internal to the child)
                        linker_argv.push(final_cmd.clone()); // argv[1] - linker 必须加载的绝对路径
                        
                        // 透传剩余参数（跳过原来的 argv[0]，因为我们已经用它作为 prog_name）
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
                
                crate::utils::android_log(
                    crate::utils::LogPriority::INFO,
                    &format!("[PTY] final_exec cmd='{}' argv={:?}", final_cmd, final_args)
                );

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
