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
    // === 1. 预解析与路径规范化 (在 Parent 进程完成，确保线程安全) ===
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
    
    let termux_data_root = format!("/data/data/{}", real_pkg);
    let termux_files = format!("{}/files", termux_data_root);
    let termux_prefix_data = format!("{}/usr{}", termux_files, if termux_prefix.contains("staging") { "-staging" } else { "" });
    let termux_bin = format!("{}/bin", termux_prefix_data);
    let termux_lib = format!("{}/lib", termux_prefix_data);
    let termux_home = format!("{}/home", termux_files);

    // === 2. 命令与参数决议 (在 Parent 进程完成) ===
    let mut final_cmd = cmd_str.replace("/data/user/0/", "/data/data/");
    if !final_cmd.starts_with('/') {
        let resolved = format!("{}/{}", termux_bin, final_cmd);
        if std::path::Path::new(&resolved).exists() { final_cmd = resolved; }
    }
    
    let mut final_args: Vec<String> = argv.iter().map(|a| a.replace("/data/user/0/", "/data/data/")).collect();
    
    // ELF 探测
    let mut is_actual_elf = false;
    if let Ok(mut f) = std::fs::File::open(&final_cmd) {
        use std::io::Read;
        let mut magic = [0u8; 4];
        if f.read(&mut magic).is_ok() && &magic == b"\x7fELF" { is_actual_elf = true; }
    }

    // 脚本转换
    if !is_actual_elf && (final_cmd.contains(&real_pkg) || final_cmd.contains("/data/")) {
        let mut sh_argv = Vec::new();
        sh_argv.push(String::from("sh"));
        sh_argv.push(final_cmd.clone());
        if final_args.len() > 1 { sh_argv.extend(final_args.into_iter().skip(1)); }
        final_args = sh_argv;
        final_cmd = format!("{}/bin/sh", termux_prefix_data);
        is_actual_elf = true; 
    }

    // Go/Static 扫描
    let is_static_or_go = |path: &str| -> bool {
        if let Ok(mut f) = std::fs::File::open(path) {
            use std::io::Read;
            let mut buffer = [0u8; 4096];
            if f.read(&mut buffer).is_ok() {
                let content_str = String::from_utf8_lossy(&buffer);
                if content_str.contains("Go build ID") || content_str.contains("Go runtime") { return true; }
            }
            let name = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("");
            if ["gh", "docker", "podman", "terraform", "rclone"].contains(&name) { return true; }
        }
        false
    };

    let mut wrapper_dir_to_inject = None;
    if is_static_or_go(&final_cmd) {
        let wrapper_dir = format!("{}/bin-wrappers", termux_prefix_data);
        if std::fs::create_dir_all(&wrapper_dir).is_ok() {
            let sh_p = format!("{}/bin/sh", termux_prefix_data);
            for (name, rp) in [("git", format!("{}/bin/git", termux_prefix_data)), ("ssh", format!("{}/bin/ssh", termux_prefix_data))] {
                let wp = format!("{}/{}", wrapper_dir, name);
                let content = format!("#!{}\nexec /system/bin/linker64 {} \"$@\"\n", sh_p, rp);
                let _ = std::fs::write(&wp, content);
                if let Ok(c_p) = CString::new(wp) { unsafe { libc::chmod(c_p.as_ptr(), 0o755); } }
            }
            wrapper_dir_to_inject = Some(wrapper_dir);
        }
    }

    // Login Shell 处理
    let is_shell = ["sh", "bash", "zsh", "dash", "fish"].iter().any(|&s| final_cmd.ends_with(s));
    if is_shell {
        let shell_name = std::path::Path::new(&final_cmd).file_name().and_then(|n| n.to_str()).unwrap_or("sh");
        if final_args.is_empty() { final_args.push(format!("-{}", shell_name)); }
        else if !final_args[0].starts_with('-') { final_args[0] = format!("-{}", shell_name); }
    }

    // Linker 包装
    if is_actual_elf && (final_cmd.contains(&real_pkg) || final_cmd.contains("/data/")) {
        #[cfg(target_pointer_width = "64")] let linker = "/system/bin/linker64";
        #[cfg(target_pointer_width = "32")] let linker = "/system/bin/linker";
        if std::path::Path::new(linker).exists() {
            let mut linker_argv = Vec::new();
            linker_argv.push(if !final_args.is_empty() { final_args[0].clone() } else { final_cmd.clone() });
            linker_argv.push(final_cmd.clone());
            if final_args.len() > 1 { linker_argv.extend(final_args.into_iter().skip(1)); }
            final_args = linker_argv;
            final_cmd = linker.to_string();
        }
    }

    // === 3. 准备子进程环境与清理 (在 Parent 进程完成) ===
    let uid_count = get_uid_process_count();
    if uid_count > 28 {
        crate::utils::android_log(crate::utils::LogPriority::WARN, &format!("[PTY] High UID process count ({}), throttling...", uid_count));
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

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
        let devname_str = std::ffi::CStr::from_ptr(devname).to_string_lossy().into_owned();

        match fork() {
            Ok(ForkResult::Parent { child }) => {
                let ch = ch as u16;
                let cw = cw as u16;
                let sz = libc::winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: (cols as u16 * cw), ws_ypixel: (rows as u16 * ch) };
                libc::ioctl(ptm, libc::TIOCSWINSZ, &sz);
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                // --- 以下进入 Async-Signal-Safe 区域 ---
                let _ = setsid();
                libc::setpriority(libc::PRIO_PROCESS, 0, 19);

                let c_devname = match CString::new(devname_str) {
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

                // 外科手术式设置环境变量
                let mut set_env_safe = |k: &str, v: &str| {
                    if let (Ok(ck), Ok(cv)) = (CString::new(k), CString::new(v)) {
                        libc::setenv(ck.as_ptr(), cv.as_ptr(), 1);
                    }
                };

                set_env_safe("PREFIX", &termux_prefix_data);
                set_env_safe("HOME", &termux_home);
                let old_p = std::env::var("PATH").unwrap_or_else(|_| "/system/bin".to_string());
                let final_path = if let Some(wd) = wrapper_dir_to_inject {
                    format!("{}:{}:{}", wd, termux_bin, old_p.replace("/data/user/0/", "/data/data/"))
                } else {
                    format!("{}:{}", termux_bin, old_p.replace("/data/user/0/", "/data/data/"))
                };
                set_env_safe("PATH", &final_path);
                set_env_safe("LD_LIBRARY_PATH", &termux_lib);
                set_env_safe("LD_PRELOAD", &format!("{}/lib/libtermux-exec.so", termux_prefix_data));
                set_env_safe("TERMUX_EXEC__SYSTEM_LINKER_EXEC__MODE", "force");

                // 系统变量继承
                let sys_vars = ["ANDROID_ART_ROOT", "ANDROID_ASSETS", "ANDROID_DATA", "ANDROID_I18N_ROOT", "ANDROID_ROOT", "ANDROID_RUNTIME_ROOT", "ANDROID_STORAGE", "ANDROID_TZDATA_ROOT", "EXTERNAL_STORAGE", "BOOTCLASSPATH", "DEX2OATBOOTCLASSPATH", "SYSTEMSERVERCLASSPATH"];
                for var in sys_vars {
                    if let Ok(val) = std::env::var(var) { set_env_safe(var, &val); }
                }

                // Java 变量注入
                for env_var in envp {
                    if let Some(pos) = env_var.find('=') {
                        let k = &env_var[..pos];
                        if !["PATH", "PREFIX", "HOME", "LD_PRELOAD", "LD_LIBRARY_PATH"].contains(&k) {
                            set_env_safe(k, &env_var[pos+1..].replace("/data/user/0/", "/data/data/"));
                        }
                    }
                }

                if !cwd_str.is_empty() {
                    let c_cwd = CString::new(cwd_str.replace("/data/user/0/", "/data/data/")).unwrap();
                    let _ = chdir(c_cwd.as_c_str());
                }

                // 信号清理
                let mut signals_to_unblock: libc::sigset_t = std::mem::zeroed();
                libc::sigfillset(&mut signals_to_unblock);
                libc::sigprocmask(libc::SIG_UNBLOCK, &signals_to_unblock, std::ptr::null_mut());

                // 准备 C 风格参数
                let mut c_args = Vec::new();
                for a in &final_args { if let Ok(ca) = CString::new(a.clone()) { c_args.push(ca); } }
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();

                // 最终执行
                if let Ok(c_cmd) = CString::new(final_cmd.clone()) {
                    libc::execv(c_cmd.as_ptr(), ptr_args.as_ptr());
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
    let sz = libc::winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: (cols as u16 * cell_width as u16), ws_ypixel: (rows as u16 * cell_height as u16) };
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
