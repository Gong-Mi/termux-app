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
    // 2. 环境序列化
    let mut final_env = Vec::new();
    for env_var in envp {
        final_env.push(env_var);
    }
    
    android_log(LogPriority::INFO, &format!("[TRACE_SESSION] Preparing to exec: {} with argv: {:?}", cmd_str, argv));

    let c_cmd = CString::new(cmd_str).unwrap();
    let c_args: Vec<CString> = argv.iter().map(|a| CString::new(a.clone()).unwrap()).collect();
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

                // Close inherited file descriptors (except stdio) to match upstream behavior.
                // Use raw libc::opendir/readdir/closedir (not Rust std::fs::read_dir) so we
                // can call dirfd() to exclude the directory's own fd from being closed.
                unsafe {
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

                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr())
                    .chain(std::iter::once(std::ptr::null())).collect();

                // Use execvp to search PATH and match upstream behavior
                libc::execvp(c_cmd.as_ptr(), ptr_args.as_ptr());

                // Fallback to /system/bin/sh
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
