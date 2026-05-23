use jni::JNIEnv;
use jni::objects::{JIntArray, JObjectArray, JString};
use jni::sys::{jint, jintArray, jobjectArray, jstring};
use nix::unistd::{ForkResult, chdir, fork, setsid};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi::CString;
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::utils::{LogPriority, android_log};

#[cfg(not(feature = "test-helpers"))]
type WatcherCallback = jni::objects::GlobalRef;
#[cfg(feature = "test-helpers")]
type WatcherCallback = std::sync::Arc<dyn Fn(i32) + Send + Sync>;

static PTY_ALLOC_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// 进程创建信号量：限制瞬时并发 fork 数量为 4。
/// 即使上层请求 40 个进程，底层也会排队执行 fork，避免内核资源瞬间被掏空。
static SPAWN_SEMAPHORE: Lazy<std::sync::Arc<std::sync::Condvar>> =
    Lazy::new(|| std::sync::Arc::new(std::sync::Condvar::new()));
static SPAWN_COUNTER: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(0));
const MAX_CONCURRENT_SPAWN: usize = 4;

pub unsafe fn create_subprocess(
    env: &mut JNIEnv,
    cmd: jstring,
    cwd: jstring,
    args: jobjectArray,
    _env_vars: jobjectArray,
    process_id_array: jintArray,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
    is_failsafe: bool,
) -> jint {
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

    // 环境变量由 Rust 层完全自主构建，不再解析 Java 层传递的 env_vars。
    // _env_vars 参数保留以保持 JNI 签名兼容，但内容被忽略。
    match create_subprocess_with_data(cmd_str, cwd_str, argv, rows, cols, cw, ch, is_failsafe) {
        Ok((fd, pid)) => {
            let pid_val = [pid as jint];
            let j_pid_array = unsafe { JIntArray::from_raw(process_id_array) };
            let _ = env.set_int_array_region(&j_pid_array, 0, &pid_val);
            fd
        }
        Err(_) => -1,
    }
}

/// Parse a shebang line into (interpreter_path, optional_args).
/// Handles spaces in the shebang, e.g. "#!/usr/bin/env bash" -> ("/usr/bin/env", Some("bash"))
pub fn parse_shebang(buffer: &[u8]) -> Option<(String, Option<String>)> {
    if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' {
        return None;
    }
    // Shebang line ends at first newline (or end of buffer)
    let line_end = buffer
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(buffer.len());
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

/// Map an interpreter path to the Termux prefix, matching upstream logic.
pub fn map_interpreter(interp: &str, normalize: &dyn Fn(String) -> String) -> String {
    let termux_prefix = crate::get_termux_prefix();
    if interp.starts_with("/usr/bin/env") {
        format!("{}/bin/env", termux_prefix)
    } else if interp.starts_with("/bin/") || interp.starts_with("/usr/bin/") {
        let binary = interp.rsplit('/').next().unwrap_or("sh");
        format!("{}/bin/{}", termux_prefix, binary)
    } else if interp.starts_with("/data/data/com.termux/")
        || interp.starts_with("/data/user/0/com.termux/")
    {
        normalize(interp.to_string())
    } else {
        interp.to_string()
    }
}

pub fn create_subprocess_with_data(
    cmd_str: String,
    cwd_str: String,
    argv: Vec<String>,
    rows: jint,
    cols: jint,
    cw: jint,
    ch: jint,
    is_failsafe: bool,
) -> Result<(jint, i32), ()> {
    android_log(
        LogPriority::DEBUG,
        &format!(
            "[PTY] create_subprocess_with_data: cmd={}, cwd={}",
            cmd_str, cwd_str
        ),
    );
    // ------------------------------------------------------------------
    // 并发限制机制 (Semaphore/Throttling)
    // ------------------------------------------------------------------
    {
        let mut count = SPAWN_COUNTER.lock().unwrap();
        while *count >= MAX_CONCURRENT_SPAWN {
            count = SPAWN_SEMAPHORE.wait(count).unwrap();
        }
        *count += 1;
    }

    // 确保函数退出时释放信号量
    struct SpawnGuard;
    impl Drop for SpawnGuard {
        fn drop(&mut self) {
            let mut count = SPAWN_COUNTER.lock().unwrap();
            *count -= 1;
            SPAWN_SEMAPHORE.notify_one();
        }
    }
    let _guard = SpawnGuard;

    let termux_prefix = crate::get_termux_prefix();
    let termux_files_dir = if let Some(parent) = std::path::Path::new(&termux_prefix).parent() {
        parent.to_string_lossy().to_string()
    } else {
        "/data/data/com.termux/files".to_string()
    };
    let termux_data_dir = if let Some(parent) = std::path::Path::new(&termux_files_dir).parent() {
        parent.to_string_lossy().to_string()
    } else {
        "/data/data/com.termux".to_string()
    };

    let normalize_path = |path: String| -> String {
        // 动态适配：如果路径包含 /data/user/0/com.termux 或类似的硬编码 Android 路径，
        // 且它不匹配当前实际的 termux_data_dir，则进行全量替换。
        if (path.contains("/data/user/0/com.termux") || path.contains("/data/data/com.termux"))
            && !path.contains(&termux_data_dir)
        {
            // 将所有已知的硬编码前缀替换为动态探测的前缀
            path.replace("/data/user/0/com.termux", &termux_data_dir)
                .replace("/data/data/com.termux", &termux_data_dir)
        } else {
            path
        }
    };

    let cmd_str = normalize_path(cmd_str);
    let cwd_str = normalize_path(cwd_str);
    let argv: Vec<String> = argv.into_iter().map(normalize_path).collect();

    // ------------------------------------------------------------------
    // 环境变量由 Rust 层完全自主构建，不再接收/修补 Java 层传递的 envp。
    // 这消除了 Java ↔ Native 之间的“中间状态”不一致。
    // ------------------------------------------------------------------
    let env_list = crate::env_builder::build_termux_environment(&cwd_str, is_failsafe);

    // 二次清洗环境变量：确保即使 env_builder 漏掉某些系统变量，这里也会进行路径归一化
    let c_envs: Vec<CString> = env_list
        .into_iter()
        .map(|cs| {
            let s = cs.to_string_lossy().to_string();
            let normalized = s
                .replace("/data/user/0/com.termux", &termux_data_dir)
                .replace("/data/data/com.termux", &termux_data_dir);
            CString::new(normalized).unwrap_or_else(|_| cs)
        })
        .collect();

    let mut real_cmd = cmd_str.clone();
    let mut real_argv = argv.clone();

    // Default shell selection logic moved to Rust to reduce Java code
    // Prefer ELF binaries over shebang scripts to avoid linker wrapper issues
    if real_cmd.is_empty() {
        // Prefer ELF binaries over shebang scripts for faster startup
        // and to reduce linker-wrapper surface area.
        let default_shells = [
            format!("{}/bin/bash", termux_prefix),
            format!("{}/bin/dash", termux_prefix),
            format!("{}/bin/sh", termux_prefix),
            "/system/bin/sh".to_string(),
            format!("{}/bin/login", termux_prefix),
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
                android_log(
                    LogPriority::INFO,
                    &format!(
                        "[TRACE_SESSION] No command provided, selected default shell: {}",
                        real_cmd
                    ),
                );
                break;
            }
        }
    }

    android_log(
        LogPriority::INFO,
        &format!(
            "[TRACE_SESSION] Preparing to exec: {} with argv: {:?}",
            real_cmd, real_argv
        ),
    );

    let _cmd_log = real_cmd.clone();

    // Read the first 256 bytes of the target file to determine ELF / shebang / plain script.
    let (final_cmd, final_argv) = if let Ok(mut file) = std::fs::File::open(&real_cmd) {
        use std::io::Read;
        let mut buffer = [0u8; 4096];
        if let Ok(n) = file.read(&mut buffer) {
            if n > 4
                && buffer[0] == 0x7F
                && buffer[1] == b'E'
                && buffer[2] == b'L'
                && buffer[3] == b'F'
            {
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

                android_log(
                    LogPriority::INFO,
                    &format!(
                        "[PTY] Shebang detected: interpreter={}, args={:?}, script={}, new_argv={:?}",
                        interpreter, shebang_args, real_cmd, new_argv
                    ),
                );
                (interpreter, new_argv)
            } else {
                // No shebang and no ELF - default to $PREFIX/bin/sh.
                let interpreter = format!("{}/bin/sh", termux_prefix);
                let mut new_argv = Vec::new();
                if !real_argv.is_empty() {
                    new_argv.push(real_argv[0].clone()); // process name
                }
                new_argv.push(real_cmd.clone()); // script path
                if real_argv.len() > 1 {
                    new_argv.extend(real_argv[1..].iter().cloned());
                }
                android_log(
                    LogPriority::INFO,
                    &format!("[PTY] No shebang/ELF, defaulting to shell: {}", interpreter),
                );
                (interpreter, new_argv)
            }
        } else {
            (real_cmd, real_argv)
        }
    } else {
        (real_cmd, real_argv)
    };

    // ------------------------------------------------------------------
    // Android 10+ (API 29+) W^X / exec() 限制绕过：linker64 间接执行
    // ------------------------------------------------------------------
    // 当 targetSdk >= 29 时，SELinux neverallow 禁止直接执行 app_data_file。
    // 但对于 $PREFIX 下的 ELF，我们可以通过系统链接器间接加载：
    //   /system/bin/linker64 <argv0> <target_elf> [args...]
    // linker64 作为系统域程序不受此限制，它加载目标 ELF 到内存后移交控制权。
    //
    // 注意：此 wrapper 只保护通过 PTY 直接启动的程序。
    // 子进程自己再 exec 的程序（如 Go 静态链接二进制内部的 exec）
    // 需要 LD_PRELOAD + libtermux-exec.so 来覆盖。
    // ------------------------------------------------------------------

    /// 判断文件是否为 ELF（跟随符号链接）
    fn is_elf_file(path: &str) -> bool {
        std::fs::File::open(path)
            .and_then(|mut f| {
                let mut buf = [0u8; 4];
                f.read_exact(&mut buf)?;
                Ok(buf[0] == 0x7F && buf[1] == b'E' && buf[2] == b'L' && buf[3] == b'F')
            })
            .unwrap_or(false)
    }

    /// 判断目标路径是否需要 linker64 wrapper
    fn needs_linker_wrapper(path: &str, _termux_files_dir: &str) -> bool {
        // 允许各种 Termux 路径变体（/data/data, /data/user/N, /apex/等）
        if !path.contains("/com.termux/files/") {
            return false;
        }
        // 符号链接需要解析后判断
        let real_path = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string());
        is_elf_file(&real_path)
    }

    let linker_path = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    // 对 final_cmd（已解析 shebang 后的真实可执行文件，如 /data/data/.../sh）
    // 检查是否需要 linker64 wrapper。
    let use_linker_wrapper = needs_linker_wrapper(&final_cmd, &termux_files_dir);

    let (exec_cmd, exec_argv) = if use_linker_wrapper {
        // Android linker64 的 argv 约定：
        //   linker64 <target_elf_path> [args...]
        // 目标 ELF 的 argv[0] 会被 linker 自动设为 target_elf_path，
        // 因此我们需要把期望的 process name 放在更前面的位置，
        // 但 linker64 的标准行为是 argv[0]=linker64, argv[1]=target_elf, argv[2..]=args
        // 目标程序看到的 argv[0] 实际上是 target_elf_path。
        //
        // 为了保持与 upstream Java 的 TermuxShellUtils 行为一致：
        //   [process_name, target_elf, original_args...]
        // 其中 target_elf 会被 linker 当作要加载的 ELF，original_args 会透传。
        let mut wrapped_argv = vec![linker_path.to_string()];
        wrapped_argv.push(final_cmd.clone()); // target ELF path for linker64
        if !final_argv.is_empty() {
            wrapped_argv.extend(final_argv[1..].iter().cloned());
        }
        android_log(
            LogPriority::INFO,
            &format!(
                "[PTY] W^X Bypass: execvp({}, {:?})",
                linker_path, wrapped_argv
            ),
        );
        (linker_path.to_string(), wrapped_argv)
    } else {
        (final_cmd, final_argv)
    };

    let c_exec_cmd = CString::new(exec_cmd).unwrap();
    let c_exec_args: Vec<CString> = exec_argv
        .iter()
        .map(|a| CString::new(a.clone()).unwrap())
        .collect();

    let (ptm, c_pts) = {
        let _guard = PTY_ALLOC_LOCK.lock().unwrap();
        unsafe {
            let ptm = libc::open(
                "/dev/ptmx\0".as_ptr() as *const _,
                libc::O_RDWR | libc::O_CLOEXEC,
            );
            if ptm < 0 {
                return Err(());
            }

            if libc::grantpt(ptm) != 0 || libc::unlockpt(ptm) != 0 {
                libc::close(ptm);
                return Err(());
            }

            // 使用线程安全的 ptsname_r 代替 ptsname
            let mut buf = [0; 64];
            if libc::ptsname_r(ptm, buf.as_mut_ptr(), buf.len()) != 0 {
                libc::close(ptm);
                return Err(());
            }

            let name_cstr = std::ffi::CStr::from_ptr(buf.as_ptr());
            let c_pts = name_cstr.to_owned();
            (ptm, c_pts)
        }
    };

    unsafe {
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

        // ------------------------------------------------------------------
        // 预分配所有子进程需要的 CString / Vec —— 必须在 fork() 之前完成。
        // fork 后子进程中只有一个线程，若其他 Rust 线程在 fork 前持有
        // 全局分配器锁，子进程再 malloc 会死锁（phantom thread 问题）。
        // ------------------------------------------------------------------
        let c_cwd = if cwd_str.is_empty() {
            None
        } else {
            CString::new(cwd_str).ok()
        };
        let ptr_args: Vec<_> = c_exec_args
            .iter()
            .map(|s| s.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();
        let fallback_cmd = CString::new("/system/bin/sh").unwrap();
        let fallback_arg0 = CString::new("sh").unwrap();
        let fallback_args = [fallback_arg0.as_ptr(), std::ptr::null()];

        match fork() {
            Ok(ForkResult::Parent { child }) => Ok((ptm, child.as_raw())),
            Ok(ForkResult::Child) => {
                // Clear signals.
                let mut signals_to_unblock: libc::sigset_t = std::mem::zeroed();
                libc::sigfillset(&mut signals_to_unblock);
                libc::sigprocmask(libc::SIG_UNBLOCK, &signals_to_unblock, std::ptr::null_mut());

                libc::close(ptm);
                let _ = setsid();

                let pts = libc::open(c_pts.as_ptr(), libc::O_RDWR);
                if pts < 0 {
                    libc::_exit(-1);
                }

                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);
                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);

                if pts > 2 {
                    libc::close(pts);
                }

                // Close inherited file descriptors (except stdio) to match upstream behavior.
                // 使用纯 libc atoi（无分配），避免 fork 后调用 Rust 分配器。
                let self_dir = libc::opendir(b"/proc/self/fd\0".as_ptr() as *const _);
                if !self_dir.is_null() {
                    let self_dir_fd = libc::dirfd(self_dir);
                    loop {
                        let entry = libc::readdir(self_dir);
                        if entry.is_null() {
                            break;
                        }
                        let name_ptr = (*entry).d_name.as_ptr();
                        let fd = libc::atoi(name_ptr);
                        if fd > 2 && fd != self_dir_fd {
                            libc::close(fd);
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
                if let Some(ref c_cwd) = c_cwd {
                    let _ = chdir(c_cwd.as_c_str());
                }

                // Use execvp to search PATH and match upstream behavior
                libc::execvp(c_exec_cmd.as_ptr(), ptr_args.as_ptr());

                // --- If we reach here, execvp failed ---
                // 使用栈上静态缓冲区，避免 fork 后分配
                let mut buf = [0u8; 256];
                let msg = b"\r\n[Termux] execvp() failed (see errno)\r\n";
                let len = msg.len().min(buf.len());
                buf[..len].copy_from_slice(&msg[..len]);
                libc::write(2, buf.as_ptr() as *const _, len);

                // Fallback to /system/bin/sh as last resort
                libc::execvp(fallback_cmd.as_ptr(), fallback_args.as_ptr());
                libc::_exit(1);
            }
            Err(_) => Err(()),
        }
    }
}

pub fn write_to_fd(fd: jint, data: &[u8]) -> jint {
    if fd < 0 {
        return -1;
    }
    let res = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
    res as jint
}

/// 合并 watcher 状态到单个 Mutex，消除多锁竞争
struct WatcherState {
    map: HashMap<i32, WatcherCallback>,
    thread: Option<std::thread::JoinHandle<()>>,
}

static WATCHER_STATE: Lazy<Mutex<WatcherState>> = Lazy::new(|| {
    Mutex::new(WatcherState {
        map: HashMap::new(),
        thread: None,
    })
});
static WATCHER_SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[inline]
fn notify_process_exit(callback: &WatcherCallback, exit_code: i32) {
    #[cfg(not(feature = "test-helpers"))]
    {
        if let Some(vm) = crate::JAVA_VM.get() {
            if let Ok(mut env) = vm.attach_current_thread_as_daemon() {
                let _ = env.call_method(
                    callback.as_obj(),
                    "onProcessExited",
                    "(I)V",
                    &[jni::objects::JValue::Int(exit_code)],
                );
            }
        }
    }
    #[cfg(feature = "test-helpers")]
    {
        callback(exit_code);
    }
}

/// 启动全局子进程监视线程（需在已持有 WATCHER_STATE 锁时调用）
fn ensure_watcher_thread_locked(state: &mut WatcherState) {
    if let Some(ref handle) = state.thread {
        if !handle.is_finished() {
            return;
        }
    }

    let handle = std::thread::Builder::new()
        .name("ChildWatcher".to_string())
        .spawn(|| {
            #[cfg(feature = "test-helpers")]
            WATCHER_THREAD_COUNT.fetch_add(1, Ordering::SeqCst);
            android_log(LogPriority::INFO, "Global ChildWatcher thread started");
            loop {
                if WATCHER_SHUTDOWN.load(Ordering::SeqCst) {
                    android_log(
                        LogPriority::INFO,
                        "[Watcher] Shutdown signal received, exiting",
                    );
                    break;
                }
                let targets: Vec<(i32, WatcherCallback)> = {
                    let state = WATCHER_STATE.lock().unwrap();
                    if state.map.is_empty() {
                        drop(state);
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    state.map.iter().map(|(&k, v)| (k, v.clone())).collect()
                };

                for (pid, callback) in targets {
                    let mut status: i32 = 0;
                    let res = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };

                    if res == pid {
                        let exit_code = if libc::WIFEXITED(status) {
                            libc::WEXITSTATUS(status)
                        } else if libc::WIFSIGNALED(status) {
                            -libc::WTERMSIG(status)
                        } else {
                            0
                        };
                        android_log(
                            LogPriority::INFO,
                            &format!("[Watcher] Process {} exited with status {}", pid, exit_code),
                        );
                        {
                            let mut state = WATCHER_STATE.lock().unwrap();
                            state.map.remove(&pid);
                        }
                        notify_process_exit(&callback, exit_code);
                    } else if res == 0 {
                        // 仍在运行，不做任何操作
                    } else {
                        match nix::errno::Errno::last() {
                            nix::errno::Errno::ECHILD => {
                                android_log(
                                    LogPriority::INFO,
                                    &format!("[Watcher] Process {} already reaped (ECHILD)", pid),
                                );
                                {
                                    let mut state = WATCHER_STATE.lock().unwrap();
                                    state.map.remove(&pid);
                                }
                                notify_process_exit(&callback, 0);
                            }
                            nix::errno::Errno::EINTR => {
                                // 被信号中断，保留到下一轮检查
                            }
                            other => {
                                android_log(
                                    LogPriority::WARN,
                                    &format!("[Watcher] waitpid({}) failed: {:?}", pid, other),
                                );
                                {
                                    let mut state = WATCHER_STATE.lock().unwrap();
                                    state.map.remove(&pid);
                                }
                            }
                        }
                    }
                }

                std::thread::sleep(Duration::from_millis(100));
            }
        })
        .expect("Failed to spawn watcher thread");
    state.thread = Some(handle);
}

#[cfg(not(feature = "test-helpers"))]
pub fn spawn_waiter(pid: i32, callback: jni::objects::GlobalRef) {
    let mut state = WATCHER_STATE.lock().unwrap();
    state.map.insert(pid, callback);
    ensure_watcher_thread_locked(&mut state);
}

#[cfg(feature = "test-helpers")]
pub fn spawn_waiter(pid: i32, callback: std::sync::Arc<dyn Fn(i32) + Send + Sync>) {
    let mut state = WATCHER_STATE.lock().unwrap();
    state.map.insert(pid, callback);
    ensure_watcher_thread_locked(&mut state);
}

// -------------------------------------------------------------------------
// 测试辅助 API（仅用于 child_watcher_regression 等集成测试）
// -------------------------------------------------------------------------
#[cfg(feature = "test-helpers")]
pub fn watcher_map_len() -> usize {
    WATCHER_STATE.lock().unwrap().map.len()
}

#[cfg(feature = "test-helpers")]
pub fn reset_watcher_state() {
    // 1. 原子地获取 handle 并清空 map（单锁操作）
    let handle = {
        WATCHER_SHUTDOWN.store(true, Ordering::SeqCst);
        let mut state = WATCHER_STATE.lock().unwrap();
        state.map.clear();
        state.thread.take()
    };
    // 2. 在锁外 join，避免死锁
    if let Some(h) = handle {
        let _ = h.join();
    }
    // 3. 重置计数器
    WATCHER_THREAD_COUNT.store(0, Ordering::SeqCst);
    WATCHER_SHUTDOWN.store(false, Ordering::SeqCst);
}

#[cfg(feature = "test-helpers")]
pub fn watcher_thread_flag() -> bool {
    WATCHER_STATE.lock().unwrap().thread.is_some()
}

#[cfg(feature = "test-helpers")]
use std::sync::atomic::AtomicUsize;

#[cfg(feature = "test-helpers")]
static WATCHER_THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "test-helpers")]
pub fn watcher_thread_count() -> usize {
    WATCHER_THREAD_COUNT.load(Ordering::SeqCst)
}

pub fn set_pty_window_size(fd: jint, rows: jint, cols: jint, cell_width: jint, cell_height: jint) {
    if fd < 0 {
        return;
    }
    let sz = libc::winsize {
        ws_row: rows as u16,
        ws_col: cols as u16,
        ws_xpixel: (cols as u32 * cell_width as u32) as u16,
        ws_ypixel: (rows as u32 * cell_height as u32) as u16,
    };
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, &sz);
    }
}

pub fn wait_for(pid: i32) -> jint {
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

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // parse_shebang
    // -------------------------------------------------------------------------
    #[test]
    fn parse_shebang_basic() {
        let data = b"#!/bin/bash\necho hello";
        let result = parse_shebang(data);
        assert_eq!(result, Some(("/bin/bash".to_string(), None)));
    }

    #[test]
    fn parse_shebang_with_env() {
        let data = b"#!/usr/bin/env bash\necho hello";
        let result = parse_shebang(data);
        assert_eq!(
            result,
            Some(("/usr/bin/env".to_string(), Some("bash".to_string())))
        );
    }

    #[test]
    fn parse_shebang_with_args() {
        let data = b"#!/usr/bin/env python3 -u\nprint(1)";
        let result = parse_shebang(data);
        assert_eq!(
            result,
            Some(("/usr/bin/env".to_string(), Some("python3 -u".to_string())))
        );
    }

    #[test]
    fn parse_shebang_no_shebang() {
        let data = b"echo hello";
        assert_eq!(parse_shebang(data), None);
    }

    #[test]
    fn parse_shebang_empty_after_hashbang() {
        let data = b"#!   \necho hello";
        assert_eq!(parse_shebang(data), None);
    }

    #[test]
    fn parse_shebang_too_short() {
        let data = b"#";
        assert_eq!(parse_shebang(data), None);
    }

    #[test]
    fn parse_shebang_no_newline() {
        let data = b"#!/bin/sh";
        assert_eq!(parse_shebang(data), Some(("/bin/sh".to_string(), None)));
    }

    #[test]
    fn parse_shebang_extra_spaces() {
        let data = b"#!   /bin/bash   \n";
        assert_eq!(parse_shebang(data), Some(("/bin/bash".to_string(), None)));
    }

    // -------------------------------------------------------------------------
    // map_interpreter
    // -------------------------------------------------------------------------
    fn noop_normalize(s: String) -> String {
        s
    }

    #[test]
    fn map_interpreter_env() {
        let prefix = crate::get_termux_prefix();
        assert_eq!(
            map_interpreter("/usr/bin/env", &noop_normalize),
            format!("{}/bin/env", prefix)
        );
    }

    #[test]
    fn map_interpreter_bin_sh() {
        let prefix = crate::get_termux_prefix();
        assert_eq!(
            map_interpreter("/bin/sh", &noop_normalize),
            format!("{}/bin/sh", prefix)
        );
    }

    #[test]
    fn map_interpreter_usr_bin_awk() {
        let prefix = crate::get_termux_prefix();
        assert_eq!(
            map_interpreter("/usr/bin/awk", &noop_normalize),
            format!("{}/bin/awk", prefix)
        );
    }

    #[test]
    fn map_interpreter_termux_path() {
        let prefix = crate::get_termux_prefix();
        assert_eq!(
            map_interpreter(&format!("{}/bin/python", prefix), &noop_normalize),
            format!("{}/bin/python", prefix)
        );
    }

    #[test]
    fn map_interpreter_user_path() {
        assert_eq!(
            map_interpreter(
                "/data/user/0/com.termux/files/usr/bin/ruby",
                &noop_normalize
            ),
            "/data/user/0/com.termux/files/usr/bin/ruby"
        );
    }

    #[test]
    fn map_interpreter_custom_untouched() {
        assert_eq!(
            map_interpreter("/opt/local/bin/myapp", &noop_normalize),
            "/opt/local/bin/myapp"
        );
    }

    #[test]
    fn map_interpreter_absolute_termux() {
        let prefix = crate::get_termux_prefix();
        let normalize = |s: String| -> String {
            s.replace("/data/user/0/com.termux", &prefix.replace("/files/usr", ""))
        };
        // This test's expectation depends on how normalize is defined.
        // If we want to test that it is PASSED to normalize:
        assert_eq!(
            map_interpreter("/data/user/0/com.termux/files/usr/bin/perl", &normalize),
            normalize("/data/user/0/com.termux/files/usr/bin/perl".to_string())
        );
    }
}
