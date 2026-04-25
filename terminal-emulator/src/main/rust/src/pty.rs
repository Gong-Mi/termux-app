use jni::JNIEnv;
use jni::objects::{JObjectArray, JString, JIntArray};
use jni::sys::{JNINativeInterface_, jint, jintArray, jobjectArray, jstring};
use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;
use nix::unistd::{ForkResult, fork, setsid, chdir, execv};
use std::ffi::CString;
use std::sync::Arc;

use crate::engine::TerminalContext;

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
    let mut env = JNIEnv::from_raw(env_ptr).unwrap();

    let cmd_str: String = env.get_string(&JString::from_raw(cmd)).unwrap().into();
    let cwd_str: String = env.get_string(&JString::from_raw(cwd)).unwrap().into();

    let mut argv = Vec::new();
    let args_obj = JObjectArray::from_raw(args);
    if !args_obj.is_null() {
        let len = env.get_array_length(&args_obj).unwrap();
        for i in 0..len {
            let arg_obj = env.get_object_array_element(&args_obj, i).unwrap();
            let arg_str: String = env.get_string(&JString::from(arg_obj)).unwrap().into();
            argv.push(arg_str);
        }
    }

    let mut envp = Vec::new();
    let env_vars_obj = JObjectArray::from_raw(env_vars);
    if !env_vars_obj.is_null() {
        let len = env.get_array_length(&env_vars_obj).unwrap();
        for i in 0..len {
            let env_obj = env.get_object_array_element(&env_vars_obj, i).unwrap();
            let env_str: String = env.get_string(&JString::from(env_obj)).unwrap().into();
            envp.push(env_str);
        }
    }

    match create_subprocess_with_data(cmd_str, cwd_str, argv, envp, rows, cols, cw, ch) {
        Ok((fd, pid)) => {
            let pid_val = [pid as jint];
            let j_pid_array = JIntArray::from_raw(process_id_array);
            env.set_int_array_region(&j_pid_array, 0, &pid_val).unwrap();
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

        match fork() {
            Ok(ForkResult::Parent { child }) => {
                let sz = libc::winsize {
                    ws_row: rows as u16,
                    ws_col: cols as u16,
                    ws_xpixel: (cols * cw) as u16,
                    ws_ypixel: (rows * ch) as u16,
                };
                libc::ioctl(ptm, libc::TIOCSWINSZ, &sz);
                Ok((ptm, child.as_raw()))
            }
            Ok(ForkResult::Child) => {
                let _ = setsid();
                
                // 降低子进程优先级 (Nice 19)
                libc::setpriority(libc::PRIO_PROCESS, 0, 19);

                let c_devname = CString::new(devname).unwrap();
                let pts = libc::open(c_devname.as_ptr(), libc::O_RDWR);
                if pts < 0 { libc::_exit(1); }

                libc::ioctl(pts, libc::TIOCSCTTY as _, 0);

                libc::dup2(pts, 0);
                libc::dup2(pts, 1);
                libc::dup2(pts, 2);
                if pts > 2 { libc::close(pts); }
                libc::close(ptm);

                // === 动态身份与 Prefix 检测 ===
                let mut real_pkg = String::from("com.termux");
                let mut termux_prefix = String::from("/data/data/com.termux/files/usr");
                
                // 从命令路径、当前工作目录探测真实沙盒根目录
                let paths_to_check = [&cmd_str, &cwd_str];
                for p in paths_to_check {
                    if let Some(pos) = p.find("/files/") {
                        let root = &p[..pos];
                        if let Some(pkg_pos) = root.rfind('/') {
                            real_pkg = root[pkg_pos+1..].to_string();
                        }
                        termux_prefix = format!("{}/files/usr", root);
                        break;
                    }
                }
                
                let real_data_root = termux_prefix.replace("/files/usr", "");
                let termux_bin = format!("{}/bin", termux_prefix);
                let termux_lib = format!("{}/lib", termux_prefix);
                let termux_home = format!("{}/home", termux_prefix.replace("/usr", ""));
                let termux_tmp = format!("{}/tmp", termux_prefix);

                // 辅助函数：将旧包名路径修正为当前真实包名
                let fix_path = |s: &str| -> String {
                    s.replace("/data/data/com.termux", &real_data_root)
                     .replace("/data/user/0/com.termux", &real_data_root)
                };

                // 1. 设置环境变量
                let old_path = std::env::var("PATH").unwrap_or_else(|_| "/system/bin:/system/xbin".to_string());
                libc::setenv(CString::new("PATH").unwrap().as_ptr(), CString::new(format!("{}:{}", termux_bin, fix_path(&old_path))).unwrap().as_ptr(), 1);
                libc::setenv(CString::new("PREFIX").unwrap().as_ptr(), CString::new(termux_prefix.clone()).unwrap().as_ptr(), 1);
                libc::setenv(CString::new("HOME").unwrap().as_ptr(), CString::new(termux_home).unwrap().as_ptr(), 1);
                libc::setenv(CString::new("TMPDIR").unwrap().as_ptr(), CString::new(termux_tmp).unwrap().as_ptr(), 1);
                libc::setenv(CString::new("LD_LIBRARY_PATH").unwrap().as_ptr(), CString::new(termux_lib.clone()).unwrap().as_ptr(), 1);
                libc::setenv(CString::new("TERM").unwrap().as_ptr(), CString::new("xterm-256color").unwrap().as_ptr(), 1);
                libc::setenv(CString::new("COLORTERM").unwrap().as_ptr(), CString::new("truecolor").unwrap().as_ptr(), 1);
                libc::setenv(CString::new("LANG").unwrap().as_ptr(), CString::new("en_US.UTF-8").unwrap().as_ptr(), 1);
                
                // LD_PRELOAD (核心：SDK 36 W^X Bypass)
                let termux_exec_candidates = [
                    "libtermux-exec-direct-ld-preload.so",
                    "libtermux-exec-linker-ld-preload.so",
                    "libtermux-exec.so",
                    "libtermux-exec-ld-preload.so",
                ];
                for candidate in &termux_exec_candidates {
                    let path = format!("{}/{}", termux_lib, candidate);
                    if std::path::Path::new(&path).exists() {
                        libc::setenv(CString::new("LD_PRELOAD").unwrap().as_ptr(), CString::new(path).unwrap().as_ptr(), 1);
                        break;
                    }
                }
                
                // 自定义变量
                for env_var in envp {
                    if let Some(pos) = env_var.find('=') {
                        let key = &env_var[..pos];
                        let value = fix_path(&env_var[pos+1..]);
                        if let (Ok(ck), Ok(cv)) = (CString::new(key), CString::new(value)) {
                            libc::setenv(ck.as_ptr(), cv.as_ptr(), 1);
                        }
                    }
                }

                if !cwd_str.is_empty() {
                    let fixed_cwd = fix_path(&cwd_str);
                    let c_cwd = CString::new(fixed_cwd).unwrap();
                    let _ = chdir(c_cwd.as_c_str());
                }
                
                // === 命令解析与加载器包装 ===
                let mut final_cmd = fix_path(&cmd_str);
                let mut final_args: Vec<String> = argv.iter().map(|a| fix_path(a)).collect();
                
                // 相对路径解析
                if !final_cmd.starts_with('/') {
                    let resolved = format!("{}/{}", termux_bin, final_cmd);
                    if std::path::Path::new(&resolved).exists() { final_cmd = resolved; }
                }

                // ELF/Shebang 检查
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
                                if !line.is_empty() {
                                    let parts: Vec<&str> = line.split_whitespace().collect();
                                    if !parts.is_empty() {
                                        shebang_interpreter = parts[0].to_string();
                                        shebang_args = parts[1..].iter().map(|&s| s.to_string()).collect();
                                    }
                                }
                            }
                        }
                    }
                }

                if has_shebang && !shebang_interpreter.is_empty() {
                    let mut interpreter = shebang_interpreter;
                    if interpreter.starts_with("/usr/bin/") || interpreter.starts_with("/bin/") {
                        let name = std::path::Path::new(&interpreter).file_name().unwrap().to_str().unwrap();
                        interpreter = format!("{}/bin/{}", termux_prefix, name);
                    }
                    let old_cmd = final_cmd.clone();
                    final_cmd = interpreter;
                    let mut new_argv = Vec::new();
                    new_argv.push(if !final_args.is_empty() && (final_args[0].starts_with('-') || final_args[0].starts_with('/')) { final_args[0].clone() } else { final_cmd.clone() });
                    new_argv.extend(shebang_args);
                    new_argv.push(old_cmd);
                    if final_args.len() > 1 { new_argv.extend(final_args.iter().skip(1).cloned()); }
                    final_args = new_argv;
                } else if !is_elf && !has_shebang {
                    let old_cmd = final_cmd.clone();
                    final_cmd = format!("{}/bin/sh", termux_prefix);
                    let mut new_argv = Vec::new();
                    new_argv.push(final_cmd.clone());
                    new_argv.push(old_cmd);
                    if final_args.len() > 1 { new_argv.extend(final_args.iter().skip(1).cloned()); }
                    final_args = new_argv;
                }

                // W^X Bypass
                let canonical = std::fs::canonicalize(&final_cmd).unwrap_or_else(|_| std::path::PathBuf::from(&final_cmd));
                let c_str = canonical.to_string_lossy();
                let needs_linker = final_cmd.contains("/data/") || c_str.contains("/data/") || final_cmd.contains(&real_pkg);

                if needs_linker {
                    #[cfg(target_pointer_width = "64")] let linker = "/system/bin/linker64";
                    #[cfg(target_pointer_width = "32")] let linker = "/system/bin/linker";
                    
                    if std::path::Path::new(linker).exists() {
                        let mut linker_argv = Vec::new();
                        linker_argv.push(linker.to_string()); // argv[0] for linker
                        linker_argv.push(final_cmd.clone());  // argv[1] for linker (binary to load)
                        linker_argv.extend(final_args);       // argv[2...] for child (including child's argv[0])
                        
                        final_args = linker_argv;
                        final_cmd = linker.to_string();
                    }
                }

                let mut c_args = Vec::new();
                for a in &final_args { if let Ok(ca) = CString::new(a.clone()) { c_args.push(ca); } }
                let ptr_args: Vec<_> = c_args.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();

                crate::utils::android_log(crate::utils::LogPriority::INFO, &format!("[PTY] final_exec cmd='{}' argv={:?}", final_cmd, final_args));

                if !final_cmd.is_empty() {
                    let c_cmd = CString::new(final_cmd).unwrap();
                    libc::execv(c_cmd.as_ptr(), ptr_args.as_ptr());
                    let err = nix::errno::Errno::last_raw();
                    crate::utils::android_log(crate::utils::LogPriority::ERROR, &format!("[PTY] execv FAILED! errno={} cmd_args={:?}", err, ptr_args));
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
