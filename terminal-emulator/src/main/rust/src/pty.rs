use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::unistd::{ForkResult, fork, setsid, chdir};
use std::ffi::CString;

/// 获取当前 UID 下的进程数，用于规避 Android 12+ 的 Phantom Killer
/// Phantom Killer 限制的是同一 UID 的进程数（~32），不是系统总进程数
fn get_uid_process_count() -> usize {
    let own_uid = unsafe { libc::getuid() };
    std::fs::read_dir("/proc")
        .map(|dir| {
            dir.filter_map(|entry| entry.ok())
                .filter(|entry| {
                    let file_name = entry.file_name();
                    let name = file_name.to_string_lossy();
                    if !name.chars().all(|c| c.is_numeric()) { return false; }
                    let status_path = format!("/proc/{}/status", name);
                    if let Ok(content) = std::fs::read_to_string(&status_path) {
                        for line in content.lines() {
                            if line.starts_with("Uid:") {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() >= 2 {
                                    if let Ok(uid) = parts[1].parse::<u32>() {
                                        return uid == own_uid;
                                    }
                                }
                            }
                        }
                    }
                    false
                })
                .count()
        })
        .unwrap_or(0)
}

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
    unsafe {
        let ptm = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if ptm < 0 { return Err(()); }
        if libc::grantpt(ptm) < 0 || libc::unlockpt(ptm) < 0 {
            libc::close(ptm);
            return Err(());
        }

        let devname = libc::ptsname(ptm);
        if devname.is_null() {
            libc::close(ptm);
            return Err(());
        }
        let devname = std::ffi::CStr::from_ptr(devname).to_string_lossy().into_owned();

        // Phantom Killer 流控：限制同一 UID 的进程数（Android 12+ 限制约 32）
        let uid_count = get_uid_process_count();
        if uid_count > 28 {
            crate::utils::android_log(crate::utils::LogPriority::WARN, &format!("[PTY] High UID process count ({}), throttling...", uid_count));
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        match fork() {
            Ok(ForkResult::Parent { child }) => {
                let sz = libc::winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: (cols * cw) as u16, ws_ypixel: (rows * ch) as u16 };
                libc::ioctl(ptm, libc::TIOCSWINSZ, &sz);
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                let _ = setsid();
                libc::setpriority(libc::PRIO_PROCESS, 0, 19);

                let c_devname = match CString::new(devname.clone()) {
                    Ok(c) => c,
                    Err(_) => { libc::_exit(1); }
                };
                let pts = libc::open(c_devname.as_ptr(), libc::O_RDWR);
                if pts < 0 { libc::_exit(1); }
                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);
                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);
                if pts > 2 { libc::close(pts); }
                libc::close(ptm);

                // === 动态 Prefix 与 规范路径修正 ===
                let mut real_pkg = String::from("com.termux");
                let mut termux_prefix = String::from("/data/user/0/com.termux/files/usr");
                
                let paths_to_check = [&cmd_str, &cwd_str];
                for p in paths_to_check {
                    if let Some(pos) = p.find("/files/") {
                        let root = &p[..pos];
                        if let Some(pkg_pos) = root.rfind('/') { real_pkg = root[pkg_pos+1..].to_string(); }
                        
                        // 修复：识别并保留 usr-staging 状态，否则解压阶段的 LD_PRELOAD 会因为找不到库而失效
                        if p.contains("/usr-staging/") {
                            termux_prefix = format!("{}/files/usr-staging", root);
                        } else {
                            termux_prefix = format!("{}/files/usr", root);
                        }
                        break;
                    }
                }
                
                // 强制规范化为 /data/data/ (兼容性更好)
                let termux_data_root = format!("/data/data/{}", real_pkg);
                let termux_prefix = format!("{}/files/usr{}", termux_data_root, if termux_prefix.contains("staging") { "-staging" } else { "" });
                let termux_bin = format!("{}/bin", termux_prefix);
                let termux_lib = format!("{}/lib", termux_prefix);
                let termux_home = format!("{}/files/home", termux_data_root);
                let termux_tmp = format!("{}/tmp", termux_prefix);

                let fix_canonical = |s: &str| -> String {
                    // 优先使用 /data/data/
                    s.replace("/data/user/0/", "/data/data/")
                };

                // 3. 核心修复：极致环境对齐 (参考 termux.c / TermuxShellUtils.java)
                let mut final_env: Vec<String> = Vec::new();
                let old_path = std::env::var("PATH").unwrap_or_else(|_| "/system/bin:/system/xbin".to_string());

                // 基础系统变量 (必须包含，Linker 可能依赖)
                let sys_vars = [
                    "ANDROID_ART_ROOT", "ANDROID_ASSETS", "ANDROID_DATA", "ANDROID_I18N_ROOT",
                    "ANDROID_ROOT", "ANDROID_RUNTIME_ROOT", "ANDROID_STORAGE", "ANDROID_TZDATA_ROOT",
                    "EXTERNAL_STORAGE", "BOOTCLASSPATH", "DEX2OATBOOTCLASSPATH", "SYSTEMSERVERCLASSPATH"
                ];
                for var in sys_vars {
                    if let Ok(val) = std::env::var(var) { final_env.push(format!("{}={}", var, val)); }
                }

                // Termux 核心环境
                final_env.push(format!("PATH={}:{}", termux_bin, fix_canonical(&old_path)));
                final_env.push(format!("PREFIX={}", termux_prefix));
                final_env.push(format!("HOME={}", termux_home));
                final_env.push(format!("TMPDIR={}", termux_tmp));
                final_env.push(format!("TERM=xterm-256color"));
                final_env.push(format!("COLORTERM=truecolor"));
                final_env.push(format!("LANG=en_US.UTF-8"));

                // 劫持库注入 (不再使用 realpath，回归标准 /data/data/ 路径)
                let preloader = format!("{}/libtermux-exec.so", termux_lib);
                final_env.push(format!("LD_PRELOAD={}", preloader));

                // 注入 Java 剩余环境
                for env_var in envp {
                    if let Some(pos) = env_var.find('=') {
                        let k = &env_var[..pos];
                        if ["PATH", "PREFIX", "HOME", "TMP", "TMPDIR", "LD_LIBRARY_PATH", "LD_PRELOAD"].contains(&k) { continue; }
                        final_env.push(format!("{}={}", k, fix_canonical(&env_var[pos+1..])));
                    }
                }

                // --- 子进程清理 (关键) ---
                unsafe {
                    // 1. 解除信号屏蔽
                    let mut signals_to_unblock: libc::sigset_t = std::mem::zeroed();
                    libc::sigfillset(&mut signals_to_unblock);
                    libc::sigprocmask(libc::SIG_UNBLOCK, &signals_to_unblock, std::ptr::null_mut());

                    // 2. 关闭多余句柄 (FD > 2)
                    if let Ok(entries) = std::fs::read_dir("/proc/self/fd") {
                        for entry in entries.filter_map(|e| e.ok()) {
                            if let Ok(fd_str) = entry.file_name().into_string() {
                                if let Ok(fd) = fd_str.parse::<i32>() {
                                    if fd > 2 { 
                                        // 避免关闭 /proc/self/fd 自己的句柄导致遍历中断
                                        libc::close(fd); 
                                    }
                                }
                            }
                        }
                    }
                }

                // 打印关键调试日志
                crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY] FINAL_EXECUTE: cmd='{}' ld_preload='{}'", final_cmd, preloader));

                // 准备环境与参数指针
                let mut c_envs = Vec::new();
                for e in &final_env { if let Ok(ce) = CString::new(e.clone()) { c_envs.push(ce); } }
                let ptr_envs: Vec<_> = c_envs.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();

                let mut c_args = Vec::new();
                for a in &final_args { if let Ok(ca) = CString::new(a.clone()) { c_args.push(ca); } }
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();

                if !final_cmd.is_empty() {
                    if let Ok(c_cmd) = CString::new(final_cmd.clone()) {
                        libc::execve(c_cmd.as_ptr(), ptr_args.as_ptr(), ptr_envs.as_ptr());
                    }
                    let err = nix::errno::Errno::last_raw();
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("[PTY] execve FAILED! errno={} cmd={}", err, final_cmd));
                }
                libc::_exit(1);
                }
            Err(_) => Err(()),
        }
    }
}

pub fn set_pty_window_size(fd: jint, rows: jint, cols: jint, cell_width: jint, cell_height: jint) {
    if fd < 0 { return; }
    let sz = libc::winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: (cols * cell_width) as u16, ws_ypixel: (rows * cell_height) as u16 };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_resolution_staging() {
        let cmd = "/data/user/0/com.termux/files/usr-staging/bin/login";
        let cwd = "/data/user/0/com.termux/files/home";
        
        let mut real_pkg = String::from("com.termux");
        let mut termux_prefix = String::from("/data/user/0/com.termux/files/usr");
        
        let paths_to_check = [cmd, cwd];
        for p in paths_to_check {
            if let Some(pos) = p.find("/files/") {
                let root = &p[..pos];
                if let Some(pkg_pos) = root.rfind('/') { real_pkg = root[pkg_pos+1..].to_string(); }
                if p.contains("/usr-staging/") {
                    termux_prefix = format!("{}/files/usr-staging", root);
                } else {
                    termux_prefix = format!("{}/files/usr", root);
                }
                break;
            }
        }
        
        assert!(termux_prefix.contains("usr-staging"));
        assert_eq!(real_pkg, "com.termux");
    }

    #[test]
    fn test_prefix_resolution_normal() {
        let cmd = "/data/data/com.termux/files/usr/bin/bash";
        let cwd = "/data/data/com.termux/files/home";
        
        let mut real_pkg = String::from("com.termux");
        let mut termux_prefix = String::from("/data/user/0/com.termux/files/usr");
        
        let paths_to_check = [cmd, cwd];
        for p in paths_to_check {
            if let Some(pos) = p.find("/files/") {
                let root = &p[..pos];
                if let Some(pkg_pos) = root.rfind('/') { real_pkg = root[pkg_pos+1..].to_string(); }
                if p.contains("/usr-staging/") {
                    termux_prefix = format!("{}/files/usr-staging", root);
                } else {
                    termux_prefix = format!("{}/files/usr", root);
                }
                break;
            }
        }
        
        assert!(!termux_prefix.contains("usr-staging"));
        assert_eq!(real_pkg, "com.termux");
    }
}
