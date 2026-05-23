//! 诊断测试：验证 sigsys_handler 中 ucontext 寄存器读取是否正确
//! 
//! 编译（设备端）：
//!   cargo build --bin test_sigsys --target aarch64-linux-android
//! 推送运行：
//!   adb push target/aarch64-linux-android/debug/test_sigsys /data/local/tmp/
//!   adb shell /data/local/tmp/test_sigsys

use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};

static CAUGHT: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn get_execve_path(ucontext: *mut libc::c_void) -> libc::c_ulong;
    fn get_execve_argv(ucontext: *mut libc::c_void) -> libc::c_ulong;
    fn get_execve_envp(ucontext: *mut libc::c_void) -> libc::c_ulong;
}

#[repr(C)]
struct sock_filter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}
#[repr(C)]
struct sock_fprog {
    len: u16,
    _pad: [u16; 3],
    filter: *const sock_filter,
}

const SECCOMP_RET_TRAP: u32 = 0x00030000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;

/// 安全的字符串打印：只打印可打印 ASCII 和常见路径字符
fn safe_str(ptr: *const libc::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
    if s.len() > 0 && s.len() < 512 && s.chars().all(|c| c.is_ascii_graphic() || c == ' ' || c == '/' || c == '.' || c == '-' || c == '_') {
        Some(s.to_string())
    } else {
        None
    }
}

unsafe extern "C" fn sigsys_handler(_sig: libc::c_int, _info: *mut libc::siginfo_t, void_context: *mut libc::c_void) {
    println!("\n========== SIGSYS CAUGHT ==========");
    println!("void_context = {:p}", void_context);

    // Dump 前 32 个 u64，逐个尝试解释为字符串指针
    let ctx_ptr = void_context as *mut libc::c_ulong;
    println!("\n--- ucontext raw dump (first 32 u64) ---");
    for i in 0..32 {
        let val = unsafe { *ctx_ptr.offset(i) };
        if let Some(s) = safe_str(val as *const libc::c_char) {
            println!("  offset({:2}) = 0x{:016x}  ->  \"{}\"", i, val, s);
        } else {
            println!("  offset({:2}) = 0x{:016x}", i, val);
        }
    }

    // 对比 get_regs.c 的输出
    let path_from_c = unsafe { get_execve_path(void_context) as *const libc::c_char };
    let argv_from_c = unsafe { get_execve_argv(void_context) as *const *const libc::c_char };
    let envp_from_c = unsafe { get_execve_envp(void_context) as *const *const libc::c_char };

    println!("\n--- get_regs.c helper output ---");
    println!("get_execve_path -> {:p}", path_from_c);
    if let Some(s) = safe_str(path_from_c) {
        println!("  -> \"{}\"", s);
    }
    println!("get_execve_argv -> {:p}", argv_from_c);
    if !argv_from_c.is_null() {
        let arg0 = unsafe { *argv_from_c };
        if !arg0.is_null() {
            if let Some(s) = safe_str(arg0) {
                println!("  argv[0] -> \"{}\"", s);
            }
        }
    }
    println!("get_execve_envp -> {:p}", envp_from_c);

    // 手动遍历 envp，找 PREFIX 和 HOME，帮助判断 app 路径
    if !envp_from_c.is_null() {
        println!("\n--- envp scan (looking for PREFIX/HOME/LD_PRELOAD) ---");
        let mut i = 0;
        loop {
            let env = unsafe { *envp_from_c.offset(i) };
            if env.is_null() {
                break;
            }
            if let Some(s) = safe_str(env) {
                if s.starts_with("PREFIX=") || s.starts_with("HOME=") || s.starts_with("LD_PRELOAD=") {
                    println!("  {}", s);
                }
            }
            i += 1;
        }
    }

    // 在 ucontext dump 中搜索预期的字符串，自动匹配正确偏移
    let expected_path = "/system/bin/echo";
    let expected_argv0 = "echo";
    println!("\n--- auto-match expected values ---");
    println!("Looking for path=\"{}\"  argv[0]=\"{}\"", expected_path, expected_argv0);
    let mut matched_path_offset = -1i32;
    let mut matched_argv0_offset = -1i32;
    for i in 0..32 {
        let val = unsafe { *ctx_ptr.offset(i) };
        if let Some(s) = safe_str(val as *const libc::c_char) {
            if s == expected_path {
                matched_path_offset = i as i32;
            }
            if s == expected_argv0 {
                matched_argv0_offset = i as i32;
            }
        }
    }
    if matched_path_offset >= 0 {
        println!("AUTO-MATCH: path found at offset({})", matched_path_offset);
    } else {
        println!("AUTO-MATCH: path NOT FOUND in first 32 offsets!");
    }
    if matched_argv0_offset >= 0 {
        println!("AUTO-MATCH: argv[0] found at offset({})", matched_argv0_offset);
    } else {
        println!("AUTO-MATCH: argv[0] NOT FOUND in first 32 offsets!");
    }

    println!("========== END SIGSYS ==========\n");
    CAUGHT.store(true, Ordering::SeqCst);
}

fn main() {
    unsafe {
        // 1. 安装 SIGSYS handler
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigsys_handler as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigaction(libc::SIGSYS, &sa, std::ptr::null_mut());

        // 2. 安装 seccomp filter：只 trap execve
        let filter = [
            sock_filter { code: 0x20, jt: 0, jf: 0, k: 0 },
            sock_filter { code: 0x15, jt: 1, jf: 0, k: libc::SYS_execve as u32 },
            sock_filter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW },
            sock_filter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_TRAP },
        ];
        let prog = sock_fprog {
            len: filter.len() as u16,
            _pad: [0; 3],
            filter: filter.as_ptr(),
        };
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            eprintln!("prctl(NO_NEW_PRIVS) failed");
            std::process::exit(1);
        }
        if libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &prog as *const _) != 0 {
            eprintln!("prctl(SECCOMP) failed");
            std::process::exit(1);
        }

        // 3. 触发 execve("/system/bin/echo", ["echo", "hello"], envp)
        let path = CString::new("/system/bin/echo").unwrap();
        let argv0 = CString::new("echo").unwrap();
        let argv1 = CString::new("hello").unwrap();
        let argv: [*const libc::c_char; 3] = [argv0.as_ptr(), argv1.as_ptr(), std::ptr::null()];

        println!("[MAIN] Triggering execve(\"{}\", [\"{}\", \"{}\"])", path.to_str().unwrap(), argv0.to_str().unwrap(), argv1.to_str().unwrap());
        libc::syscall(libc::SYS_execve, path.as_ptr(), argv.as_ptr(), std::ptr::null::<libc::c_char>());

        // 4. execve 被拦截后不会返回，但如果 filter 没生效会走到这里
        if CAUGHT.load(Ordering::SeqCst) {
            println!("[MAIN] ✅ SIGSYS caught and diagnosed successfully");
        } else {
            println!("[MAIN] ❌ SIGSYS was NOT caught — seccomp filter may have failed");
        }
    }
}
