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
unsafe fn init_logging() {}

fn ensure_ld_preload_is_exported() {
    let Some(path) = current_library_path() else { return; };
    if let Ok(existing) = std::env::var("LD_PRELOAD") {
        if existing == path { return; }
    }
    unsafe {
        if let Ok(key) = CString::new("LD_PRELOAD") {
            if let Ok(value) = CString::new(path.as_str()) {
                libc::setenv(key.as_ptr(), value.as_ptr(), 1);
            }
        }
    }
}

fn ensure_termux_prefix_is_exported() {
    if std::env::var("PREFIX").is_ok() { return; }
    if let Some(prefix) = prefix_from_ld_preload_path() {
        unsafe {
            if let Ok(key) = CString::new("PREFIX") {
                if let Ok(value) = CString::new(prefix.as_str()) {
                    libc::setenv(key.as_ptr(), value.as_ptr(), 1);
                }
            }
        }
    }
}

fn prefix_from_ld_preload_path() -> Option<String> {
    let ld_preload = selected_ld_preload_path()?;
    let path = std::path::Path::new(&ld_preload);
    
    let mut curr = path.parent();
    while let Some(p) = curr {
        if p.ends_with("files") {
            let usr = p.join("usr");
            if usr.exists() { return Some(usr.to_string_lossy().to_string()); }
        }
        if p.ends_with("usr") {
            return Some(p.to_string_lossy().to_string());
        }
        curr = p.parent();
    }
    None
}

fn selected_ld_preload_path() -> Option<String> {
    current_library_path().or_else(|| {
        std::env::var("LD_PRELOAD").ok().filter(|v| !v.is_empty())
    })
}

fn current_library_path() -> Option<String> {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else { return None; };
    for line in maps.lines() {
        if let Some(idx) = line.find('/') {
            let path = &line[idx..];
            if path.ends_with("libtermux-exec.so") { return Some(path.to_string()); }
        }
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

    let current_lib = selected_ld_preload_path().unwrap_or_default();

    let mut prefix_set = false;
    let mut home_set = false;
    let mut path_idx = None;
    let mut ld_preload_idx = None;

    for (i, entry) in entries.iter_mut().enumerate() {
        if entry.starts_with("PREFIX=") { *entry = format!("PREFIX={}", prefix); prefix_set = true; }
        else if entry.starts_with("HOME=") { *entry = format!("HOME={}", home); home_set = true; }
        else if entry.starts_with("PATH=") { path_idx = Some(i); }
        else if entry.starts_with("LD_PRELOAD=") { ld_preload_idx = Some(i); }
    }

    if !prefix_set { entries.push(format!("PREFIX={}", prefix)); }
    if !home_set { entries.push(format!("HOME={}", home)); }

    let termux_bin = format!("{}/bin", prefix);
    if let Some(idx) = path_idx {
        let current_path = &entries[idx][5..];
        if !current_path.contains(&termux_bin) {
            entries[idx] = format!("PATH={}:{}", termux_bin, current_path);
        }
    } else {
        entries.push(format!("PATH={}:/system/bin", termux_bin));
    }

    if !current_lib.is_empty() {
        if let Some(idx) = ld_preload_idx {
            let existing = &entries[idx][11..];
            if existing != current_lib {
                android_log(LogPriority::WARN, "TermuxExec", &format!("termux_exec: refreshing LD_PRELOAD ({} -> {})", existing, current_lib));
                entries[idx] = format!("LD_PRELOAD={}", current_lib);
            }
        } else {
            entries.push(format!("LD_PRELOAD={}", current_lib));
        }
    }

    push_env_if_missing(entries, "TMPDIR", format!("{}/tmp", prefix));
    push_env_if_missing(entries, "TERM", "xterm-256color".to_string());
    push_env_if_missing(entries, "SHELL", format!("{}/bin/bash", prefix));
}

fn build_final_env(envp: *const *const c_char, original_path: &str) -> (Vec<CString>, Vec<*const c_char>) {
    let mut raw_entries = Vec::new();
    unsafe {
        let mut i = 0;
        while !envp.is_null() && !(*envp.offset(i)).is_null() {
            raw_entries.push(CStr::from_ptr(*envp.offset(i)).to_string_lossy().to_string());
            i += 1;
        }
    }
    
    let key = "TERMUX_ORIGINAL_EXE_PATH";
    let new_value = format!("{}={}", key, original_path);
    let mut replaced = false;
    for entry in &mut raw_entries {
        if entry.starts_with("TERMUX_ORIGINAL_EXE_PATH=") { *entry = new_value.clone(); replaced = true; break; }
    }
    if !replaced { raw_entries.push(new_value); }

    ensure_termux_core_env(&mut raw_entries);

    let c_entries: Vec<CString> = raw_entries.into_iter()
        .map(|s| CString::new(s).unwrap_or_else(|_| CString::new("").unwrap()))
        .collect();
    let mut ptrs: Vec<*const c_char> = c_entries.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(ptr::null());
    (c_entries, ptrs)
}

fn is_failsafe_mode() -> bool {
    std::env::var("TERMUX_FAILSAFE_MODE").ok().map(|v| v == "1" || v == "true").unwrap_or(false)
}

fn get_termux_prefix() -> String {
    if let Ok(prefix) = std::env::var("PREFIX") { return prefix; }
    if let Some(prefix) = prefix_from_ld_preload_path() { return prefix; }
    "/data/data/com.termux/files/usr".to_string()
}

fn map_path(path: &str) -> String {
    let prefix = get_termux_prefix();
    if path.starts_with("/usr/bin/") { return format!("{}/bin/{}", prefix, &path[9..]); }
    if path.starts_with("/bin/") { return format!("{}/bin/{}", prefix, &path[5..]); }
    let pkg_path = "/com.termux/files/usr/";
    if let Some(idx) = path.find(pkg_path) {
        return format!("{}/{}", prefix, &path[idx + pkg_path.len()..]);
    }
    path.to_string()
}

fn resolve_exec_path(path: &str, c_envs: &[CString]) -> Option<String> {
    if path.contains('/') { return Some(map_path(path)); }
    
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(':') {
            if dir.is_empty() { continue; }
            let cand = format!("{}/{}", dir, path);
            if std::path::Path::new(&cand).exists() { return Some(map_path(&cand)); }
        }
    }

    let path_env_from_entries = c_envs.iter()
        .map(|c| c.to_string_lossy())
        .find(|s| s.starts_with("PATH="))
        .map(|s| s[5..].to_string());

    if let Some(ps) = path_env_from_entries {
        for dir in ps.split(':') {
            if dir.is_empty() { continue; }
            let cand = format!("{}/{}", dir, path);
            if std::path::Path::new(&cand).exists() { return Some(map_path(&cand)); }
        }
    }

    let fallback = format!("{}/bin/{}", get_termux_prefix(), path);
    if std::path::Path::new(&fallback).exists() { Some(fallback) } else { None }
}

unsafe fn execve_common(path: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int {
    if path.is_null() || is_failsafe_mode() {
        return unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, path, argv, envp, 0) as c_int };
    }
    let path_str = unsafe { CStr::from_ptr(path) }.to_string_lossy().to_string();
    
    let mut args_summary = String::new();
    let mut i = 0;
    while !argv.is_null() && unsafe { !(*argv.offset(i)).is_null() } {
        if i > 0 { args_summary.push(' '); }
        args_summary.push_str(&unsafe { CStr::from_ptr(*argv.offset(i)) }.to_string_lossy());
        i += 1;
        if i > 8 { args_summary.push_str(" ..."); break; }
    }
    android_log(LogPriority::INFO, "PTY_CHECKPOINT", &format!("execve: \"{}\" with argv [{}]", path_str, args_summary));

    let (c_envs, env_ptrs) = build_final_env(envp, &path_str);
    let final_envp = env_ptrs.as_ptr();

    let is_linker = path_str.ends_with("/linker64") || path_str.ends_with("/linker");
    let is_flag_start = unsafe { !argv.is_null() && !(*argv.offset(1)).is_null() && (*(*argv.offset(1)) as u8) == b'-' };

    if is_linker && is_flag_start {
        if let Ok(orig) = std::env::var("TERMUX_ORIGINAL_EXE_PATH") {
            android_log(LogPriority::INFO, "PTY_CHECKPOINT", &format!("detected linker relaunch of \"{}\", redirecting", orig));
            if let Some((f_path, n_argv)) = transform_exec(&orig, argv, &c_envs, 0) {
                let Ok(c_p) = CString::new(f_path) else { return -1; };
                let a_ptrs: Vec<_> = n_argv.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
                return unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, c_p.as_ptr(), a_ptrs.as_ptr(), final_envp, 0) as c_int };
            }
        }
    }

    let res_path = map_path(&path_str);
    if let Some((f_path, n_argv)) = transform_exec(&res_path, argv, &c_envs, 0) {
        let Ok(c_p) = CString::new(f_path) else { return -1; };
        let a_ptrs: Vec<_> = n_argv.iter().map(|s| s.as_ptr()).chain(std::iter::once(ptr::null())).collect();
        return unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, c_p.as_ptr(), a_ptrs.as_ptr(), final_envp, 0) as c_int };
    }

    unsafe { libc::syscall(libc::SYS_execveat, libc::AT_FDCWD, path, argv, final_envp, 0) as c_int }
}

#[unsafe(no_mangle)] pub unsafe extern "C" fn execve(p: *const c_char, a: *const *const c_char, e: *const *const c_char) -> c_int { unsafe { execve_common(p, a, e) } }
#[unsafe(no_mangle)] pub unsafe extern "C" fn execvpe(p: *const c_char, a: *const *const c_char, e: *const *const c_char) -> c_int { unsafe { execve_common(p, a, e) } }
#[unsafe(no_mangle)] pub unsafe extern "C" fn execvp(p: *const c_char, a: *const *const c_char) -> c_int { unsafe { execve_common(p, a, environ as *const *const c_char) } }
#[unsafe(no_mangle)] pub unsafe extern "C" fn execveat(_d: c_int, p: *const c_char, a: *const *const c_char, e: *const *const c_char, _f: c_int) -> c_int {
    if p.is_null() { return -1; }
    unsafe { execve_common(p, a, e) }
}

fn is_app_private_path(path: &str) -> bool {
    path.contains("/com.termux/files/")
}

fn push_original_args(n_argv: &mut Vec<CString>, orig_argv: *const *const c_char) {
    if orig_argv.is_null() {
        return;
    }

    let mut i = 1;
    unsafe {
        while !(*orig_argv.offset(i)).is_null() {
            n_argv.push(CStr::from_ptr(*orig_argv.offset(i)).to_owned());
            i += 1;
        }
    }
}

fn transform_exec(path: &str, orig_argv: *const *const c_char, c_envs: &[CString], depth: u32) -> Option<(String, Vec<CString>)> {
    if depth > 4 { return None; }
    let abs_path = resolve_exec_path(path, c_envs)?;
    let mut file = std::fs::File::open(&abs_path).ok()?;
    let mut buf = [0u8; 1024];
    let n = file.read(&mut buf).ok()?;
    let linker = if std::path::Path::new("/system/bin/linker64").exists() { "/system/bin/linker64" } else { "/system/bin/linker" };

    if n > 17 && buf[0] == 0x7F && buf[1] == b'E' && buf[2] == b'L' && buf[3] == b'F' {
        if u16::from_le_bytes([buf[16], buf[17]]) != 3 { return None; }
        if !is_app_private_path(&abs_path) { return None; }

        let mut n_argv = vec![CString::new(linker).unwrap(), CString::new(abs_path).unwrap()];
        push_original_args(&mut n_argv, orig_argv);
        Some((linker.to_string(), n_argv))
    } else if let Some((interp, s_args)) = parse_shebang(&buf[..n]) {
        let res_interp = map_path(&interp);
        if let Some((r_linker, mut i_argv)) = transform_exec(&res_interp, std::ptr::null(), c_envs, depth + 1) {
            if let Some(args) = s_args { for arg in args.split_whitespace() { i_argv.push(CString::new(arg).unwrap()); } }
            i_argv.push(CString::new(abs_path).unwrap());
            push_original_args(&mut i_argv, orig_argv);
            Some((r_linker, i_argv))
        } else {
            let mut n_argv = vec![CString::new(res_interp.clone()).unwrap()];
            if let Some(args) = s_args { for arg in args.split_whitespace() { n_argv.push(CString::new(arg).unwrap()); } }
            n_argv.push(CString::new(abs_path).unwrap());
            push_original_args(&mut n_argv, orig_argv);
            Some((res_interp, n_argv))
        }
    } else { None }
}

fn parse_shebang(buf: &[u8]) -> Option<(String, Option<String>)> {
    if buf.len() < 2 || buf[0] != b'#' || buf[1] != b'!' { return None; }
    let end = buf.iter().position(|&b| b == b'\n').unwrap_or(buf.len());
    let line = String::from_utf8_lossy(&buf[2..end]);
    let tokens: Vec<&str> = line.trim().split_whitespace().collect();
    if tokens.is_empty() { return None; }
    Some((tokens[0].to_string(), if tokens.len() > 1 { Some(tokens[1..].join(" ")) } else { None }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_exec_does_not_wrap_system_shell_elf() {
        if !std::path::Path::new("/system/bin/sh").exists() {
            eprintln!("/system/bin/sh not present on this host; skipping Android-specific assertion");
            return;
        }

        let transformed = transform_exec("/system/bin/sh", std::ptr::null(), &[], 0);

        assert!(
            transformed.is_none(),
            "system/root ELF paths must keep system-shell exec semantics and not be rewritten through linker64; got {:?}",
            transformed.map(|(path, argv)| (
                path,
                argv.iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            ))
        );
    }

    #[test]
    fn transform_exec_wraps_app_private_et_dyn_elf() {
        let fake_elf = "/data/data/com.termux/files/home/tmp-termux-exec-fake-elf";
        let mut bytes = vec![0u8; 64];
        bytes[0] = 0x7f;
        bytes[1] = b'E';
        bytes[2] = b'L';
        bytes[3] = b'F';
        bytes[16] = 3;
        bytes[17] = 0;
        std::fs::write(fake_elf, bytes).unwrap();

        let transformed = transform_exec(fake_elf, std::ptr::null(), &[], 0)
            .map(|(path, argv)| {
                (
                    path,
                    argv.iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<_>>(),
                )
            });

        let _ = std::fs::remove_file(fake_elf);
        let (path, argv) = transformed.expect("app-private ET_DYN ELF should be linker-wrapped");
        assert!(path.ends_with("/linker64") || path.ends_with("/linker"));
        assert_eq!(argv[1], fake_elf);
    }
}

pub enum LogPriority { VERBOSE = 2, DEBUG = 3, INFO = 4, WARN = 5, ERROR = 6, FATAL = 7 }
#[cfg(target_os = "android")]
unsafe extern "C" { fn __android_log_print(prio: i32, tag: *const c_char, fmt: *const c_char, ...); }
pub fn android_log(prio: LogPriority, tag: &str, msg: &str) {
    #[cfg(target_os = "android")]
    {
        let t_c = CString::new(tag).unwrap_or_else(|_| CString::new("").unwrap());
        let m_c = CString::new(msg).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { __android_log_print(prio as i32, t_c.as_ptr(), b"%s\0".as_ptr() as *const c_char, m_c.as_ptr()); }
    }
    #[cfg(not(target_os = "android"))]
    {
        let p = match prio { LogPriority::FATAL => "F", LogPriority::ERROR => "E", LogPriority::WARN => "W", LogPriority::INFO => "I", _ => "D" };
        println!("[{}] {}: {}", p, tag, msg);
    }
}
