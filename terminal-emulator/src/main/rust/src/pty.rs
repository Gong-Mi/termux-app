use jni::JNIEnv;
use jni::objects::{JObjectArray, JString};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;
use nix::unistd::{ForkResult, chdir, close, fork, setsid};
use std::ffi::{CStr, CString};

// 模拟 C 里的 grantpt, unlockpt, ptsname_r
unsafe extern "C" {
    fn grantpt(fd: i32) -> i32;
    fn unlockpt(fd: i32) -> i32;
    fn ptsname_r(fd: i32, buf: *mut libc::c_char, buflen: usize) -> i32;
}

/// # Safety
///
/// This function is unsafe because it interacts with raw JNI pointers and performs
/// low-level process creation (fork, exec) and PTY operations. The caller must
/// ensure that the JNI environment pointer and Java object handles are valid.
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
    unsafe {
        let mut env = match JNIEnv::from_raw(env_ptr) {
            Ok(e) => e,
            Err(_) => return -1,
        };

        let cmd_str: String = env.get_string(&JString::from_raw(cmd)).unwrap().into();
        let cwd_str: String = env.get_string(&JString::from_raw(cwd)).unwrap().into();

        let mut argv = Vec::new();
        let args_obj = JObjectArray::from_raw(args);
        if !args_obj.is_null() {
            let len = env.get_array_length(&args_obj).unwrap();
            for i in 0..len {
                let arg_java: JString = env.get_object_array_element(&args_obj, i).unwrap().into();
                let arg_str: String = env.get_string(&arg_java).unwrap().into();
                argv.push(arg_str);
            }
        }

        let mut envp = Vec::new();
        let env_vars_obj = JObjectArray::from_raw(env_vars);
        if !env_vars_obj.is_null() {
            let len = env.get_array_length(&env_vars_obj).unwrap();
            for i in 0..len {
                let env_java: JString = env
                    .get_object_array_element(&env_vars_obj, i)
                    .unwrap()
                    .into();
                let env_str: String = env.get_string(&env_java).unwrap().into();
                envp.push(env_str);
            }
        }

        // 1. 打开 PTM
        use std::os::fd::IntoRawFd;
        let ptm = match open("/dev/ptmx", OFlag::O_RDWR | OFlag::O_CLOEXEC, Mode::empty()) {
            Ok(fd) => fd.into_raw_fd(),
            Err(_) => return -1,
        };

        if grantpt(ptm) != 0 || unlockpt(ptm) != 0 {
            let _ = close(ptm);
            return -1;
        }

        let mut devname_buf = [0u8; 64];
        if ptsname_r(
            ptm,
            devname_buf.as_mut_ptr() as *mut libc::c_char,
            devname_buf.len(),
        ) != 0
        {
            let _ = close(ptm);
            return -1;
        }
        let devname = CStr::from_ptr(devname_buf.as_ptr() as *const libc::c_char).to_owned();

        // 2. 设置初始 winsize
        let sz = libc::winsize {
            ws_row: rows as u16,
            ws_col: columns as u16,
            ws_xpixel: (columns * cell_width) as u16,
            ws_ypixel: (rows * cell_height) as u16,
        };
        let _ = libc::ioctl(ptm, libc::TIOCSWINSZ, &sz);

        // 3. Fork
        match fork() {
            Ok(ForkResult::Parent { child }) => {
                let internal = env.get_native_interface();
                let mut is_copy = jni::sys::JNI_FALSE;
                let pid_ptr = ((**internal).GetPrimitiveArrayCritical.unwrap())(
                    internal,
                    process_id_array,
                    &mut is_copy,
                ) as *mut i32;
                if !pid_ptr.is_null() {
                    *pid_ptr = child.as_raw();
                    ((**internal).ReleasePrimitiveArrayCritical.unwrap())(
                        internal,
                        process_id_array,
                        pid_ptr as *mut _,
                        0,
                    );
                }
                ptm as jint
            }
            Ok(ForkResult::Child) => {
                // 子进程：避免在这里调用任何复杂的 Rust/JNI 逻辑
                // 必须手动关闭 ptm，因为它是 O_CLOEXEC 的，但在 exec 之前我们已经拿到了它的拷贝
                // 实际上，dup2 后的 fd 0,1,2 才是我们需要的。

                let _ = setsid();

                let pts = match open(devname.as_c_str(), OFlag::O_RDWR, Mode::empty()) {
                    Ok(fd) => fd.into_raw_fd(),
                    Err(_) => libc::_exit(1),
                };

                // 绑定标准输入输出
                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);
                if pts > 2 {
                    libc::close(pts);
                }
                libc::close(ptm);

                // 清理并设置环境变量
                libc::clearenv();
                for env_var in envp {
                    let c_env = CString::new(env_var).unwrap();
                    libc::putenv(c_env.into_raw()); // 泄露内存是安全的，因为即将 exec
                }

                let _ = chdir(cwd_str.as_str());

                // 构造参数
                // 根据 TermuxSession.java，传入的 argv 数组第一个元素已经是进程名（可能是 "-bash"）
                let mut c_args = Vec::new();
                for arg in argv {
                    c_args.push(CString::new(arg).unwrap());
                }
                // 如果 argv 为空（防御性编程），则至少放入命令名
                if c_args.is_empty() {
                    c_args.push(CString::new(cmd_str.clone()).unwrap());
                }

                let ptr_args: Vec<_> = c_args
                    .iter()
                    .map(|s| s.as_ptr())
                    .chain(std::iter::once(std::ptr::null()))
                    .collect();

                let c_cmd = CString::new(cmd_str).unwrap();
                libc::execvp(c_cmd.as_ptr(), ptr_args.as_ptr());
                libc::_exit(1);
            }
            Err(_) => -1,
        }
    }
}

pub fn set_pty_window_size(fd: jint, rows: jint, cols: jint, cell_width: jint, cell_height: jint) {
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
