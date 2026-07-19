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
                ACTIVE_CHILD_COUNT.fetch_add(1, Ordering::SeqCst);
                
                // 返回原始 PTM 给 Rust IO 线程管理。
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                let c_devname = CString::new(devname).unwrap();
                let pts = libc::open(c_devname.as_ptr(), libc::O_RDWR);
                if pts < 0 { libc::_exit(1); }

                // === 深度修复：彻底解除 fdsan 保护 ===
                // 我们直接在子进程中清除 FD 0, 1, 2 的所有权标签，防止触发父进程的 fdsan 检查。
                unsafe {
                    unsafe extern "C" {
                        fn android_fdsan_set_error_level(new_level: i32) -> i32;
                        fn android_fdsan_exchange_owner_tag(fd: i32, expected_tag: u64, new_tag: u64);
                    }
                    // 1. 彻底禁用当前进程的 fdsan 报错
                    android_fdsan_set_error_level(0);
                    // 2. 强行重置标准流的 tag (0 = FDSAN_OWNER_TAG_NONE)
                    android_fdsan_exchange_owner_tag(0, u64::MAX, 0); 
                    android_fdsan_exchange_owner_tag(1, u64::MAX, 0);
                    android_fdsan_exchange_owner_tag(2, u64::MAX, 0);
                    
                    // 3. 关闭所有继承自 JVM 的多余 FD (非常重要)
                    for i in 3..1024 {
                        if i != pts && i != ptm {
                            libc::close(i);
                        }
                    }
                }

                let _ = setsid();

                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);

                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);
                if pts > 2 { libc::close(pts); }
                libc::close(ptm);

                // === CHECKPOINT 系统：记录 exec 解析链条，用于 W^X 错误分析 ===
                let mut termux_data = "/data/data/com.termux".to_string();
                
                // 动态检测实际路径（适配多用户/工作资料）
                if cmd_str.contains("/data/user/") {
                    if let Some(pos) = cmd_str.find("/com.termux") {
                        termux_data = cmd_str[..pos + 11].to_string();
                    }
                } else if cwd_str.contains("/data/user/") {
                    if let Some(pos) = cwd_str.find("/com.termux") {
                        termux_data = cwd_str[..pos + 11].to_string();
                    }
                }

                let termux_files = format!("{}/files", termux_data);
                let termux_prefix = format!("{}/usr", termux_files);
                
                // === 详细环境追踪 ===
                crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY_TRACE] Child Process (PID={}) starting...", libc::getpid()));
                crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY_TRACE] termux_data='{}' termux_prefix='{}'", termux_data, termux_prefix));

                // === 顶级环境清洗：彻底扫除 /data/data/ 幽灵 ===
                let mut final_envp: Vec<String> = envp.iter().map(|s| {
                    if s.contains("/data/data/com.termux") {
                        s.replace("/data/data/com.termux", &termux_data)
                    } else {
                        s.clone()
                    }
                }).collect();

                // 1. 强制纠正核心变量
                let termux_bin = format!("{}/bin", termux_prefix);

                // PATH 清洗与注入
                if let Some(pos) = final_envp.iter().position(|s| s.starts_with("PATH=")) {
                    let old_path = final_envp[pos].splitn(2, '=').nth(1).unwrap_or("");
                    // 彻底移除旧 PATH 中所有包含 /data/data/ 的条目
                    let clean_path: Vec<&str> = old_path.split(':')
                        .filter(|p| !p.contains("/data/data/com.termux"))
                        .collect();
                    let mut new_path_str = termux_bin.clone();
                    if !clean_path.is_empty() {
                        new_path_str.push(':');
                        new_path_str.push_str(&clean_path.join(":"));
                    }
                    final_envp[pos] = format!("PATH={}", new_path_str);
                } else {
                    final_envp.push(format!("PATH={}:/system/bin:/system/xbin", termux_bin));
                }

                // Do not force LD_LIBRARY_PATH for Termux child processes.
                // Modern Termux binaries use DT_RUNPATH; injecting $PREFIX/lib
                // globally can override Android/Termux library resolution and
                // break APT methods, curl, Python, and other subprocesses.

                // Keep exec interception, but only advertise it when the
                // library is actually present. Preserve an explicitly supplied
                // LD_PRELOAD rather than overwriting it.
                let exec_path_string = format!("{}/lib/libtermux-exec.so", termux_prefix);
                let exec_path = std::path::Path::new(&exec_path_string);
                let termux_exec_path = exec_path
                    .canonicalize()
                    .unwrap_or_else(|_| exec_path.to_path_buf())
                    .to_string_lossy()
                    .into_owned();
                let has_nonempty_ld_preload = final_envp.iter().any(|s| {
                    s.strip_prefix("LD_PRELOAD=")
                        .map(|value| !value.is_empty())
                        .unwrap_or(false)
                });
                if !has_nonempty_ld_preload {
                    if let Some(pos) = final_envp.iter().position(|s| s.starts_with("LD_PRELOAD=")) {
                        final_envp[pos] = format!("LD_PRELOAD={}", termux_exec_path);
                    } else {
                        final_envp.push(format!("LD_PRELOAD={}", termux_exec_path));
                    }
                }

                crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY_TRACE] Final ENV count: {}", final_envp.len()));
                for e in &final_envp {
                    if e.starts_with("PATH=") || e.starts_with("LD_") || e.starts_with("HOME=") {
                        crate::utils::android_log(crate::utils::LogPriority::DEBUG, &format!("[PTY_ENV] {}", e));
                    }
                }

                libc::clearenv();
                for env_var in &final_envp {
                    if let Ok(c_env) = CString::new(env_var.clone()) {
                        libc::putenv(c_env.into_raw());
                    }
                }

                if !cwd_str.is_empty() {
                    let c_cwd = CString::new(cwd_str.clone()).unwrap();
                    if libc::chdir(c_cwd.as_ptr()) != 0 {
                        crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("[PTY_TRACE] chdir FAILED for '{}'", cwd_str));
                    }
                }

                // ====== 开始 exec 解析链条检查点 ======
                crate::utils::android_log(
                    crate::utils::LogPriority::INFO,
                    &format!("[PTY_CHECKPOINT] CP01: input cmd='{}' argv={:?}", cmd_str, argv)
                );

                let mut final_cmd = cmd_str.clone();
                if !final_cmd.starts_with('/') {
                    let resolved = format!("{}/{}", termux_bin, final_cmd);
                    if std::path::Path::new(&resolved).exists() {
                        final_cmd = resolved;
                    }
                }
                crate::utils::android_log(
                    crate::utils::LogPriority::INFO,
                    &format!("[PTY_CHECKPOINT] CP02: resolved absolute path='{}'", final_cmd)
                );
                
                let mut final_args = argv.clone();
                if !final_args.is_empty() && !final_args[0].starts_with('/') && !final_args[0].starts_with('-') {
                    let resolved = format!("{}/{}", termux_bin, final_args[0]);
                    if std::path::Path::new(&resolved).exists() {
                        final_args[0] = resolved;
                    }
                }
                
                // Login Shell 处理
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

                // Parse ELF / Shebang
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

                crate::utils::android_log(
                    crate::utils::LogPriority::INFO,
                    &format!(
                        "[PTY_CHECKPOINT] CP03: file header  is_elf={} has_shebang={}",
                        is_elf, has_shebang
                    )
                );

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

                    crate::utils::android_log(
                        crate::utils::LogPriority::INFO,
                        &format!(
                            "[PTY_CHECKPOINT] CP04: shebang interpreter='{}' args={:?}",
                            shebang_interpreter, shebang_args
                        )
                    );
                    crate::utils::android_log(
                        crate::utils::LogPriority::INFO,
                        &format!(
                            "[PTY_CHECKPOINT] CP05: path converted interpreter='{}' script='{}'",
                            final_cmd, old_cmd
                        )
                    );

                    // 构造新的 argv: [prog_name, interpreter_args..., script_path, original_script_args...]
                    let mut new_argv = Vec::new();
                    // 保留原始程序名（如 -bash）或 interpreter 路径
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
                    new_argv.push(old_cmd.clone());
                    if argv.len() > 1 {
                        new_argv.extend(argv.iter().skip(1).cloned());
                    }
                    final_args = new_argv;
                    crate::utils::android_log(
                        crate::utils::LogPriority::INFO,
                        &format!("[PTY_CHECKPOINT] CP04: no ELF/shebang, fallback to sh script='{}'", old_cmd)
                    );
                }

                // W^X Bypass: 决定是否使用 system linker
                let canonical_cmd = std::fs::canonicalize(&final_cmd).unwrap_or_else(|_| std::path::PathBuf::from(&final_cmd));
                let canonical_str = canonical_cmd.to_string_lossy();
                
                let needs_linker = final_cmd.contains("/com.termux/") || 
                                  final_cmd.starts_with("/data/data/") ||
                                  canonical_str.contains("/com.termux/") ||
                                  canonical_str.starts_with("/data/data/");

                crate::utils::android_log(
                    crate::utils::LogPriority::INFO,
                    &format!(
                        "[PTY_CHECKPOINT] CP06: linker_needed={} cmd='{}' canonical='{}'",
                        needs_linker, final_cmd, canonical_str
                    )
                );

                if needs_linker {
                    #[cfg(target_pointer_width = "64")]
                    let linker = "/system/bin/linker64";
                    #[cfg(target_pointer_width = "32")]
                    let linker = "/system/bin/linker";
                    
                    if std::path::Path::new(linker).exists() {
                        let mut linker_argv = Vec::new();
                        
                        // 关键修复：确保 final_cmd 路径也是最新的转换后路径
                        let mut corrected_cmd = final_cmd.clone();
                        if corrected_cmd.contains("/data/data/com.termux") {
                            corrected_cmd = corrected_cmd.replace("/data/data/com.termux", &termux_data);
                        }

                        // 关键：保留原始 argv[0]（如 -bash），linker 把它传给子进程作为 progname
                        let prog_name = if !final_args.is_empty() {
                            final_args[0].clone()
                        } else {
                            corrected_cmd.clone()
                        };
                        linker_argv.push(prog_name);        // argv[0] - 程序名
                        linker_argv.push(corrected_cmd.clone()); // argv[1] - linker 必须加载的绝对路径
                        
                        // 透传剩余参数（跳过原来的 argv[0]，因为我们已经用它作为 prog_name）
                        if final_args.len() > 1 {
                            for arg in final_args.iter().skip(1) {
                                let mut corrected_arg = arg.clone();
                                if corrected_arg.contains("/data/data/com.termux") {
                                    corrected_arg = corrected_arg.replace("/data/data/com.termux", &termux_data);
                                }
                                linker_argv.push(corrected_arg);
                            }
                        }
                        
                        final_args = linker_argv;
                        final_cmd = linker.to_string();
                    }
                }

                // 构建 C 字符串参数列表
                let mut c_args = Vec::new();
                for arg in &final_args {
                    if let Ok(ca) = CString::new(arg.clone()) { c_args.push(ca); }
                }
                
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
                
                crate::utils::android_log(
                    crate::utils::LogPriority::INFO,
                    &format!(
                        "[PTY_CHECKPOINT] CP07: final_exec cmd='{}' argv={:?}",
                        final_cmd, final_args
                    )
                );

                if !final_cmd.is_empty() {
                    // 额外检查：文件是否真的存在且可执行
                    let check_path = std::path::Path::new(&final_cmd);
                    crate::utils::android_log(
                        crate::utils::LogPriority::INFO,
                        &format!(
                            "[PTY_TRACE] Execution Pre-check: exists={}, is_file={}, metadata={:?}",
                            check_path.exists(),
                            check_path.is_file(),
                            check_path.metadata().ok()
                        )
                    );

                    let c_cmd = CString::new(final_cmd.clone()).unwrap();
                    libc::execv(c_cmd.as_ptr(), ptr_args.as_ptr());
                    
                    // execv 失败，记录关键错误信息
                    let err = nix::errno::Errno::last_raw();
                    let err_name = match err {
                        1 => "EPERM",
                        2 => "ENOENT",
                        13 => "EACCES",
                        8 => "ENOEXEC",
                        14 => "EFAULT",
                        _ => "UNKNOWN",
                    };
                    crate::utils::android_log(
                        crate::utils::LogPriority::ERROR,
                        &format!(
                            "[PTY_CHECKPOINT] CP08: execv FAILED! errno={} ({}) cmd='{}' argv={:?}",
                            err, err_name, final_cmd, final_args
                        )
                    );
                    
                    // W^X 典型错误：EACCES(13) 或 ENOEXEC(8)
                    if err == 13 || err == 8 {
                        crate::utils::android_log(
                            crate::utils::LogPriority::ERROR,
                            "[PTY_CHECKPOINT] CP08b: W^X EXECUTION DENIED - The binary is in app data directory but linker bypass failed or was skipped. Check CP06 linker_needed decision."
                        );
                    }
                    
                    // Fallback 到系统 shell
                    crate::utils::android_log(
                        crate::utils::LogPriority::WARN,
                        "[PTY_CHECKPOINT] CP09: Fallback to /system/bin/sh"
                    );
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
