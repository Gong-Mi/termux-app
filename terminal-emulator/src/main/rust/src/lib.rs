use jni::JNIEnv;
use jni::objects::{JClass, JByteArray};
use jni::sys::{jint, jlong, jbyteArray};

mod terminal_engine;
use terminal_engine::TerminalEngine;

pub struct TerminalContext {
    pub engine: TerminalEngine,
}

// --------------------------------------------------------
// JNI 导出函数 (遵循 Java_包名_类名_方法名 规范)
// --------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeInit(
    _env: JNIEnv,
    _class: JClass,
    cols: jint,
    rows: jint,
) -> jlong {
    let engine = TerminalEngine::new(cols, rows);
    let context = Box::new(TerminalContext { engine });
    
    // 将 Rust 对象的指针返回给 Java 长期持有
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
    context.engine.resize(cols, rows);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeProcess(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: jbyteArray,
) {
    let context = unsafe { &mut *(ptr as *mut TerminalContext) };
    
    // 显式将原始 jbyteArray 转换为 JByteArray 对象以进行转换
    let array = unsafe { JByteArray::from_raw(data) };
    if let Ok(bytes) = env.convert_byte_array(&array) {
        context.engine.parse_bytes(&bytes);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorX(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.cursor_x
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeGetCursorY(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    let context = unsafe { &*(ptr as *const TerminalContext) };
    context.engine.cursor_y
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_terminal_TerminalEmulator_nativeRelease(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        // 回收内存，防止泄露
        unsafe {
            let _ = Box::from_raw(ptr as *mut TerminalContext);
        }
    }
}
