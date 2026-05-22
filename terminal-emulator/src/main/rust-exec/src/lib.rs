use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

static REAL_EXECVE: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static REAL_EXECVEAT: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static REAL_POSIX_SPAWN: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static REAL_POSIX_SPAWNP: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static CURRENT_PREFIX: OnceLock<String> = OnceLock::new();
static MY_PHYSICAL_PATH: OnceLock<String> = OnceLock::new();

const RTLD_NEXT: *mut libc::c_void = -1i64 as *mut libc::c_void;

unsafe extern "C" {
    static environ: *const *const c_char;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".init_array")]
pub static LOAD_HOOK: unsafe extern "C" fn() = {
    unsafe extern "C" fn init() {
        debug_log("INTERCEPTOR LOADED");
    }
    init
};

fn debug_log(msg: &str) {
    let pid = std::process::id();
    for p in ["/data/user/0/com.termux", "/data/data/com.termux"] {
        let log_path = format!("{}/files/home/termux_exec_debug.log", p);
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(file, "[{}] {}", pid, msg);
            break;
        }
    }
}

unsafe fn argv_to_debug(argv: *const *const c_char) -> String {
    let mut values = Vec::new();
    let mut i = 0;
    while !argv.is_null() && !(*argv.offset(i)).is_null() && i < 32 {
        values.push(
            CStr::from_ptr(*argv.offset(i))
                .to_string_lossy()
                .into_owned(),
        );
        i += 1;
    }
    format!("{:?}", values)
}

fn canonical_or_original(path: String) -> String {
    std::fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(path)
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn get_my_physical_path() -> &'static str {
    MY_PHYSICAL_PATH.get_or_init(|| {
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        if unsafe { libc::dladdr(execve as *const libc::c_void, &mut info) } != 0 {
            if !info.dli_fname.is_null() {
                let path = unsafe { CStr::from_ptr(info.dli_fname) }
                    .to_string_lossy()
                    .into_owned();
                if !path.is_empty() {
                    return path;
                }
            }
        }
        String::new()
    })
}

fn get_current_prefix() -> &'static str {
    CURRENT_PREFIX.get_or_init(|| {
        if let Ok(maps) = std::fs::read_to_string("/proc/self/maps") {
            for line in maps.lines() {
                if line.contains("com.termux")
                    && (line.contains("/lib/") || line.contains("/files/"))
                {
                    if let Some(start) = line.find("/data/") {
                        if let Some(end) = line[start..].find("/files/") {
                            return line[start..start + end].to_string();
                        }
                    }
                }
            }
        }
        "/data/user/0/com.termux".to_string()
    })
}

fn map_termux_path(path: &str) -> String {
    let prefix = get_current_prefix();
    if path.starts_with("/usr/bin/") {
        format!("{}/files/usr/bin/{}", prefix, &path[9..])
    } else if path.starts_with("/bin/") {
        format!("{}/files/usr/bin/{}", prefix, &path[5..])
    } else if path.starts_with("/usr/lib/") {
        format!("{}/files/usr/lib/{}", prefix, &path[9..])
    } else {
        path.to_string()
    }
}

unsafe fn resolve_path(file: &str) -> Option<String> {
    let mapped = map_termux_path(file);
    if mapped.contains('/') {
        if std::path::Path::new(&mapped).exists() {
            return Some(canonical_or_original(mapped));
        }
        if file != mapped && std::path::Path::new(file).exists() {
            return Some(canonical_or_original(file.to_string()));
        }
        return None;
    }
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(':') {
            let full = format!("{}/{}", dir, file);
            if std::path::Path::new(&full).exists() {
                return Some(canonical_or_original(full));
            }
        }
    }
    None
}

unsafe fn fix_envp(envp: *const *const c_char, proc_self_exe: Option<&str>) -> Vec<CString> {
    let mut new_env = Vec::new();
    let my_path = get_my_physical_path();
    let mut i = 0;
    let mut ld_preload_found = false;
    let mut proc_self_exe_found = false;
    let mut path_found = false;

    while !envp.is_null() && !(*envp.offset(i)).is_null() {
        let s = CStr::from_ptr(*envp.offset(i)).to_string_lossy();
        if s.starts_with("LD_PRELOAD=") {
            new_env.push(CString::new(format!("LD_PRELOAD={}", my_path)).unwrap());
            ld_preload_found = true;
        } else if let Some(path_value) = s.strip_prefix("PATH=") {
            new_env.push(CString::new(format!("PATH={}", path_value)).unwrap());
            path_found = true;
        } else if s.starts_with("TERMUX_EXEC__PROC_SELF_EXE=") {
            if let Some(path) = proc_self_exe {
                new_env.push(CString::new(format!("TERMUX_EXEC__PROC_SELF_EXE={}", path)).unwrap());
            } else {
                new_env.push(CStr::from_ptr(*envp.offset(i)).to_owned());
            }
            proc_self_exe_found = true;
        } else {
            new_env.push(CStr::from_ptr(*envp.offset(i)).to_owned());
        }
        i += 1;
    }

    if !ld_preload_found && !my_path.is_empty() {
        new_env.push(CString::new(format!("LD_PRELOAD={}", my_path)).unwrap());
    }
    if !path_found {
        let fallback_path = format!(
            "{}/files/usr/bin:{}/files/usr/bin/applets:/system/bin",
            get_current_prefix(),
            get_current_prefix()
        );
        new_env.push(CString::new(format!("PATH={}", fallback_path)).unwrap());
    }
    if !proc_self_exe_found && proc_self_exe.is_some() {
        new_env.push(
            CString::new(format!(
                "TERMUX_EXEC__PROC_SELF_EXE={}",
                proc_self_exe.unwrap()
            ))
            .unwrap(),
        );
    }

    new_env
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execve(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    let mut ptr = REAL_EXECVE.load(Ordering::Relaxed);
    if ptr.is_null() {
        ptr = libc::dlsym(RTLD_NEXT, b"execve\0".as_ptr() as *const c_char);
        REAL_EXECVE.store(ptr, Ordering::Relaxed);
    }
    let real_execve: unsafe extern "C" fn(
        *const c_char,
        *const *const c_char,
        *const *const c_char,
    ) -> c_int = std::mem::transmute(ptr);

    if path.is_null() {
        return real_execve(path, argv, envp);
    }
    let path_str = CStr::from_ptr(path).to_string_lossy().to_string();

    if path_str.starts_with("/system/")
        || path_str.starts_with("/vendor/")
        || path_str.contains("/linker")
    {
        return real_execve(path, argv, envp);
    }

    debug_log(&format!("TRY_EXECVE: {}", path_str));

    if let Some(full_path) = resolve_path(&path_str) {
        if full_path.contains("com.termux") && !full_path.contains("/applib/") {
            if let Some((final_cmd, new_argv_vec)) = transform_exec(&full_path, argv) {
                debug_log(&format!(
                    "TRANSFORM: {} -> {} argv={}",
                    full_path,
                    final_cmd,
                    argv_to_debug(
                        new_argv_vec
                            .iter()
                            .map(|s| s.as_ptr())
                            .chain(std::iter::once(ptr::null()))
                            .collect::<Vec<_>>()
                            .as_ptr()
                    )
                ));
                let c_cmd = CString::new(final_cmd.clone()).unwrap();
                let c_ptrs: Vec<*const c_char> = new_argv_vec
                    .iter()
                    .map(|s| s.as_ptr())
                    .chain(std::iter::once(ptr::null()))
                    .collect();
                let new_env = fix_envp(envp, Some(&full_path));
                let env_ptrs: Vec<*const c_char> = new_env
                    .iter()
                    .map(|s| s.as_ptr())
                    .chain(std::iter::once(ptr::null()))
                    .collect();
                let result = real_execve(c_cmd.as_ptr(), c_ptrs.as_ptr(), env_ptrs.as_ptr());
                debug_log(&format!(
                    "EXECVE_FAILED: {} errno={} argv={}",
                    final_cmd,
                    last_errno(),
                    argv_to_debug(c_ptrs.as_ptr())
                ));
                return result;
            }
        }
    }

    let new_env = fix_envp(envp, None);
    let env_ptrs: Vec<*const c_char> = new_env
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(ptr::null()))
        .collect();
    let result = real_execve(path, argv, env_ptrs.as_ptr());
    debug_log(&format!(
        "EXECVE_PASSTHROUGH_FAILED: {} errno={} argv={}",
        path_str,
        last_errno(),
        argv_to_debug(argv)
    ));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

    /// 证明 canonical_or_original 引入的 readlink 系统调用开销
    #[test]
    fn test_canonicalize_overhead() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_canonicalize");
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();
        let target = tmp_dir.join("testfile");
        fs::File::create(&target).unwrap();

        let path_str = target.to_string_lossy().to_string();
        let iterations = 1000;

        // 测量 canonical_or_original（内部调用 std::fs::canonicalize = readlink 系统调用）
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = canonical_or_original(path_str.clone());
        }
        let canonical_time = start.elapsed();

        // 测量纯字符串 clone（更新前的行为）
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = path_str.clone();
        }
        let clone_time = start.elapsed();

        println!(
            "canonicalize: {:?}, clone: {:?}, ratio: {:.2}x",
            canonical_time,
            clone_time,
            canonical_time.as_nanos() as f64 / clone_time.as_nanos().max(1) as f64
        );

        // canonicalize 应显著慢于纯字符串 clone（通常 10x 以上，保守用 3x）
        assert!(
            canonical_time > clone_time * 3,
            "canonical_or_original should be measurably slower than string clone due to readlink syscall"
        );

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    /// 证明 PATH 变长后，在最坏情况下（目标不在 prepend 目录中）
    /// 每次 resolve_path 都需要多 stat 一个目录。
    #[test]
    fn test_path_search_overhead() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_path");
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();

        // 模拟一个放在 PATH 最前面但不含目标的额外目录。
        let prepended_dir = tmp_dir.join("prepended");
        fs::create_dir(&prepended_dir).unwrap();

        // 创建 20 个目录模拟剩余 PATH，只在最后一个放目标文件
        let mut long_path_parts = vec![prepended_dir.to_string_lossy().to_string()];
        for i in 0..20 {
            let dir = tmp_dir.join(format!("bin{}", i));
            fs::create_dir(&dir).unwrap();
            long_path_parts.push(dir.to_string_lossy().to_string());
        }
        let last_dir = tmp_dir.join("bin19");
        fs::File::create(last_dir.join("my-cmd")).unwrap();

        let long_path = long_path_parts.join(":");
        let short_path = long_path_parts[1..].join(":"); // 去掉额外前置目录

        const ITERATIONS: usize = 1000;

        // 测量长 PATH（含额外前置目录）查找时间
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            for dir in long_path.split(':') {
                let full = format!("{}/my-cmd", dir);
                if std::path::Path::new(&full).exists() {
                    break;
                }
            }
        }
        let long_time = start.elapsed();

        // 测量短 PATH（不含额外前置目录）查找时间
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            for dir in short_path.split(':') {
                let full = format!("{}/my-cmd", dir);
                if std::path::Path::new(&full).exists() {
                    break;
                }
            }
        }
        let short_time = start.elapsed();

        println!(
            "long PATH ({} dirs): {:?}, short PATH ({} dirs): {:?}, \
             extra overhead: {:?}",
            long_path.split(':').count(),
            long_time,
            short_path.split(':').count(),
            short_time,
            long_time.saturating_sub(short_time)
        );

        // 长 PATH 比短 PATH 多一个空目录的 stat，因此应该更慢或持平
        assert!(
            long_time >= short_time,
            "PATH with an extra prepended directory should be >= short PATH \
             when target is not in the prepended dir"
        );

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execveat(
    dirfd: c_int,
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
    flags: c_int,
) -> c_int {
    let mut ptr = REAL_EXECVEAT.load(Ordering::Relaxed);
    if ptr.is_null() {
        ptr = libc::dlsym(RTLD_NEXT, b"execveat\0".as_ptr() as *const c_char);
        REAL_EXECVEAT.store(ptr, Ordering::Relaxed);
    }
    let real_execveat: unsafe extern "C" fn(
        c_int,
        *const c_char,
        *const *const c_char,
        *const *const c_char,
        c_int,
    ) -> c_int = std::mem::transmute(ptr);

    if path.is_null() {
        return real_execveat(dirfd, path, argv, envp, flags);
    }
    let path_str = CStr::from_ptr(path).to_string_lossy().to_string();
    debug_log(&format!("TRY_EXECVEAT: {}", path_str));

    if (path_str.contains("com.termux")
        || path_str.starts_with("/usr/")
        || path_str.starts_with("/bin/")
        || !path_str.starts_with('/'))
        && dirfd == libc::AT_FDCWD
    {
        return execve(path, argv, envp);
    }

    let result = real_execveat(dirfd, path, argv, envp, flags);
    debug_log(&format!(
        "EXECVEAT_FAILED: dirfd={} path={} flags={} errno={} argv={}",
        dirfd,
        path_str,
        flags,
        last_errno(),
        argv_to_debug(argv)
    ));
    result
}

fn transform_exec(path: &str, orig_argv: *const *const c_char) -> Option<(String, Vec<CString>)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = [0u8; 256];
    let n = file.read(&mut buffer).ok()?;
    let current_prefix = get_current_prefix();
    let linker = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(path).unwrap());
        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }
        return Some((linker.to_string(), new_argv));
    } else if let Some((interpreter, shebang_args)) =
        parse_shebang_internal(&buffer[..n], current_prefix)
    {
        let mut new_argv = Vec::new();
        if interpreter.contains("com.termux") {
            new_argv.push(CString::new(linker).unwrap());
            new_argv.push(CString::new(interpreter.clone()).unwrap());
        } else {
            new_argv.push(CString::new(interpreter.clone()).unwrap());
        }

        if let Some(args) = shebang_args {
            for arg in args.split_whitespace() {
                new_argv.push(CString::new(arg).unwrap());
            }
        }
        new_argv.push(CString::new(path).unwrap());

        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }

        if interpreter.contains("com.termux") {
            return Some((linker.to_string(), new_argv));
        }
        return Some((interpreter, new_argv));
    } else if path.contains("com.termux") {
        let sh_path = format!("{}/files/usr/bin/sh", current_prefix);
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(sh_path.clone()).unwrap());
        new_argv.push(CString::new(path).unwrap());

        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }
        return Some((linker.to_string(), new_argv));
    }
    None
}

fn parse_shebang_internal(buffer: &[u8], current_prefix: &str) -> Option<(String, Option<String>)> {
    if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' {
        return None;
    }
    let line_end = buffer
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(buffer.len());
    let line = String::from_utf8_lossy(&buffer[2..line_end]);
    let trimmed = line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let mut interpreter = tokens[0].to_string();
    if interpreter.starts_with("/usr/bin/env") {
        interpreter = format!("{}/files/usr/bin/env", current_prefix);
    } else if interpreter.starts_with("/bin/") || interpreter.starts_with("/usr/bin/") {
        let binary = interpreter.rsplit('/').next().unwrap_or("sh");
        interpreter = format!("{}/files/usr/bin/{}", current_prefix, binary);
    }

    let args = if tokens.len() > 1 {
        Some(tokens[1..].join(" "))
    } else {
        None
    };
    Some((interpreter, args))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execv(path: *const c_char, argv: *const *const c_char) -> c_int {
    execve(path, argv, environ)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int {
    execvpe(file, argv, environ)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvpe(
    file: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    execve(file, argv, envp)
}

unsafe fn fixed_variadic_argv(arg0: *const c_char, args: &[*const c_char]) -> Vec<*const c_char> {
    let mut argv = Vec::with_capacity(args.len() + 2);
    argv.push(arg0);
    for &arg in args {
        if arg.is_null() {
            break;
        }
        argv.push(arg);
    }
    argv.push(ptr::null());
    argv
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execl(
    path: *const c_char,
    arg0: *const c_char,
    arg1: *const c_char,
    arg2: *const c_char,
    arg3: *const c_char,
    arg4: *const c_char,
    arg5: *const c_char,
    arg6: *const c_char,
    arg7: *const c_char,
) -> c_int {
    let argv = fixed_variadic_argv(arg0, &[arg1, arg2, arg3, arg4, arg5, arg6, arg7]);
    execve(path, argv.as_ptr(), environ)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execlp(
    file: *const c_char,
    arg0: *const c_char,
    arg1: *const c_char,
    arg2: *const c_char,
    arg3: *const c_char,
    arg4: *const c_char,
    arg5: *const c_char,
    arg6: *const c_char,
    arg7: *const c_char,
) -> c_int {
    let argv = fixed_variadic_argv(arg0, &[arg1, arg2, arg3, arg4, arg5, arg6, arg7]);
    execvpe(file, argv.as_ptr(), environ)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execle(
    path: *const c_char,
    arg0: *const c_char,
    arg1: *const c_char,
    arg2: *const c_char,
    arg3: *const c_char,
    arg4: *const c_char,
    arg5: *const c_char,
    arg6: *const c_char,
    arg7: *const c_char,
) -> c_int {
    let args = [arg1, arg2, arg3, arg4, arg5, arg6, arg7];
    let mut envp = environ;
    for (index, &arg) in args.iter().enumerate() {
        if arg.is_null() {
            if let Some(next) = args.get(index + 1) {
                envp = *next as *const *const c_char;
            }
            break;
        }
    }
    let argv = fixed_variadic_argv(arg0, &args);
    execve(path, argv.as_ptr(), envp)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn termux_exec_rs_execve(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    execve(path, argv, envp)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn termux_exec_rs_execvpe(
    file: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    execvpe(file, argv, envp)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn(
    pid: *mut libc::pid_t,
    path: *const c_char,
    file_actions: *const libc::c_void,
    attrp: *const libc::c_void,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    let mut ptr = REAL_POSIX_SPAWN.load(Ordering::Relaxed);
    if ptr.is_null() {
        ptr = libc::dlsym(RTLD_NEXT, b"posix_spawn\0".as_ptr() as *const c_char);
        REAL_POSIX_SPAWN.store(ptr, Ordering::Relaxed);
    }
    let real_spawn: unsafe extern "C" fn(
        *mut libc::pid_t,
        *const c_char,
        *const libc::c_void,
        *const libc::c_void,
        *const *const c_char,
        *const *const c_char,
    ) -> c_int = std::mem::transmute(ptr);

    if path.is_null() {
        return real_spawn(pid, path, file_actions, attrp, argv, envp);
    }
    let path_str = CStr::from_ptr(path).to_string_lossy().to_string();

    if path_str.starts_with("/system/")
        || path_str.starts_with("/vendor/")
        || path_str.contains("/linker")
    {
        return real_spawn(pid, path, file_actions, attrp, argv, envp);
    }

    debug_log(&format!("TRY_SPAWN: {}", path_str));

    if let Some(full_path) = resolve_path(&path_str) {
        if full_path.contains("com.termux") && !full_path.contains("/applib/") {
            if let Some((final_cmd, new_argv_vec)) = transform_exec(&full_path, argv) {
                debug_log(&format!(
                    "SPAWN: {} -> {} argv={}",
                    full_path,
                    final_cmd,
                    argv_to_debug(
                        new_argv_vec
                            .iter()
                            .map(|s| s.as_ptr())
                            .chain(std::iter::once(ptr::null()))
                            .collect::<Vec<_>>()
                            .as_ptr()
                    )
                ));
                let c_cmd = CString::new(final_cmd.clone()).unwrap();
                let c_ptrs: Vec<*const c_char> = new_argv_vec
                    .iter()
                    .map(|s| s.as_ptr())
                    .chain(std::iter::once(ptr::null()))
                    .collect();
                let new_env = fix_envp(envp, Some(&full_path));
                let env_ptrs: Vec<*const c_char> = new_env
                    .iter()
                    .map(|s| s.as_ptr())
                    .chain(std::iter::once(ptr::null()))
                    .collect();
                let result = real_spawn(
                    pid,
                    c_cmd.as_ptr(),
                    file_actions,
                    attrp,
                    c_ptrs.as_ptr(),
                    env_ptrs.as_ptr(),
                );
                if result != 0 {
                    debug_log(&format!(
                        "SPAWN_FAILED: {} result={} errno={} argv={}",
                        final_cmd,
                        result,
                        last_errno(),
                        argv_to_debug(c_ptrs.as_ptr())
                    ));
                }
                return result;
            }
        }
    }

    let new_env = fix_envp(envp, None);
    let env_ptrs: Vec<*const c_char> = new_env
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(ptr::null()))
        .collect();
    let result = real_spawn(pid, path, file_actions, attrp, argv, env_ptrs.as_ptr());
    if result != 0 {
        debug_log(&format!(
            "SPAWN_PASSTHROUGH_FAILED: {} result={} errno={} argv={}",
            path_str,
            result,
            last_errno(),
            argv_to_debug(argv)
        ));
    }
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnp(
    pid: *mut libc::pid_t,
    file: *const c_char,
    file_actions: *const libc::c_void,
    attrp: *const libc::c_void,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    let mut ptr = REAL_POSIX_SPAWNP.load(Ordering::Relaxed);
    if ptr.is_null() {
        ptr = libc::dlsym(RTLD_NEXT, b"posix_spawnp\0".as_ptr() as *const c_char);
        REAL_POSIX_SPAWNP.store(ptr, Ordering::Relaxed);
    }
    let real_spawnp: unsafe extern "C" fn(
        *mut libc::pid_t,
        *const c_char,
        *const libc::c_void,
        *const libc::c_void,
        *const *const c_char,
        *const *const c_char,
    ) -> c_int = std::mem::transmute(ptr);

    if file.is_null() {
        return real_spawnp(pid, file, file_actions, attrp, argv, envp);
    }
    let file_str = CStr::from_ptr(file).to_string_lossy().to_string();

    if file_str.starts_with("/system/")
        || file_str.starts_with("/vendor/")
        || file_str.contains("/linker")
    {
        return real_spawnp(pid, file, file_actions, attrp, argv, envp);
    }

    debug_log(&format!("TRY_SPAWNP: {}", file_str));

    if let Some(full_path) = resolve_path(&file_str) {
        if full_path.contains("com.termux") && !full_path.contains("/applib/") {
            if let Some((final_cmd, new_argv_vec)) = transform_exec(&full_path, argv) {
                debug_log(&format!(
                    "SPAWNP: {} -> {} argv={}",
                    full_path,
                    final_cmd,
                    argv_to_debug(
                        new_argv_vec
                            .iter()
                            .map(|s| s.as_ptr())
                            .chain(std::iter::once(ptr::null()))
                            .collect::<Vec<_>>()
                            .as_ptr()
                    )
                ));
                let c_cmd = CString::new(final_cmd.clone()).unwrap();
                let c_ptrs: Vec<*const c_char> = new_argv_vec
                    .iter()
                    .map(|s| s.as_ptr())
                    .chain(std::iter::once(ptr::null()))
                    .collect();
                let new_env = fix_envp(envp, Some(&full_path));
                let env_ptrs: Vec<*const c_char> = new_env
                    .iter()
                    .map(|s| s.as_ptr())
                    .chain(std::iter::once(ptr::null()))
                    .collect();
                let result = real_spawnp(
                    pid,
                    c_cmd.as_ptr(),
                    file_actions,
                    attrp,
                    c_ptrs.as_ptr(),
                    env_ptrs.as_ptr(),
                );
                if result != 0 {
                    debug_log(&format!(
                        "SPAWNP_FAILED: {} result={} errno={} argv={}",
                        final_cmd,
                        result,
                        last_errno(),
                        argv_to_debug(c_ptrs.as_ptr())
                    ));
                }
                return result;
            }
        }
    }

    let new_env = fix_envp(envp, None);
    let env_ptrs: Vec<*const c_char> = new_env
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(ptr::null()))
        .collect();
    let result = real_spawnp(pid, file, file_actions, attrp, argv, env_ptrs.as_ptr());
    if result != 0 {
        debug_log(&format!(
            "SPAWNP_PASSTHROUGH_FAILED: {} result={} errno={} argv={}",
            file_str,
            result,
            last_errno(),
            argv_to_debug(argv)
        ));
    }
    result
}
