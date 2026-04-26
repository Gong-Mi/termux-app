use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::unistd::{ForkResult, fork, setsid, chdir};
use std::ffi::CString;

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
    // 1. 预解析路径 (Parent)
    let mut real_pkg = String::from("com.termux");
    let mut termux_prefix = String::from("/data/user/0/com.termux/files/usr");
    
    if let Some(pos) = cmd_str.find("/files/") {
        let root = &cmd_str[..pos];
        if let Some(pkg_pos) = root.rfind('/') { real_pkg = root[pkg_pos+1..].to_string(); }
        termux_prefix = format!("{}/files/usr{}", root, if cmd_str.contains("/usr-staging/") { "-staging" } else { "" });
    } else if let Some(pos) = cwd_str.find("/files/") {
        let root = &cwd_str[..pos];
        if let Some(pkg_pos) = root.rfind('/') { real_pkg = root[pkg_pos+1..].to_string(); }
        termux_prefix = format!("{}/files/usr", root);
    }
    
    let termux_data_root = format!("/data/data/{}", real_pkg);
    let termux_files = format!("{}/files", termux_data_root);
    let termux_prefix_data = format!("{}/usr{}", termux_files, if termux_prefix.contains("staging") { "-staging" } else { "" });
    let termux_bin = format!("{}/bin", termux_prefix_data);
    let termux_home = format!("{}/home", termux_files);

    // 2. 部署核心马甲 (Physical PATH Interception)
    // 使用 /system/bin/sh 作为解释器以避开 W^X 拦截，强制通过 Linker 运行关键工具
    let wrapper_dir = format!("{}/bin-wrappers", termux_prefix_data);
    let _ = std::fs::create_dir_all(&wrapper_dir);
    for (name, rp) in [("git", format!("{}/bin/git", termux_prefix_data)), ("ssh", format!("{}/bin/ssh", termux_prefix_data))] {
        let wp = format!("{}/{}", wrapper_dir, name);
        let content = format!("#!/system/bin/sh\nexec /system/bin/linker64 {} \"$@\"\n", rp);
        let _ = std::fs::write(&wp, content);
        let _ = std::path::Path::new(&wp).metadata().map(|m| {
            use std::os::unix::fs::PermissionsExt;
            let mut p = m.permissions();
            p.set_mode(0o755);
            let _ = std::fs::set_permissions(&wp, p);
        });
    }

    // 3. 命令决议与包装
    let mut final_cmd = cmd_str.replace("/data/user/0/", "/data/data/");
    if !final_cmd.starts_with('/') {
        let resolved = format!("{}/{}", termux_bin, final_cmd);
        if std::path::Path::new(&resolved).exists() { final_cmd = resolved; }
    }
    let mut final_args: Vec<String> = argv.iter().map(|a| a.replace("/data/user/0/", "/data/data/")).collect();
    
    let mut is_actual_elf = false;
    if let Ok(mut f) = std::fs::File::open(&final_cmd) {
        use std::io::Read;
        let mut magic = [0u8; 4];
        if f.read_exact(&mut magic).is_ok() && &magic == b"\x7fELF" { is_actual_elf = true; }
    }

    // 递归转换脚本
    if !is_actual_elf && (final_cmd.contains(&real_pkg) || final_cmd.contains("/data/")) {
        let mut sh_argv = Vec::new();
        sh_argv.push(String::from("sh"));
        sh_argv.push(final_cmd.clone());
        if final_args.len() > 1 { sh_argv.extend(final_args.into_iter().skip(1)); }
        final_args = sh_argv;
        final_cmd = format!("{}/bin/sh", termux_prefix_data);
        is_actual_elf = true; 
    }

    // Login Shell 特殊处理
    let is_shell = ["sh", "bash", "zsh", "dash", "fish"].iter().any(|&s| final_cmd.ends_with(s));
    if is_shell {
        let shell_name = std::path::Path::new(&final_cmd).file_name().and_then(|n| n.to_str()).unwrap_or("sh");
        if final_args.is_empty() { final_args.push(format!("-{}", shell_name)); }
        else if !final_args[0].starts_with('-') { final_args[0] = format!("-{}", shell_name); }
    }

    // 最终包装
    if is_actual_elf && (final_cmd.contains(&real_pkg) || final_cmd.contains("/data/")) {
        #[cfg(target_pointer_width = "64")] let linker = "/system/bin/linker64";
        #[cfg(target_pointer_width = "32")] let linker = "/system/bin/linker";
        if std::path::Path::new(linker).exists() {
            let mut linker_argv = Vec::new();
            let prog_name = if !final_args.is_empty() { final_args[0].clone() } else { final_cmd.clone() };
            linker_argv.push(prog_name);
            linker_argv.push(final_cmd.clone());
            if final_args.len() > 1 { linker_argv.extend(final_args.into_iter().skip(1)); }
            final_args = linker_argv;
            final_cmd = linker.to_string();
        }
    }

    // 4. 环境准备
    let mut final_env = Vec::new();
    let old_p = std::env::var("PATH").unwrap_or_else(|_| "/system/bin".to_string());
    // 强制注入马甲路径到 PATH 首位
    let fp = format!("{}:{}:{}", wrapper_dir, termux_bin, old_p.replace("/data/user/0/", "/data/data/"));
    final_env.push(format!("PATH={}", fp));
    final_env.push(format!("PREFIX={}", termux_prefix_data));
    final_env.push(format!("HOME={}", termux_home));
    final_env.push(format!("LD_PRELOAD={}/lib/libtermux-exec.so", termux_prefix_data));
    final_env.push(format!("TERMUX_EXEC__SYSTEM_LINKER_EXEC__MODE=force"));

    for var in ["ANDROID_ART_ROOT", "ANDROID_ASSETS", "ANDROID_DATA", "ANDROID_I18N_ROOT", "ANDROID_ROOT", "ANDROID_RUNTIME_ROOT", "ANDROID_STORAGE", "ANDROID_TZDATA_ROOT", "EXTERNAL_STORAGE", "BOOTCLASSPATH", "DEX2OATBOOTCLASSPATH", "SYSTEMSERVERCLASSPATH"] {
        if let Ok(val) = std::env::var(var) { final_env.push(format!("{}={}", var, val)); }
    }

    for env_var in envp {
        if let Some(pos) = env_var.find('=') {
            let k = &env_var[..pos];
            if !["PATH", "PREFIX", "HOME", "LD_PRELOAD", "LD_LIBRARY_PATH"].contains(&k) {
                final_env.push(env_var.replace("/data/user/0/", "/data/data/"));
            }
        }
    }

    crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY_CHECKPOINT] FINAL_PLAN: cmd='{}' argv={:?}", final_cmd, final_args));

    let c_cmd = CString::new(final_cmd).unwrap();
    let c_args: Vec<CString> = final_args.iter().map(|a| CString::new(a.clone()).unwrap()).collect();
    let c_envs: Vec<CString> = final_env.iter().map(|e| CString::new(e.clone()).unwrap()).collect();

    unsafe {
        let ptm = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if ptm < 0 { return Err(()); }
        libc::grantpt(ptm); libc::unlockpt(ptm);
        let devname = libc::ptsname(ptm);
        let devname_str = std::ffi::CStr::from_ptr(devname).to_string_lossy().into_owned();

        match fork() {
            Ok(ForkResult::Parent { child }) => {
                let sz = libc::winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: (cols as u32 * cw as u32) as u16, ws_ypixel: (rows as u32 * ch as u32) as u16 };
                libc::ioctl(ptm, libc::TIOCSWINSZ, &sz);
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                let _ = setsid();
                let c_pts = CString::new(devname_str).unwrap();
                let pts = libc::open(c_pts.as_ptr(), libc::O_RDWR);
                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);
                libc::dup2(pts, 0); libc::dup2(pts, 1); libc::dup2(pts, 2);
                libc::close(ptm);

                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
                let ptr_envs: Vec<_> = c_envs.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();

                libc::execve(c_cmd.as_ptr(), ptr_args.as_ptr(), ptr_envs.as_ptr());
                libc::_exit(1);
            }
            Err(_) => Err(()),
        }
    }
}

pub fn set_pty_window_size(fd: jint, rows: jint, cols: jint, cell_width: jint, cell_height: jint) {
    if fd < 0 { return; }
    let sz = libc::winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: (cols as u32 * cell_width as u32) as u16, ws_ypixel: (rows as u32 * cell_height as u32) as u16 };
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
