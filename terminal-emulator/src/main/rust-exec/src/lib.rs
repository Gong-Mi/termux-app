use std::ffi::{CStr, CString};
use std::io::Read;
use std::os::raw::{c_char, c_int};
use std::ptr;
#[cfg(target_os = "android")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "android")]
use std::sync::atomic::Ordering;

#[cfg(target_os = "android")]
static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    static mut environ: *mut *mut c_char;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".init_array")]
pub static LOAD_HOOK: unsafe extern "C" fn() = {
    unsafe extern "C" fn init() {
        unsafe { init_logging() };
        ensure_ld_preload_is_exported();
        ensure_termux_prefix_is_exported();
        log::info!("execve preload hooks active");
    }
    init
};

#[cfg(target_os = "android")]
unsafe fn init_logging() {
    if LOGGER_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("TermuxExec"),
    );
}

#[cfg(not(target_os = "android"))]
unsafe fn init_logging() {
    // noop on non-Android targets
}

fn ensure_ld_preload_is_exported() {
    let Some(path) = current_library_path() else {
        log::warn!("unable to restore LD_PRELOAD: libtermux-exec.so not found in maps");
        return;
    };

    if let Ok(existing) = std::env::var("LD_PRELOAD") {
        if !existing.is_empty() {
            if existing == path {
                // Already points to the same library; nothing to do.
                return;
            }
            if std::path::Path::new(&existing).exists() {
                log::warn!(
                    "LD_PRELOAD points to a different library ({} -> refreshing to {})",
                    existing, path
                );
            } else {
                log::warn!("LD_PRELOAD stale path detected: {} -> refreshing to {}", existing, path);
            }
        }
    }

    unsafe {
        if let Ok(key) = CString::new("LD_PRELOAD") {
            if let Ok(value) = CString::new(path.as_str()) {
                libc::setenv(key.as_ptr(), value.as_ptr(), 1);
            }
        }
    }
    log::info!("restored LD_PRELOAD to {}", path);
}

/// If PREFIX is missing from the environment, infer it from the LD_PRELOAD path
/// and export it. This ensures child processes always have a valid Termux prefix,
/// even when launched from a clean environment (e.g. linker64 → target ELF).
fn ensure_termux_prefix_is_exported() {
    if std::env::var("PREFIX").is_ok() {
        return;
    }

    if let Some(prefix) = prefix_from_ld_preload_path() {
        unsafe {
            if let Ok(key) = CString::new("PREFIX") {
                if let Ok(value) = CString::new(prefix.as_str()) {
                    libc::setenv(key.as_ptr(), value.as_ptr(), 1);
                }
            }
        }
        log::info!("restored PREFIX to {}", prefix);
    }
}

/// Infer Termux prefix from the LD_PRELOAD path.
/// LD_PRELOAD is usually .../files/usr/lib/libtermux-exec.so or .../applib/libtermux-exec.so.
fn prefix_from_ld_preload_path() -> Option<String> {
    let ld_preload = selected_ld_preload_path()?;
    let path = std::path::Path::new(&ld_preload);

    // Case 1: .../files/usr/lib/libtermux-exec.so → prefix is .../files/usr
    if let Some(parent) = path.parent() {
        if let Some(grandparent) = parent.parent() {
            let candidate = grandparent.to_string_lossy().to_string();
            if candidate.ends_with("/usr") || candidate.ends_with("\\usr") {
                return Some(candidate);
            }
        }
    }

    // Case 2: .../applib/libtermux-exec.so → prefix is .../files/usr
    if let Some(parent) = path.parent() {
        if let Some(grandparent) = parent.parent() {
            let candidate = grandparent.to_string_lossy().to_string();
            let prefix_candidate = format!("{}/usr", candidate);
            if std::path::Path::new(&prefix_candidate).exists() {
                return Some(prefix_candidate);
            }
        }
    }

    None
}

fn selected_ld_preload_path() -> Option<String> {
    current_library_path().or_else(|| {
        std::env::var("LD_PRELOAD")
            .ok()
            .filter(|value| !value.is_empty())
    })
}

fn current_library_path() -> Option<String> {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return None;
    };

    for line in maps.lines() {
        let Some(path_start) = line.find('/') else {
            continue;
        };
        let path = &line[path_start..];
        if !path.ends_with("libtermux-exec.so") {
            continue;
        }

        return Some(path.to_string());
    }

    None
}

fn push_env_if_missing(entries: &mut Vec<String>, key: &str, value: String) {
    let prefix = format!("{}=", key);
    if !entries.iter().any(|entry| entry.starts_with(&prefix)) {
        entries.push(format!("{}={}", key, value));
    }
}

fn ensure_termux_core_env(entries: &mut Vec<String>) {
    let prefix = get_termux_prefix();
    let home = std::env::var("HOME").unwrap_or_else(|_| {
        let path = std::path::Path::new(&prefix);
        if let Some(parent) = path.parent() {
            parent.join("home").to_string_lossy().to_string()
        } else {
            "/data/data/com.termux/files/home".to_string()
        }
    });

    // Force key environment variables to ensure consistency
    let mut prefix_set = false;
    let mut home_set = false;
    let mut path_idx = None;

    for (i, entry) in entries.iter_mut().enumerate() {
        if entry.starts_with("PREFIX=") {
            *entry = format!("PREFIX={}", prefix);
            prefix_set = true;
        } else if entry.starts_with("HOME=") {
            *entry = format!("HOME={}", home);
            home_set = true;
        } else if entry.starts_with("PATH=") {
            path_idx = Some(i);
        }
    }

    if !prefix_set { entries.push(format!("PREFIX={}", prefix)); }
    if !home_set { entries.push(format!("HOME={}", home)); }

    // Prepend Termux bin to PATH
    let termux_bin = format!("{}/bin", prefix);
    if let Some(idx) = path_idx {
        let current_path = &entries[idx][5..];
        if !current_path.contains(&termux_bin) {
            entries[idx] = format!("PATH={}:{}", termux_bin, current_path);
        }
    } else {
        entries.push(format!("PATH={}:/system/bin", termux_bin));
    }

    push_env_if_missing(entries, "TMPDIR", format!("{}/tmp", prefix));
    push_env_if_missing(entries, "TERM", "xterm-256color".to_string());
    push_env_if_missing(entries, "SHELL", format!("{}/bin/bash", prefix));
}

fn env_entries_with_termux_defaults<I, S>(entries: I, ld_preload: &str) -> Vec<CString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut raw_entries = Vec::new();
    let mut has_ld_preload = false;

    for entry in entries {
        let entry = entry.as_ref();
        if entry.starts_with("LD_PRELOAD=") {
            has_ld_preload = true;
            raw_entries.push(format!("LD_PRELOAD={}", ld_preload));
        } else {
            raw_entries.push(entry.to_string());
        }
    }

    if !has_ld_preload {
        raw_entries.push(format!("LD_PRELOAD={}", ld_preload));
    }

    ensure_termux_core_env(&mut raw_entries);

    raw_entries
        .into_iter()
        .map(|entry| CString::new(entry.as_bytes()).unwrap_or_else(|_| CString::new("").unwrap()))
        .collect()
}

fn envp_with_ld_preload(envp: *const *const c_char, original_path: &str) -> (Vec<CString>, Vec<*const c_char>) {
    let Some(ld_preload) = selected_ld_preload_path() else {
        return (Vec::new(), vec![ptr::null()]);
    };

    let mut raw_entries = Vec::new();

    unsafe {
        let mut i = 0;
        while !envp.is_null() && !(*envp.offset(i)).is_null() {
            let entry = CStr::from_ptr(*envp.offset(i)).to_string_lossy();
            raw_entries.push(entry.to_string());
            i += 1;
        }
    }

    // Always overwrite TERMUX_ORIGINAL_EXE_PATH so child processes know their own
    // true executable path, not the inherited one from the parent shell.
    let key = "TERMUX_ORIGINAL_EXE_PATH";
    let prefix = format!("{}=", key);
    let new_value = format!("{}={}", key, original_path);
    let mut replaced = false;
    for entry in &mut raw_entries {
        if entry.starts_with(&prefix) {
            *entry = new_value.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        raw_entries.push(new_value);
    }

    let entries =
        env_entries_with_termux_defaults(raw_entries.iter().map(String::as_str), &ld_preload);

    let mut ptrs: Vec<*const c_char> = entries.iter().map(|entry| entry.as_ptr()).collect();
    ptrs.push(ptr::null());
    (entries, ptrs)
}

unsafe fn execve_common(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    if path.is_null() {
        return -1;
    }

    let path_str = unsafe { CStr::from_ptr(path) }.to_string_lossy().to_string();
    let (_env_entries, env_ptrs) = envp_with_ld_preload(envp, &path_str);
    let final_envp = if env_ptrs.len() > 1 { env_ptrs.as_ptr() } else { envp };

    let is_linker = path_str.ends_with("/linker64") || path_str.ends_with("/linker");
    if path_str.starts_with("/system/") || path_str.starts_with("/vendor/") || is_linker {
        // Special case: If a wrapped process (e.g. Node.js) tries to relaunch itself using process.execPath
        // which has been corrupted to /system/bin/linker64, we need to fix the command line.
        // Symptoms: execve("/system/bin/linker64", ["/system/bin/linker64", "--max-old-space-size=...", ...])
        let is_flag_start = unsafe {
            !argv.is_null() && !(*argv.offset(1)).is_null() && 
            (*(*argv.offset(1)) as u8) == b'-'
        };

        if is_linker && is_flag_start {
            if let Ok(original_exe) = std::env::var("TERMUX_ORIGINAL_EXE_PATH") {
                log::info!("execve hook: detected linker relaunch of \"{}\", redirecting", original_exe);
                if let Some((final_path, new_argv)) = transform_exec(&original_exe, argv, 0) {
                    let Ok(c_path) = CString::new(final_path) else { return -1; };
                    let c_argv_ptrs: Vec<*const c_char> = new_argv.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                    return unsafe {
                        libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, c_path.as_ptr(), c_argv_ptrs.as_ptr(), final_envp, 0) as c_int
                    };
                }
            }
        }

        log::debug!("execve hook: passing through system binary \"{}\"", path_str);
        return unsafe {
            libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, path, argv, final_envp, 0) as c_int
        };
    }

    if let Some((final_path, new_argv)) = transform_exec(&path_str, argv, 0) {
        let argv_display: Vec<String> = new_argv
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        log::info!(
            "execve hook: transformed \"{}\" -> \"{}\" with argv {:?}",
            path_str,
            final_path,
            argv_display
        );

        let Ok(c_path) = CString::new(final_path) else {
            return -1;
        };
        let c_argv_ptrs: Vec<*const c_char> = new_argv
            .iter()
            .map(|s| s.as_ptr())
            .chain(std::iter::once(ptr::null()))
            .collect();

        return unsafe {
            libc::syscall(
                libc::SYS_execveat,
                libc::AT_FDCWD,
                c_path.as_ptr(),
                c_argv_ptrs.as_ptr(),
                final_envp,
                0,
            ) as c_int
        };
    }

    log::debug!("execve hook: no transform for \"{}\", using execveat", path_str);
    unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, path, argv, final_envp, 0) as c_int }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execve(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    unsafe { execve_common(path, argv, envp) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvpe(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    unsafe { execve_common(path, argv, envp) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvp(path: *const c_char, argv: *const *const c_char) -> c_int {
    let envp = unsafe { environ as *const *const c_char };
    unsafe { execve_common(path, argv, envp) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execveat(
    _dirfd: c_int,
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
    _flags: c_int,
) -> c_int {
    if path.is_null() {
        log::debug!("execveat hook: null path");
        return -1;
    }

    let path_str = unsafe { CStr::from_ptr(path) }.to_string_lossy().to_string();
    let argv_dump = debug_argv(argv);
    log::info!(
        "execveat hook: path=\"{}\" argv={:?}",
        path_str,
        argv_dump
    );

    unsafe { execve_common(path, argv, envp) }
}

fn debug_argv(argv: *const *const c_char) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut i = 0;
        while !argv.is_null() && !(*argv.offset(i)).is_null() && i < 32 {
            out.push(CStr::from_ptr(*argv.offset(i)).to_string_lossy().to_string());
            i += 1;
        }
    }
    out
}

fn transform_exec(path: &str, orig_argv: *const *const c_char, depth: u32) -> Option<(String, Vec<CString>)> {
    if depth > 3 {
        log::warn!("transform_exec: shebang chain too deep for \"{}\", giving up", path);
        return None;
    }

    let path = resolve_exec_path(path)?;
    let mut file = std::fs::File::open(&path).ok()?;
    let mut buffer = [0u8; 256];
    let n = file.read(&mut buffer).ok()?;
    
    let linker = if std::path::Path::new("/system/bin/linker64").exists() {
        "/system/bin/linker64"
    } else {
        "/system/bin/linker"
    };

    if n > 17 && buffer[0] == 0x7F && buffer[1] == b'E' && buffer[2] == b'L' && buffer[3] == b'F' {
        let e_type = u16::from_le_bytes([buffer[16], buffer[17]]);
        if e_type != 3 {
            log::debug!(
                "transform_exec: \"{}\" is ELF with e_type {} (not ET_DYN), skipping linker wrapper",
                path,
                e_type
            );
            return None;
        }

        log::debug!("transform_exec: \"{}\" is PIE ELF, prepending linker", path);
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(path.clone()).unwrap());
        let mut i = 1;
        unsafe {
            while !orig_argv.is_null() && !(*orig_argv.offset(i)).is_null() {
                new_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
                i += 1;
            }
        }
        return Some((linker.to_string(), new_argv));
    } else if let Some((interpreter, shebang_args)) = parse_shebang(&buffer[..n]) {
        log::debug!("transform_exec: \"{}\" has shebang interpreter=\"{}\" args={:?}", path, interpreter, shebang_args);
        // Shebang script — resolve interpreter path and check if it is ALSO a script
        let resolved_interp = map_path(&interpreter);
        log::debug!("transform_exec: mapped interpreter \"{}\" -> \"{}\"", interpreter, resolved_interp);

        // If the interpreter itself is a shebang script, recurse to find the real ELF loader
        if let Some((real_linker, mut real_argv)) = transform_exec(&resolved_interp, orig_argv, depth + 1) {
            // Insert the shebang arguments and then the script path into the recursive result
            // real_argv layout: [linker, interp, ...orig_args...]
            // we need: [linker, interp, [shebang_args], script_path, ...orig_args...]
            let mut insert_idx = 2;
            if let Some(args) = shebang_args {
                for arg in args.split_whitespace() {
                    real_argv.insert(insert_idx, CString::new(arg).unwrap());
                    insert_idx += 1;
                }
            }

            if real_argv.len() >= insert_idx {
                real_argv.insert(insert_idx, CString::new(path.clone()).unwrap());
            } else {
                real_argv.push(CString::new(path.clone()).unwrap());
            }
            return Some((real_linker, real_argv));
        }

        // Interpreter is ELF (or direct exec). Build argv for linker -> interp -> script
        let mut new_argv = Vec::new();
        new_argv.push(CString::new(linker).unwrap());
        new_argv.push(CString::new(resolved_interp.clone()).unwrap());
        
        if let Some(args) = shebang_args {
            for arg in args.split_whitespace() {
                new_argv.push(CString::new(arg).unwrap());
            }
        }
        new_argv.push(CString::new(path.clone()).unwrap());
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

fn resolve_exec_path(path: &str) -> Option<String> {
    if path.contains('/') {
        return Some(map_path(path));
    }

    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = format!("{}/{}", dir, path);
            if std::path::Path::new(&candidate).exists() {
                return Some(map_path(&candidate));
            }
        }
    }

    let prefix_candidate = format!("{}/bin/{}", get_termux_prefix(), path);
    if std::path::Path::new(&prefix_candidate).exists() {
        return Some(prefix_candidate);
    }

    None
}

fn parse_shebang(buffer: &[u8]) -> Option<(String, Option<String>)> {
    if buffer.len() < 2 || buffer[0] != b'#' || buffer[1] != b'!' {
        return None;
    }
    let line_end = buffer.iter().position(|&b| b == b'\n').unwrap_or(buffer.len());
    let line = String::from_utf8_lossy(&buffer[2..line_end]);
    let trimmed = line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() { return None; }

    let interpreter = tokens[0].to_string();
    let args = if tokens.len() > 1 { Some(tokens[1..].join(" ")) } else { None };
    Some((interpreter, args))
}

fn get_termux_prefix() -> String {
    if let Ok(prefix) = std::env::var("PREFIX") {
        return prefix;
    }

    if let Some(prefix) = prefix_from_ld_preload_path() {
        return prefix;
    }

    "/data/data/com.termux/files/usr".to_string()
}

fn map_path(path: &str) -> String {
    let prefix = get_termux_prefix();
    
    // Handle standard short paths
    if path.starts_with("/usr/bin/") {
        return format!("{}/bin/{}", prefix, &path[9..]);
    }
    if path.starts_with("/bin/") {
        return format!("{}/bin/{}", prefix, &path[5..]);
    }

    // Handle legacy and multi-user absolute paths
    // Example: /data/data/com.termux/files/usr/bin/sh -> [dynamic prefix]/bin/sh
    // Example: /data/user/0/com.termux/files/usr/bin/sh -> [dynamic prefix]/bin/sh
    let package_files_usr = "/com.termux/files/usr/";
    if let Some(idx) = path.find(package_files_usr) {
        let suffix = &path[idx + package_files_usr.len()..];
        let mapped = format!("{}/{}", prefix, suffix);
        log::debug!("map_path: matched package path, mapping to \"{}\"", mapped);
        return mapped;
    }

    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    fn cstring_argv_from_slice(args: &[&str]) -> Vec<CString> {
        args.iter().map(|arg| CString::new(*arg).unwrap()).collect()
    }

    fn cstring_ptrs(args: &[CString]) -> Vec<*const c_char> {
        args.iter()
            .map(|arg| arg.as_ptr())
            .chain(std::iter::once(ptr::null()))
            .collect()
    }

    fn create_test_elf(path: &std::path::Path) {
        let mut file = std::fs::File::create(path).unwrap();
        // Use a 64-byte buffer to ensure n > 17 and enough room for ELF header
        let mut header = [0u8; 64];
        header[0..4].copy_from_slice(b"\x7fELF");
        header[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        file.write_all(&header).unwrap();
    }

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "termux-exec-rs-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn argv_strings(args: Vec<CString>) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect()
    }

    fn transform_with_argv(path: &Path, argv: &[&str]) -> (String, Vec<String>) {
        let argv = cstring_argv_from_slice(argv);
        let ptrs = cstring_ptrs(&argv);
        let (exec_path, exec_argv) =
            transform_exec(path.to_str().unwrap(), ptrs.as_ptr(), 0).unwrap();
        (exec_path, argv_strings(exec_argv))
    }

    #[test]
    fn test_parse_shebang_simple() {
        let content = b"#!/usr/bin/python\nprint('hello')";
        let (interp, args) = parse_shebang(content).unwrap();
        assert_eq!(interp, "/usr/bin/python");
        assert_eq!(args, None);
    }

    #[test]
    fn test_parse_shebang_with_args() {
        let content = b"#!/usr/bin/env python -u\n...";
        let (interp, args) = parse_shebang(content).unwrap();
        assert_eq!(interp, "/usr/bin/env");
        assert_eq!(args, Some("python -u".to_string()));
    }

    #[test]
    fn test_map_path_usr_bin() {
        let prefix = get_termux_prefix();
        assert_eq!(
            map_path("/usr/bin/ls"),
            format!("{}/bin/ls", prefix)
        );
    }

    #[test]
    fn test_map_path_bin() {
        let prefix = get_termux_prefix();
        assert_eq!(
            map_path("/bin/sh"),
            format!("{}/bin/sh", prefix)
        );
    }

    #[test]
    fn test_map_path_untouched() {
        assert_eq!(
            map_path("/system/bin/linker64"),
            "/system/bin/linker64"
        );
    }

    #[test]
    fn test_transform_relative_coreutils_applet_symlink() {
        let root = test_dir("coreutils-applet");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let coreutils = bin.join("coreutils");
        let id = bin.join("id");
        create_test_elf(&coreutils);
        symlink("coreutils", &id).unwrap();

        let (exec_path, argv) = transform_with_argv(&id, &[id.to_str().unwrap(), "-u"]);

        assert!(exec_path.ends_with("linker") || exec_path.ends_with("linker64"));
        assert_eq!(argv[0], exec_path);
        assert_eq!(argv[1], id.to_string_lossy());
        // argv[2] is argv[0] of the executed process, which is 'id'
        assert_eq!(argv[2], id.to_string_lossy());
        assert_eq!(argv[3], "-u");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_transform_apt_https_method_symlink() {
        let root = test_dir("apt-https-method");
        let methods = root.join("usr/lib/apt/methods");
        std::fs::create_dir_all(&methods).unwrap();
        let http = methods.join("http");
        let https = methods.join("https");
        create_test_elf(&http);
        symlink("http", &https).unwrap();

        let (exec_path, argv) = transform_with_argv(&https, &[https.to_str().unwrap()]);

        assert!(exec_path.ends_with("linker") || exec_path.ends_with("linker64"));
        assert_eq!(argv[0], exec_path);
        assert_eq!(argv[1], https.to_string_lossy());
        assert_eq!(argv[2], https.to_string_lossy());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_transform_shebang_adds_script_after_interpreter() {
        let root = test_dir("shebang");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let sh = bin.join("sh");
        let script = bin.join("script");
        create_test_elf(&sh);
        std::fs::write(&script, format!("#!{}\n", sh.to_string_lossy())).unwrap();

        let (exec_path, argv) = transform_with_argv(&script, &[script.to_str().unwrap(), "arg1"]);

        assert!(exec_path.ends_with("linker") || exec_path.ends_with("linker64"));
        assert_eq!(argv[0], exec_path);
        assert_eq!(argv[1], sh.to_string_lossy());
        assert_eq!(argv[2], script.to_string_lossy());
        assert_eq!(argv[3], script.to_string_lossy());
        assert_eq!(argv[4], "arg1");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_env_entries_add_ld_preload_when_missing() {
        let entries =
            env_entries_with_termux_defaults(["PATH=/bin", "TERM=xterm"], "/app/libtermux-exec.so");
        let strings = argv_strings(entries);

        let prefix = get_termux_prefix();
        assert!(strings.contains(&format!("PATH={}/bin:/bin", prefix)));
        assert!(strings.contains(&"TERM=xterm".to_string()));
        assert!(strings.contains(&"LD_PRELOAD=/app/libtermux-exec.so".to_string()));
        assert!(strings.iter().any(|entry| entry.starts_with("PREFIX=")));
        assert!(strings.iter().any(|entry| entry.starts_with("HOME=")));
        assert!(strings.iter().any(|entry| entry.starts_with("TMPDIR=")));
    }

    #[test]
    fn test_env_entries_replace_existing_ld_preload() {
        let entries = env_entries_with_termux_defaults(
            ["PATH=/bin", "LD_PRELOAD=/old/lib.so"],
            "/app/libtermux-exec.so",
        );
        let strings = argv_strings(entries);

        let prefix = get_termux_prefix();
        assert!(strings.contains(&format!("PATH={}/bin:/bin", prefix)));
        assert!(strings.contains(&"LD_PRELOAD=/app/libtermux-exec.so".to_string()));
        assert!(!strings.contains(&"LD_PRELOAD=/old/lib.so".to_string()));
    }
}
