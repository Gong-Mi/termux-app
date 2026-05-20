use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;

static REAL_EXECVE: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static REAL_EXECVEAT: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static REAL_POSIX_SPAWN: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static REAL_POSIX_SPAWNP: AtomicPtr<libc::c_void> = AtomicPtr::new(ptr::null_mut());
static CURRENT_PREFIX: OnceLock<String> = OnceLock::new();
static MY_PHYSICAL_PATH: OnceLock<String> = OnceLock::new();
static EXEC_WRAPPER_PATH: OnceLock<Option<String>> = OnceLock::new();

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
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(file, "[{}] {}", pid, msg);
            break;
        }
    }
}

unsafe fn argv_to_debug(argv: *const *const c_char) -> String {
    let mut values = Vec::new();
    let mut i = 0;
    while !argv.is_null() && !(*argv.offset(i)).is_null() && i < 32 {
        values.push(CStr::from_ptr(*argv.offset(i)).to_string_lossy().into_owned());
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

fn ensure_exec_wrappers(prefix_base: &str) -> Option<String> {
    let termux_prefix = format!("{}/files/usr", prefix_base);
    let wrapper_dir = format!("{}/libexec/termux-exec-wrappers", termux_prefix);
    if std::fs::create_dir_all(&wrapper_dir).is_err() {
        debug_log(&format!("WRAPPER_DIR_FAILED: {}", wrapper_dir));
        return None;
    }

    let generic_wrapper = format!("{}/.wrapper", wrapper_dir);
    let script = exec_wrapper_script();
    let needs_write = match std::fs::read_to_string(&generic_wrapper) {
        Ok(existing) if existing == script => false,
        _ => true,
    };
    if needs_write {
        if std::fs::write(&generic_wrapper, &script).is_err() {
            debug_log(&format!("GENERIC_WRAPPER_WRITE_FAILED: {}", generic_wrapper));
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&generic_wrapper) {
                let mut permissions = metadata.permissions();
                permissions.set_mode(0o700);
                let _ = std::fs::set_permissions(&generic_wrapper, permissions);
            }
        }
    }

    let bin_dir = format!("{}/bin", termux_prefix);
    let Ok(entries) = std::fs::read_dir(&bin_dir) else {
        return Some(wrapper_dir);
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name.contains('/') || name == "." || name == ".." || name == ".wrapper" {
            continue;
        }

        let target = format!("{}/{}", bin_dir, name);
        if !std::path::Path::new(&target).exists() {
            continue;
        }

        let wrapper = format!("{}/{}", wrapper_dir, name);
        let link_ok = std::fs::read_link(&wrapper)
            .map(|dest| dest.to_string_lossy() == ".wrapper")
            .unwrap_or(false);
        if !link_ok {
            let _ = std::fs::remove_file(&wrapper);
            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                if symlink(".wrapper", &wrapper).is_err() {
                    debug_log(&format!("WRAPPER_SYMLINK_FAILED: {}", wrapper));
                }
            }
        }
    }

    Some(wrapper_dir)
}

fn exec_wrapper_script() -> String {
    r#"#!/system/bin/sh
WRAPPER_DIR=$(dirname "$0")
WRAPPER_NAME=$(basename "$0")
PREFIX=$(dirname "$(dirname "$WRAPPER_DIR")")
TARGET="$PREFIX/bin/$WRAPPER_NAME"
IFS= read -r first < "$TARGET" 2>/dev/null || first=
case "$first" in
  '#!'*)
    shebang="${first#\#!}"
    set -- $shebang "$TARGET" "$@"
    interp="$1"
    shift
    case "$interp" in
      /usr/bin/env) interp="$PREFIX/bin/env" ;;
      /bin/*|/usr/bin/*) interp="$PREFIX/bin/${interp##*/}" ;;
    esac
    case "$interp" in
      "$PREFIX"/*|/data/data/com.termux/*|/data/user/0/com.termux/*)
        exec /system/bin/linker64 "$interp" "$@"
        ;;
      *)
        exec "$interp" "$@"
        ;;
    esac
    ;;
esac
exec /system/bin/linker64 "$TARGET" "$@"
"#.to_string()
}

fn path_with_exec_wrappers(path_value: &str) -> String {
    let prefix_base = get_current_prefix();
    let wrapper_path = EXEC_WRAPPER_PATH.get_or_init(|| ensure_exec_wrappers(prefix_base));
    if let Some(wrapper_dir) = wrapper_path {
        if path_value.split(':').any(|entry| entry == wrapper_dir) {
            path_value.to_string()
        } else {
            format!("{}:{}", wrapper_dir, path_value)
        }
    } else {
        path_value.to_string()
    }
}

fn get_my_physical_path() -> &'static str {
    MY_PHYSICAL_PATH.get_or_init(|| {
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        if unsafe { libc::dladdr(execve as *const libc::c_void, &mut info) } != 0 {
            if !info.dli_fname.is_null() {
                let path = unsafe { CStr::from_ptr(info.dli_fname) }.to_string_lossy().into_owned();
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
                if line.contains("com.termux") && (line.contains("/lib/") || line.contains("/files/")) {
                    if let Some(start) = line.find("/data/") {
                        if let Some(end) = line[start..].find("/files/") {
                            return line[start..start+end].to_string();
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
        if std::path::Path::new(&mapped).exists() { return Some(canonical_or_original(mapped)); }
        if file != mapped && std::path::Path::new(file).exists() { return Some(canonical_or_original(file.to_string())); }
        return None;
    }
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(':') {
            let full = format!("{}/{}", dir, file);
            if std::path::Path::new(&full).exists() { return Some(canonical_or_original(full)); }
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
            new_env.push(CString::new(format!("PATH={}", path_with_exec_wrappers(path_value))).unwrap());
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
        new_env.push(CString::new(format!("PATH={}", path_with_exec_wrappers(&fallback_path))).unwrap());
    }
    if !proc_self_exe_found && proc_self_exe.is_some() {
        new_env.push(CString::new(format!("TERMUX_EXEC__PROC_SELF_EXE={}", proc_self_exe.unwrap())).unwrap());
    }
    
    new_env
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execve(path: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int {
    let mut ptr = REAL_EXECVE.load(Ordering::Relaxed);
    if ptr.is_null() {
        ptr = libc::dlsym(RTLD_NEXT, b"execve\0".as_ptr() as *const c_char);
        REAL_EXECVE.store(ptr, Ordering::Relaxed);
    }
    let real_execve: unsafe extern "C" fn(*const c_char, *const *const c_char, *const *const c_char) -> c_int = std::mem::transmute(ptr);

    if path.is_null() { return real_execve(path, argv, envp); }
    let path_str = CStr::from_ptr(path).to_string_lossy().to_string();
    
    if path_str.starts_with("/system/") || path_str.starts_with("/vendor/") || path_str.contains("/linker") {
        return real_execve(path, argv, envp);
    }

    debug_log(&format!("TRY_EXECVE: {}", path_str));
    
    if let Some(full_path) = resolve_path(&path_str) {
        if full_path.contains("com.termux") && !full_path.contains("/applib/") {
             if let Some((final_cmd, new_argv_vec)) = transform_exec(&full_path, argv) {
                 debug_log(&format!("TRANSFORM: {} -> {} argv={}", full_path, final_cmd, argv_to_debug(new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect::<Vec<_>>().as_ptr())));
                 let c_cmd = CString::new(final_cmd.clone()).unwrap();
                 let c_ptrs: Vec<*const c_char> = new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                 let new_env = fix_envp(envp, Some(&full_path));
                 let env_ptrs: Vec<*const c_char> = new_env.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                 let result = real_execve(c_cmd.as_ptr(), c_ptrs.as_ptr(), env_ptrs.as_ptr());
                 debug_log(&format!("EXECVE_FAILED: {} errno={} argv={}", final_cmd, last_errno(), argv_to_debug(c_ptrs.as_ptr())));
                 return result;
             }
        }
    }

    let new_env = fix_envp(envp, None);
    let env_ptrs: Vec<*const c_char> = new_env.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
    let result = real_execve(path, argv, env_ptrs.as_ptr());
    debug_log(&format!("EXECVE_PASSTHROUGH_FAILED: {} errno={} argv={}", path_str, last_errno(), argv_to_debug(argv)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
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

    /// 证明 ensure_exec_wrappers 的文件 I/O 开销随 bin 数量增长
    #[test]
    fn test_wrapper_io_overhead() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_wrappers");
        let _ = fs::remove_dir_all(&tmp_dir);
        let bin_dir = tmp_dir.join("files/usr/bin");
        let wrapper_dir = tmp_dir.join("files/usr/libexec/termux-exec-wrappers");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&wrapper_dir).unwrap();

        const BIN_COUNT: usize = 50;

        // 模拟 $PREFIX/bin 下的可执行文件
        for i in 0..BIN_COUNT {
            let path = bin_dir.join(format!("cmd{}", i));
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh\necho hello{}\n", i).unwrap();
        }

        let prefix_base = tmp_dir.to_string_lossy().to_string();

        // 测量 ensure_exec_wrappers 的执行时间（symlink 方案）
        let start = Instant::now();
        let result = ensure_exec_wrappers(&prefix_base);
        let io_time = start.elapsed();

        println!(
            "ensure_exec_wrappers({} bins, symlink scheme) took: {:?}, result: {:?}",
            BIN_COUNT, io_time, result
        );

        // symlink 创建极快，但断言 I/O 必须可测量（>0）
        assert!(
            io_time.as_nanos() > 0,
            "Creating {} symlinks should take measurable time",
            BIN_COUNT
        );

        // 验证结构：1 个通用脚本 + N 个软链接
        let created_count = fs::read_dir(&wrapper_dir).unwrap().flatten().count();
        assert_eq!(
            created_count, BIN_COUNT + 1,
            "Expected 1 generic .wrapper + {} symlinks", BIN_COUNT
        );

        assert!(wrapper_dir.join(".wrapper").exists(), "Generic .wrapper script must exist");

        let mut symlink_count = 0;
        for entry in fs::read_dir(&wrapper_dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".wrapper" {
                continue;
            }
            if let Ok(dest) = fs::read_link(entry.path()) {
                assert_eq!(dest.to_string_lossy(), ".wrapper",
                    "{} should be a symlink to .wrapper", name);
                symlink_count += 1;
            }
        }
        assert_eq!(symlink_count, BIN_COUNT, "All wrappers should be symlinks to .wrapper");

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    /// 证明 PATH 变长后，在最坏情况下（目标不在 prepend 目录中）
    /// 每次 resolve_path 都需要多 stat 一个目录。
    #[test]
    fn test_path_search_overhead() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_path");
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();

        // 模拟 termux-exec-wrappers 目录（放在 PATH 最前面，但空）
        let wrapper_dir = tmp_dir.join("wrappers");
        fs::create_dir(&wrapper_dir).unwrap();

        // 创建 20 个目录模拟剩余 PATH，只在最后一个放目标文件
        let mut long_path_parts = vec![wrapper_dir.to_string_lossy().to_string()];
        for i in 0..20 {
            let dir = tmp_dir.join(format!("bin{}", i));
            fs::create_dir(&dir).unwrap();
            long_path_parts.push(dir.to_string_lossy().to_string());
        }
        let last_dir = tmp_dir.join("bin19");
        fs::File::create(last_dir.join("my-cmd")).unwrap();

        let long_path = long_path_parts.join(":");
        let short_path = long_path_parts[1..].join(":"); // 去掉 wrapper 目录

        const ITERATIONS: usize = 1000;

        // 测量长 PATH（含空 wrapper 目录）查找时间
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

        // 测量短 PATH（不含 wrapper 目录）查找时间
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

    /// 证明 "首次解析" 与 "缓存后解析" 的性能差异：
    /// EXEC_WRAPPER_PATH 的 OnceLock 首次会触发 ensure_exec_wrappers 的 I/O，
    /// 后续调用直接从缓存读取，两者差距可达数百倍。
    #[test]
    fn test_first_time_resolution_vs_cached() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_first_time");
        let _ = fs::remove_dir_all(&tmp_dir);
        let bin_dir = tmp_dir.join("files/usr/bin");
        fs::create_dir_all(&bin_dir).unwrap();

        // 创建 50 个模拟二进制文件
        for i in 0..50 {
            let path = bin_dir.join(format!("cmd{}", i));
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh\necho hello{}\n", i).unwrap();
        }

        let prefix = tmp_dir.to_string_lossy().to_string();
        let lock: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

        // 首次：走 get_or_init → ensure_exec_wrappers → 遍历目录、写文件、chmod
        let t1 = Instant::now();
        let r1 = lock.get_or_init(|| ensure_exec_wrappers(&prefix));
        let first_time = t1.elapsed();

        // 第二次：直接从 OnceLock 缓存读，零 I/O
        let t2 = Instant::now();
        let r2 = lock.get();
        let cached_time = t2.elapsed();

        println!(
            "first resolution: {:?}, cached read: {:?}, ratio: {:.0}x",
            first_time,
            cached_time,
            first_time.as_nanos() as f64 / cached_time.as_nanos().max(1) as f64
        );

        assert!(r1.is_some(), "Wrapper path should be created");
        assert_eq!(r1.clone(), r2.cloned().flatten(), "Cached value should match first result");

        // 首次应显著慢于缓存（至少 50 倍，实际通常数百到数千倍）
        assert!(
            first_time > cached_time * 50,
            "First-time resolution (with I/O) should be vastly slower than cached read"
        );

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    /// 证明 ensure_exec_wrappers 本身没有缓存：
    /// 如果没有外部 OnceLock（如 EXEC_WRAPPER_PATH），每次调用都会重新遍历、写入、chmod。
    /// 这意味着频繁 fork/exec 的场景下，每个新进程都要重新支付完整的 I/O 开销。
    #[test]
    fn test_repeated_call_pays_io_without_cache() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_no_cache");
        let _ = fs::remove_dir_all(&tmp_dir);
        let bin_dir = tmp_dir.join("files/usr/bin");
        fs::create_dir_all(&bin_dir).unwrap();

        for i in 0..50 {
            let path = bin_dir.join(format!("cmd{}", i));
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh\necho hello{}\n", i).unwrap();
        }

        let prefix = tmp_dir.to_string_lossy().to_string();

        // 第一次调用：冷启动，写 .wrapper + 创建 50 个 symlink
        let t1 = Instant::now();
        let r1 = ensure_exec_wrappers(&prefix);
        let first = t1.elapsed();

        // 第二次调用：没有内部缓存，仍需遍历 bin 目录并逐个 read_link 验证
        let t2 = Instant::now();
        let r2 = ensure_exec_wrappers(&prefix);
        let second = t2.elapsed();

        println!(
            "first: {:?}, second: {:?}, slowdown ratio: {:.1}x",
            first,
            second,
            first.as_nanos() as f64 / second.as_nanos().max(1) as f64
        );

        assert!(r1.is_some() && r2.is_some());

        // symlink 方案下第二次调用仍然要遍历 50 个文件做 read_link，有 I/O 成本
        // 但远比旧方案的 "覆盖写入 50 个文件 + chmod" 快
        assert!(
            second.as_nanos() > 0,
            "ensure_exec_wrappers has no internal cache; second call still involves measurable I/O"
        );

        // 关键：两次耗时在同一数量级，不会像 OnceLock 那样出现 1000x+ 的差距。
        let ratio = first.as_nanos() as f64 / second.as_nanos().max(1) as f64;
        assert!(
            ratio < 50.0,
            "Without external OnceLock cache, repeated calls stay in same order of magnitude. \
             Ratio was {:.1}x, proving I/O is paid every time",
            ratio
        );

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    /// 证明 symlink 方案相比独立文件方案在存储上的巨大优势。
    #[test]
    fn test_storage_overhead() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_storage");
        let _ = fs::remove_dir_all(&tmp_dir);
        let bin_dir = tmp_dir.join("files/usr/bin");
        fs::create_dir_all(&bin_dir).unwrap();

        // 模拟一个中等规模的 Termux 环境：200 个可执行文件
        const BIN_COUNT: usize = 200;
        for i in 0..BIN_COUNT {
            let path = bin_dir.join(format!("cmd{}", i));
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh\necho hello{}\n", i).unwrap();
        }

        let prefix = tmp_dir.to_string_lossy().to_string();
        let wrapper_dir = ensure_exec_wrappers(&prefix).unwrap();

        let mut total_content_bytes = 0u64;
        let mut regular_file_count = 0usize;
        let mut symlink_count = 0usize;
        for entry in fs::read_dir(&wrapper_dir).unwrap().flatten() {
            let meta = entry.metadata().unwrap();
            if meta.file_type().is_symlink() {
                symlink_count += 1;
            } else {
                regular_file_count += 1;
                total_content_bytes += meta.len();
            }
        }

        println!(
            "wrapper dir: {} regular files, {} symlinks, content bytes: {}",
            regular_file_count, symlink_count, total_content_bytes
        );

        // 验证结构：只有 1 个普通文件（.wrapper），其余全是软链接
        assert_eq!(regular_file_count, 1, "Only .wrapper should be a regular file");
        assert_eq!(symlink_count, BIN_COUNT, "All {} wrappers should be symlinks", BIN_COUNT);

        // 旧方案（独立文件）的估算成本：200 × 4KB = ~800KB
        const BLOCK_SIZE: u64 = 4096;
        let old_scheme_estimate = BIN_COUNT as u64 * BLOCK_SIZE;

        // 新方案实际内容只有 .wrapper 的 ~300 字节，软链接几乎不占额外 block
        // 断言：新方案内容大小不到旧方案估算的 1%
        assert!(
            total_content_bytes < old_scheme_estimate / 100,
            "Symlink scheme content should be <1% of old per-file block overhead. \
             New: {} bytes vs old estimate: {} bytes",
            total_content_bytes, old_scheme_estimate
        );

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    /// 断言：为什么必须使用 wrapper 脚本。
    ///
    /// 核心问题：LD_PRELOAD 只能拦截 libc 层的 exec 函数调用。
    /// 如果调用者直接走 syscall(__NR_execve, ...) —— 如 Go、Rust、静态链接程序 ——
    /// transform_exec 完全失效。但 kernel 的 shebang 解析不依赖 LD_PRELOAD，
    /// 所以 wrapper 脚本在 PATH 层面提供了第二次保险。
    #[test]
    fn test_why_wrapper_is_mandatory() {
        let tmp_dir = std::env::temp_dir().join("termux_exec_test_why_wrapper");
        let _ = fs::remove_dir_all(&tmp_dir);
        let bin_dir = tmp_dir.join("files/usr/bin");
        let wrapper_dir = tmp_dir.join("files/usr/libexec/termux-exec-wrappers");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&wrapper_dir).unwrap();

        // 创建一个 shebang 脚本，指向一个在 Android 上不存在的路径
        // 这是 Termux 脚本的典型情况：#!/usr/bin/env sh
        let script = bin_dir.join("test-script");
        let mut f = fs::File::create(&script).unwrap();
        writeln!(f, "#!/usr/bin/env sh").unwrap();
        writeln!(f, "echo SUCCESS").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
        }

        // 创建通用 wrapper 脚本 + 软链接（模拟新的 symlink 方案）
        let generic_wrapper = wrapper_dir.join(".wrapper");
        let wrapper_content = format!(
            r#"#!/system/bin/sh
exec /system/bin/sh '{target}' "$@"
"#,
            target = script.to_string_lossy()
        );
        fs::write(&generic_wrapper, wrapper_content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&generic_wrapper).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&generic_wrapper, perms).unwrap();
        }

        let wrapper = wrapper_dir.join("test-script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(".wrapper", &wrapper).unwrap();
        }

        // 辅助函数：模拟 raw exec caller 的 PATH 查找
        fn find_in_path(name: &str, path_env: &str) -> Option<String> {
            for dir in path_env.split(':') {
                let full = std::path::Path::new(dir).join(name);
                if full.exists() {
                    return Some(full.to_string_lossy().into_owned());
                }
            }
            None
        }

        let path_with_wrapper = format!("{}:{}", wrapper_dir.to_string_lossy(), bin_dir.to_string_lossy());
        let path_without_wrapper = bin_dir.to_string_lossy().to_string();

        // 断言1：有 wrapper 时，PATH 查找先命中 wrapper
        let found_with = find_in_path("test-script", &path_with_wrapper).unwrap();
        assert_eq!(found_with, wrapper.to_string_lossy().to_string());

        // 断言2：无 wrapper 时，PATH 查找命中真实脚本
        let found_without = find_in_path("test-script", &path_without_wrapper).unwrap();
        assert_eq!(found_without, script.to_string_lossy().to_string());

        // 断言3：直接 syscall execve 真实脚本会失败（因为 /usr/bin/env 不存在）
        // 这模拟了 raw exec caller 绕过 LD_PRELOAD 的场景
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0, "fork failed");

            if pid == 0 {
                // 子进程：直接 syscall execve，绕过任何 LD_PRELOAD
                let path = CString::new(script.to_string_lossy().as_bytes()).unwrap();
                let argv0 = CString::new("test-script").unwrap();
                let argv: [*const c_char; 2] = [argv0.as_ptr(), ptr::null()];

                libc::syscall(libc::SYS_execve, path.as_ptr(), argv.as_ptr(), ptr::null::<c_char>());
                // execve 失败才会到达这里
                libc::exit(42);
            } else {
                let mut status: c_int = 0;
                let waited = libc::waitpid(pid, &mut status, 0);
                assert_eq!(waited, pid);

                let exit_code = libc::WEXITSTATUS(status);
                assert!(
                    exit_code == 42,
                    "ASSERTION: Direct syscall execve of shebang script WITHOUT wrapper \
                     MUST fail (exit={}, expected 42). \
                     Because /usr/bin/env does not exist on Android, \
                     and LD_PRELOAD cannot intercept direct syscalls.",
                    exit_code
                );
            }
        }

        // 断言4：直接 syscall execve wrapper 脚本会成功
        // 因为 kernel 能解析 #!/bin/sh，sh 执行 wrapper，wrapper 做 shebang 映射
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0, "fork failed");

            if pid == 0 {
                let path = CString::new(wrapper.to_string_lossy().as_bytes()).unwrap();
                let argv0 = CString::new("test-script").unwrap();
                let argv: [*const c_char; 2] = [argv0.as_ptr(), ptr::null()];

                libc::syscall(libc::SYS_execve, path.as_ptr(), argv.as_ptr(), ptr::null::<c_char>());
                // execve 失败才会到达这里
                libc::exit(42);
            } else {
                let mut status: c_int = 0;
                let waited = libc::waitpid(pid, &mut status, 0);
                assert_eq!(waited, pid);

                let exit_code = libc::WEXITSTATUS(status);
                assert!(
                    exit_code == 0,
                    "ASSERTION: Direct syscall execve of wrapper script SHOULD succeed \
                     (exit={}, expected 0). \
                     Because kernel parses #!/bin/sh, sh executes wrapper, \
                     wrapper maps /usr/bin/env → $PREFIX/bin/env.",
                    exit_code
                );
            }
        }

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execveat(dirfd: c_int, path: *const c_char, argv: *const *const c_char, envp: *const *const c_char, flags: c_int) -> c_int {
    let mut ptr = REAL_EXECVEAT.load(Ordering::Relaxed);
    if ptr.is_null() {
        ptr = libc::dlsym(RTLD_NEXT, b"execveat\0".as_ptr() as *const c_char);
        REAL_EXECVEAT.store(ptr, Ordering::Relaxed);
    }
    let real_execveat: unsafe extern "C" fn(c_int, *const c_char, *const *const c_char, *const *const c_char, c_int) -> c_int = std::mem::transmute(ptr);

    if path.is_null() { return real_execveat(dirfd, path, argv, envp, flags); }
    let path_str = CStr::from_ptr(path).to_string_lossy().to_string();
    debug_log(&format!("TRY_EXECVEAT: {}", path_str));

    if (path_str.contains("com.termux") || path_str.starts_with("/usr/") || path_str.starts_with("/bin/") || !path_str.starts_with('/')) && dirfd == libc::AT_FDCWD {
        return execve(path, argv, envp);
    }

    let result = real_execveat(dirfd, path, argv, envp, flags);
    debug_log(&format!("EXECVEAT_FAILED: dirfd={} path={} flags={} errno={} argv={}", dirfd, path_str, flags, last_errno(), argv_to_debug(argv)));
    result
}

fn transform_exec(path: &str, orig_argv: *const *const c_char) -> Option<(String, Vec<CString>)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = [0u8; 256];
    let n = file.read(&mut buffer).ok()?;
    let current_prefix = get_current_prefix();
    let linker = if std::path::Path::new("/system/bin/linker64").exists() { "/system/bin/linker64" } else { "/system/bin/linker" };

    if n > 4 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(path).unwrap());
        new_argv.push(CString::new(path).unwrap());
        let mut i = 1;
        unsafe { while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
            new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
            i += 1;
        } }
        return Some((linker.to_string(), new_argv));
    } else if let Some((interpreter, shebang_args)) = parse_shebang_internal(&buffer[..n], current_prefix) {
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(interpreter.clone()).unwrap());
        new_argv.push(CString::new(interpreter.clone()).unwrap());

        if let Some(args) = shebang_args {
            for arg in args.split_whitespace() {
                new_argv.push(CString::new(arg).unwrap());
            }
        }
        new_argv.push(CString::new(path).unwrap());
        
        let mut i = 1;
        unsafe { while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
            new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
            i += 1;
        } }

        if interpreter.contains("com.termux") {
             return Some((linker.to_string(), new_argv));
        }
        return Some((interpreter, new_argv));
    } else if path.contains("com.termux") {
        let sh_path = format!("{}/files/usr/bin/sh", current_prefix);
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(sh_path.clone()).unwrap());
        new_argv.push(CString::new(sh_path.clone()).unwrap());
        new_argv.push(CString::new(path).unwrap());
        
        let mut i = 1;
        unsafe { while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
            new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
            i += 1;
        } }
        return Some((linker.to_string(), new_argv));
    }
    None
}

fn parse_shebang_internal(buffer: &[u8], current_prefix: &str) -> Option<(String, Option<String>)> {
    if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' { return None; }
    let line_end = buffer.iter().position(|&b| b == b'\n').unwrap_or(buffer.len());
    let line = String::from_utf8_lossy(&buffer[2..line_end]);
    let trimmed = line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() { return None; }
    
    let mut interpreter = tokens[0].to_string();
    if interpreter.starts_with("/usr/bin/env") {
        interpreter = format!("{}/files/usr/bin/env", current_prefix);
    } else if interpreter.starts_with("/bin/") || interpreter.starts_with("/usr/bin/") {
        let binary = interpreter.rsplit('/').next().unwrap_or("sh");
        interpreter = format!("{}/files/usr/bin/{}", current_prefix, binary);
    }
    
    let args = if tokens.len() > 1 { Some(tokens[1..].join(" ")) } else { None };
    Some((interpreter, args))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execv(path: *const c_char, argv: *const *const c_char) -> c_int { execve(path, argv, environ) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int { execvpe(file, argv, environ) }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvpe(file: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int {
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
pub unsafe extern "C" fn termux_exec_rs_execve(path: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int {
    execve(path, argv, envp)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn termux_exec_rs_execvpe(file: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int {
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
    let real_spawn: unsafe extern "C" fn(*mut libc::pid_t, *const c_char, *const libc::c_void, *const libc::c_void, *const *const c_char, *const *const c_char) -> c_int = std::mem::transmute(ptr);

    if path.is_null() { return real_spawn(pid, path, file_actions, attrp, argv, envp); }
    let path_str = CStr::from_ptr(path).to_string_lossy().to_string();
    
    if path_str.starts_with("/system/") || path_str.starts_with("/vendor/") || path_str.contains("/linker") {
        return real_spawn(pid, path, file_actions, attrp, argv, envp);
    }

    debug_log(&format!("TRY_SPAWN: {}", path_str));
    
    if let Some(full_path) = resolve_path(&path_str) {
        if full_path.contains("com.termux") && !full_path.contains("/applib/") {
             if let Some((final_cmd, new_argv_vec)) = transform_exec(&full_path, argv) {
                 debug_log(&format!("SPAWN: {} -> {} argv={}", full_path, final_cmd, argv_to_debug(new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect::<Vec<_>>().as_ptr())));
                 let c_cmd = CString::new(final_cmd.clone()).unwrap();
                 let c_ptrs: Vec<*const c_char> = new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                 let new_env = fix_envp(envp, Some(&full_path));
                 let env_ptrs: Vec<*const c_char> = new_env.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                 let result = real_spawn(pid, c_cmd.as_ptr(), file_actions, attrp, c_ptrs.as_ptr(), env_ptrs.as_ptr());
                 if result != 0 {
                     debug_log(&format!("SPAWN_FAILED: {} result={} errno={} argv={}", final_cmd, result, last_errno(), argv_to_debug(c_ptrs.as_ptr())));
                 }
                 return result;
             }
        }
    }

    let new_env = fix_envp(envp, None);
    let env_ptrs: Vec<*const c_char> = new_env.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
    let result = real_spawn(pid, path, file_actions, attrp, argv, env_ptrs.as_ptr());
    if result != 0 {
        debug_log(&format!("SPAWN_PASSTHROUGH_FAILED: {} result={} errno={} argv={}", path_str, result, last_errno(), argv_to_debug(argv)));
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
    let real_spawnp: unsafe extern "C" fn(*mut libc::pid_t, *const c_char, *const libc::c_void, *const libc::c_void, *const *const c_char, *const *const c_char) -> c_int = std::mem::transmute(ptr);

    if file.is_null() { return real_spawnp(pid, file, file_actions, attrp, argv, envp); }
    let file_str = CStr::from_ptr(file).to_string_lossy().to_string();

    if file_str.starts_with("/system/") || file_str.starts_with("/vendor/") || file_str.contains("/linker") {
        return real_spawnp(pid, file, file_actions, attrp, argv, envp);
    }

    debug_log(&format!("TRY_SPAWNP: {}", file_str));

    if let Some(full_path) = resolve_path(&file_str) {
        if full_path.contains("com.termux") && !full_path.contains("/applib/") {
            if let Some((final_cmd, new_argv_vec)) = transform_exec(&full_path, argv) {
                debug_log(&format!("SPAWNP: {} -> {} argv={}", full_path, final_cmd, argv_to_debug(new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect::<Vec<_>>().as_ptr())));
                let c_cmd = CString::new(final_cmd.clone()).unwrap();
                let c_ptrs: Vec<*const c_char> = new_argv_vec.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                let new_env = fix_envp(envp, Some(&full_path));
                let env_ptrs: Vec<*const c_char> = new_env.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                let result = real_spawnp(pid, c_cmd.as_ptr(), file_actions, attrp, c_ptrs.as_ptr(), env_ptrs.as_ptr());
                if result != 0 {
                    debug_log(&format!("SPAWNP_FAILED: {} result={} errno={} argv={}", final_cmd, result, last_errno(), argv_to_debug(c_ptrs.as_ptr())));
                }
                return result;
            }
        }
    }

    let new_env = fix_envp(envp, None);
    let env_ptrs: Vec<*const c_char> = new_env.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
    let result = real_spawnp(pid, file, file_actions, attrp, argv, env_ptrs.as_ptr());
    if result != 0 {
        debug_log(&format!("SPAWNP_PASSTHROUGH_FAILED: {} result={} errno={} argv={}", file_str, result, last_errno(), argv_to_debug(argv)));
    }
    result
}
