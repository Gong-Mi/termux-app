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
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                let _ = setsid();
                
                // 降低子进程优先级 (Nice 19)，减少对系统负载的冲击，从而规避 Phantom Killer
                unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 19); }

                let c_devname = CString::new(devname).unwrap();
                let pts = libc::open(c_devname.as_ptr(), libc::O_RDWR);
                if pts < 0 { libc::_exit(1); }

                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);

                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);
                if pts > 2 { libc::close(pts); }
                libc::close(ptm);

                libc::clearenv();
                for env_var in envp {
                    if let Ok(c_env) = CString::new(env_var) {
                        libc::putenv(c_env.into_raw());
                    }
                }

                if !cwd_str.is_empty() {
                    let c_cwd = CString::new(cwd_str).unwrap();
                    let _ = chdir(c_cwd.as_c_str());
                }

                let mut c_args = Vec::new();
                for arg in argv {
                    if let Ok(ca) = CString::new(arg) { c_args.push(ca); }
                }
                
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
                if !cmd_str.is_empty() {
                    let c_cmd = CString::new(cmd_str).unwrap();
                    libc::execvp(c_cmd.as_ptr(), ptr_args.as_ptr());
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
