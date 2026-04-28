use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::unistd::{ForkResult, fork, setsid, chdir};
use std::ffi::CString;

use crate::utils::{android_log, LogPriority};

pub unsafe fn create_subprocess(
    env_ptr: *mut *const JNINativeInterface_,
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
    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => e,
        Err(_) => return -1,
    };

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
    // 1. 动态路径解析 - 优先使用 /data/data/ 路径以绕过某些 Android 版本的执行限制
    let sanitized_cwd = cwd_str.replace("/data/user/0/", "/data/data/");
    let data_home_pos = sanitized_cwd.find("/files/home");
    let (termux_files, termux_prefix) = if let Some(pos) = data_home_pos {
        let base = &sanitized_cwd[..pos + 6]; // "/data/data/com.termux/files"
        (base.to_string(), format!("{}/usr", base))
    } else {
        // 回退到默认，但尽量保持通用
        ("/data/data/com.termux/files".to_string(), "/data/data/com.termux/files/usr".to_string())
    };
    
    let termux_bin = format!("{}/bin", termux_prefix);
    let termux_home = format!("{}/home", termux_files);

    // 2. 环境序列化
    let mut final_env = Vec::new();
    
    // 预设核心环境变量
    let old_p = std::env::var("PATH").unwrap_or_else(|_| "/system/bin".to_string());
    // 确保 PATH 中只有 /data/data/ 路径
    let sanitized_path = old_p.replace("/data/user/0/", "/data/data/");
    final_env.push(format!("PATH={}:{}", termux_bin, sanitized_path));
    final_env.push(format!("PREFIX={}", termux_prefix));
    final_env.push(format!("HOME={}", termux_home));
    final_env.push(format!("LD_PRELOAD={}/lib/libtermux-exec.so", termux_prefix));
    final_env.push("TERM=xterm-256color".to_string());

    for var in ["ANDROID_DATA", "ANDROID_ROOT", "EXTERNAL_STORAGE", "BOOTCLASSPATH"] {
        if let Ok(val) = std::env::var(var) { final_env.push(format!("{}={}", var, val)); }
    }

    for env_var in envp {
        if let Some(pos) = env_var.find('=') {
            let k = &env_var[..pos];
            // 避免覆盖核心变量，除非是显式传递
            if !["PATH", "PREFIX", "HOME", "LD_PRELOAD"].contains(&k) {
                final_env.push(env_var.replace("/data/user/0/", "/data/data/"));
            }
        }
    }

    // 3. 命令决议
    let mut final_cmd = cmd_str.replace("/data/user/0/", "/data/data/");
    if !final_cmd.starts_with('/') {
        let resolved = format!("{}/{}", termux_bin, final_cmd);
        if std::path::Path::new(&resolved).exists() { final_cmd = resolved; }
    }

    // 确保 argv[0] 是程序名称
    let mut final_args: Vec<String> = Vec::new();
    final_args.push(final_cmd.clone());
    for arg in argv {
        final_args.push(arg.replace("/data/user/0/", "/data/data/"));
    }

    android_log(LogPriority::INFO, &format!("[TRACE_SESSION] Preparing to exec: {} with args: {:?}", final_cmd, final_args));

    let c_cmd = CString::new(final_cmd.clone()).unwrap();
    let c_args: Vec<CString> = final_args.iter().map(|a| CString::new(a.clone()).unwrap()).collect();
    let c_envs: Vec<CString> = final_env.iter().map(|e| CString::new(e.clone()).unwrap()).collect();

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

                // 此时 stderr 已经指向 PTY
                let msg = "Child: PTY setup complete, about to execve\n";
                let _ = libc::write(2, msg.as_ptr() as *const _, msg.len());

                // 尝试确保二进制文件具有可执行权限
                let c_path = CString::new(final_cmd.clone()).unwrap();
                libc::chmod(c_path.as_ptr(), 0o700);

                // Clear environment and rebuild.
                libc::clearenv();
                for env_str in &c_envs {
                    libc::putenv(env_str.as_ptr() as *mut _);
                }

                if !cwd_str.is_empty() {
                    let final_cwd = cwd_str.replace("/data/user/0/", "/data/data/");
                    if let Ok(c_cwd) = CString::new(final_cwd) {
                        let _ = chdir(c_cwd.as_c_str());
                    }
                }

                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr())
                    .chain(std::iter::once(std::ptr::null())).collect();
                let ptr_envs: Vec<_> = c_envs.iter().map(|s| s.as_ptr())
                    .chain(std::iter::once(std::ptr::null())).collect();

                libc::execve(c_cmd.as_ptr(), ptr_args.as_ptr(), ptr_envs.as_ptr());

                // Fallback to /system/bin/sh
                let fallback_cmd = CString::new("/system/bin/sh").unwrap();
                let fallback_arg0 = CString::new("sh").unwrap();
                let fallback_args = [fallback_arg0.as_ptr(), std::ptr::null()];
                
                libc::execve(fallback_cmd.as_ptr(), fallback_args.as_ptr(), ptr_envs.as_ptr());
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
