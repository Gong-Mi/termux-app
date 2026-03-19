use jni::JNIEnv;
use jni::JavaVM;
use jni::objects::{JClass, JByteArray};
use jni::sys::{jint, jlong, jbyteArray};
use once_cell::sync::OnceCell;

// 声明子模块
pub mod utils;
pub mod engine;
pub mod bootstrap;
pub mod fastpath;
pub mod pty;

// 提供兼容性别名
pub use engine as terminal_engine;

use engine::TerminalEngine;

// 为 engine.rs 提供的静态变量
pub static JAVA_VM: OnceCell<JavaVM> = OnceCell::new();

pub struct TerminalContext {
    pub engine: TerminalEngine,
}

// --------------------------------------------------------
// JNI 导出函数
// --------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _reserved: std::ffi::c_void) -> jint {
    let _ = JAVA_VM.set(vm);
    jni::sys::JNI_VERSION_1_6
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeInit(
    _env: JNIEnv,
    _class: JClass,
    cols: jint,
    rows: jint,
) -> jlong {
    // 调用真正的构造函数
    let engine = TerminalEngine::new(cols, rows, 2000, 10, 20);
    let context = Box::new(TerminalContext { engine });
    
    Box::into_raw(context) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeResize(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    cols: jint,
    rows: jint,
) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    // 修正参数类型：engine.rs 期望 i32
    context.engine.state.resize(cols, rows);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeProcess(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    
    let array = unsafe { JByteArray::from_raw(data) };
    if let Ok(bytes) = env.convert_byte_array(&array) {
        context.engine.process_bytes(&bytes);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorX(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor_x as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorY(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.state.cursor_y as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        unsafe {
            let _ = Box::from_raw(ptr as *mut TerminalContext);
        }
    }
}
