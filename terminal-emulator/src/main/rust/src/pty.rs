use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::unistd::{ForkResult, fork, setsid, chdir};
use std::ffi::CString;

/// 获取当前 UID 下的进程数，用于规避 Android 12+ 的 Phantom Killer
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
                        if p.contains("/usr-staging/") {
                            termux_prefix = format!("{}/files/usr-staging", root);
                        } else {
                            termux_prefix = format!("{}/files/usr", root);
                        }
                        break;
                    }
                }
                
                // 关键：强制使用 /data/data 形式以通过 Linker 白名单
                let termux_data_root = format!("/data/data/{}", real_pkg);
                let termux_files = format!("{}/files", termux_data_root);
                let termux_prefix_data = format!("{}/usr{}", termux_files, if termux_prefix.contains("staging") { "-staging" } else { "" });
                let termux_bin = format!("{}/bin", termux_prefix_data);
                let termux_lib = format!("{}/lib", termux_prefix_data);
                let termux_home = format!("{}/home", termux_files);

                // --- 环境变量外科手术式注入 (不使用 clearenv 以保持 Linker 信任) ---
                let set_env = |k: &str, v: &str| {
                    if let (Ok(ck), Ok(cv)) = (CString::new(k), CString::new(v)) {
                        unsafe { libc::setenv(ck.as_ptr(), cv.as_ptr(), 1); }
                    }
                };

                // 1. 注入核心路径
                set_env("PREFIX", &termux_prefix_data);
                set_env("HOME", &termux_home);
                
                let old_path = std::env::var("PATH").unwrap_or_else(|_| "/system/bin:/system/xbin".to_string());
                set_env("PATH", &format!("{}:{}", termux_bin, old_path.replace("/data/user/0/", "/data/data/")));
                
                set_env("LD_LIBRARY_PATH", &termux_lib);
                
                // 2. 注入关键绕过库 LD_PRELOAD
                let termux_exec_path = format!("{}/lib/libtermux-exec.so", termux_prefix_data);
                set_env("LD_PRELOAD", &termux_exec_path);

                // 3. 注入系统变量 (Linker 必需)
                let sys_vars = [
                    "ANDROID_ART_ROOT", "ANDROID_ASSETS", "ANDROID_DATA", "ANDROID_I18N_ROOT",
                    "ANDROID_ROOT", "ANDROID_RUNTIME_ROOT", "ANDROID_STORAGE", "ANDROID_TZDATA_ROOT",
                    "EXTERNAL_STORAGE", "BOOTCLASSPATH", "DEX2OATBOOTCLASSPATH", "SYSTEMSERVERCLASSPATH"
                ];
                for var in sys_vars {
                    if let Ok(val) = std::env::var(var) {
                        set_env(var, &val);
                    }
                }

                // 4. 注入 Java 剩余环境
                for env_var in envp {
                    if let Some(pos) = env_var.find('=') {
                        let k = &env_var[..pos];
                        if !["PATH", "PREFIX", "HOME", "LD_PRELOAD", "LD_LIBRARY_PATH"].contains(&k) {
                            set_env(k, &env_var[pos+1..].replace("/data/user/0/", "/data/data/"));
                        }
                    }
                }

                // 5. 关键安全变量强制注入
                // termux-exec 的隐藏开关：强制拦截后通过 /system/bin/linker 执行
                set_env("TERMUX_EXEC__SYSTEM_LINKER_EXEC__MODE", "force");

                if !cwd_str.is_empty() {
                    let c_cwd = CString::new(cwd_str.replace("/data/user/0/", "/data/data/")).unwrap();
                    let _ = chdir(c_cwd.as_c_str());
                }

                // ====== [PTY_CHECKPOINT] 开始分析 ======
                let mut final_cmd = cmd_str.replace("/data/user/0/", "/data/data/");
                if !final_cmd.starts_with('/') {
                    let resolved = format!("{}/{}", termux_bin, final_cmd);
                    if std::path::Path::new(&resolved).exists() { final_cmd = resolved; }
                }
                
                let mut final_args: Vec<String> = argv.iter().map(|a| a.replace("/data/user/0/", "/data/data/")).collect();
                
                // Login Shell 处理
                let is_shell = ["sh", "bash", "zsh", "dash", "fish"].iter().any(|&s| final_cmd.ends_with(s));
                if is_shell {
                    let shell_name = std::path::Path::new(&final_cmd).file_name().and_then(|n| n.to_str()).unwrap_or("sh");
                    if final_args.is_empty() {
                        final_args.push(format!("-{}", shell_name));
                    } else if !final_args[0].starts_with('-') {
                        final_args[0] = format!("-{}", shell_name);
                    }
                }

                // ELF/Shebang 逻辑
                let mut is_elf = false;
                let mut has_shebang = false;
                let mut shebang_interpreter = String::new();
                let mut shebang_args = Vec::new();
                if let Ok(mut f) = std::fs::File::open(&final_cmd) {
                    use std::io::Read;
                    let mut buf = [0u8; 256];
                    if let Ok(n) = f.read(&mut buf) {
                        if n > 4 && &buf[..4] == b"\x7fELF" { is_elf = true; }
                        else if n > 2 && &buf[..2] == b"#!" {
                            has_shebang = true;
                            if let Ok(s) = std::str::from_utf8(&buf[2..n]) {
                                let line = s.lines().next().unwrap_or("").trim();
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if !parts.is_empty() {
                                    shebang_interpreter = parts[0].to_string();
                                    shebang_args = parts[1..].iter().map(|&s| s.to_string()).collect();
                                }
                            }
                        }
                    }
                }

                if has_shebang && !shebang_interpreter.is_empty() {
                    let mut interpreter = shebang_interpreter;
                    if interpreter.starts_with("/usr/bin/") || interpreter.starts_with("/bin/") {
                        if let Some(name) = std::path::Path::new(&interpreter).file_name().and_then(|n| n.to_str()) {
                            interpreter = format!("{}/bin/{}", termux_prefix_data, name);
                        }
                    }
                    let old_cmd = final_cmd.clone();
                    final_cmd = interpreter;
                    let mut new_argv = Vec::new();
                    new_argv.push(if !final_args.is_empty() { final_args[0].clone() } else { final_cmd.clone() });
                    new_argv.extend(shebang_args);
                    new_argv.push(old_cmd);
                    if argv.len() > 1 { new_argv.extend(argv.iter().skip(1).cloned()); }
                    final_args = new_argv;
                } else if !is_elf && !has_shebang {
                    let old_cmd = final_cmd.clone();
                    final_cmd = format!("{}/bin/sh", termux_prefix_data);
                    let mut new_argv = Vec::new();
                    new_argv.push(if !final_args.is_empty() { final_args[0].clone() } else { final_cmd.clone() });
                    new_argv.push(old_cmd);
                    if argv.len() > 1 { new_argv.extend(argv.iter().skip(1).cloned()); }
                    final_args = new_argv;
                }

                // W^X Bypass: 标准 Linker 协议构建
                let canonical_target = std::fs::canonicalize(&final_cmd).unwrap_or_else(|_| std::path::PathBuf::from(&final_cmd));
                let c_str = canonical_target.to_string_lossy();
                
                let needs_linker = final_cmd.contains(&real_pkg) || final_cmd.contains("/data/") || c_str.contains("/data/");

                if needs_linker {
                    #[cfg(target_pointer_width = "64")] let linker = "/system/bin/linker64";
                    #[cfg(target_pointer_width = "32")] let linker = "/system/bin/linker";
                    
                    if std::path::Path::new(linker).exists() {
                        let mut linker_argv = Vec::new();
                        let prog_name = if !final_args.is_empty() { final_args[0].clone() } else { final_cmd.clone() };
                        linker_argv.push(prog_name);         // argv[0]
                        linker_argv.push(final_cmd.clone()); // argv[1]
                        if final_args.len() > 1 {
                            linker_argv.extend(final_args.into_iter().skip(1));
                        }
                        final_args = linker_argv;
                        final_cmd = linker.to_string();
                    }
                }

                // 子进程清理
                unsafe {
                    let mut signals_to_unblock: libc::sigset_t = std::mem::zeroed();
                    libc::sigfillset(&mut signals_to_unblock);
                    libc::sigprocmask(libc::SIG_UNBLOCK, &signals_to_unblock, std::ptr::null_mut());
                }

                let mut c_args = Vec::new();
                for a in &final_args { if let Ok(ca) = CString::new(a.clone()) { c_args.push(ca); } }
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();

                crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY_CHECKPOINT] CP07: final_exec cmd='{}' argv={:?}", final_cmd, final_args));

                if !final_cmd.is_empty() {
                    let c_cmd = CString::new(final_cmd.clone()).unwrap();
                    libc::execv(c_cmd.as_ptr(), ptr_args.as_ptr());
                    
                    let err = nix::errno::Errno::last_raw();
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("[PTY_CHECKPOINT] CP08: execv FAILED! errno={} cmd={}", err, final_cmd));
                }
                libc::_exit(1);
            }
            Err(e) => {
                crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("[PTY] fork FAILED: {:?}", e));
                Err(())
            }
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
