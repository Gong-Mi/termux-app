use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jstring};

use crate::utils::{LogPriority, android_log};

// ============================================================================
// WcWidth.java - Unicode 字符宽度计算
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_WcWidth_widthRust(
    _env: JNIEnv,
    _class: JClass,
    ucs: jint,
) -> jint {
    crate::utils::get_char_width(ucs as u32) as jint
}

// ============================================================================
// KeyHandler.java - 键盘按键处理
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getKeyCode(
    env: JNIEnv,
    _class: JClass,
    key_code: jint,
    key_mode: jint,
    cursor_app: jboolean,
    keypad: jboolean,
) -> jstring {
    let result = crate::terminal::key_handler::get_code(
        key_code,
        key_mode as u32,
        cursor_app != 0,
        keypad != 0,
    );

    match result {
        Some(s) => env.new_string(s).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_JNI_getKeyCodeFromTermcap(
    mut env: JNIEnv,
    _class: JClass,
    termcap: JString,
    cursor_app: jboolean,
    keypad: jboolean,
) -> jstring {
    let termcap_str: String = env.get_string(&termcap).unwrap().into();

    let result = crate::terminal::key_handler::get_code_from_termcap(
        &termcap_str,
        cursor_app != 0,
        keypad != 0,
    );

    match result {
        Some(s) => env.new_string(s).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    }
}

// ============================================================================
// JNI_OnLoad
// ============================================================================

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: std::ffi::c_void) -> jint {
    // === 顶级稳定性修复：全局禁用 fdsan ===
    // 强制关闭当前进程（父 JVM）和所有未来子进程的 fdsan 错误报告。
    // 彻底解决 'attempted to close file descriptor 0, actually owned by unique_fd' 导致的自杀。
    #[cfg(target_os = "android")]
    unsafe {
        unsafe extern "C" {
            fn android_fdsan_set_error_level(new_level: i32) -> i32;
        }
        android_fdsan_set_error_level(0); // 0 = ANDROID_FDSAN_ERROR_LEVEL_DISABLED
        android_log(
            LogPriority::INFO,
            "STABILITY: fdsan error level set to DISABLED in parent process",
        );
    }

    let result = crate::JAVA_VM.set(vm);
    match result {
        Ok(()) => android_log(
            LogPriority::INFO,
            "JNI_OnLoad: Termux- library loaded successfully",
        ),
        Err(_) => android_log(LogPriority::WARN, "JNI_OnLoad: JAVA_VM was already set"),
    }
    jni::sys::JNI_VERSION_1_6
}
