use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JByteArray, JValue};
use jni::sys::{jint, jlong, jobject};
use crate::utils::local_socket as ls;
use crate::utils::{android_log, LogPriority};
use std::os::unix::io::RawFd;

/// Helper to create JniResult object
fn get_jni_result<'a>(
    env: &mut JNIEnv<'a>,
    _title: &JString,
    retval: i32,
    errno: i32,
    errmsg: &str,
    int_data: i32,
) -> JObject<'a> {
    let clazz = match env.find_class("com/termux/shared/jni/models/JniResult") {
        Ok(c) => c,
        Err(_) => {
            android_log(LogPriority::ERROR, "Failed to find JniResult class");
            return JObject::null();
        }
    };

    let errmsg_j = env.new_string(errmsg).unwrap_or_else(|_| env.new_string("").unwrap());
    
    let args = [
        JValue::Int(retval),
        JValue::Int(errno),
        JValue::Object(errmsg_j.as_ref()),
        JValue::Int(int_data),
    ];

    match env.new_object(clazz, "(IILjava/lang/String;I)V", &args) {
        Ok(obj) => obj,
        Err(e) => {
            android_log(LogPriority::ERROR, &format!("Failed to create JniResult object: {:?}", e));
            JObject::null()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_createServerSocketNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    path_array: JByteArray,
    backlog: jint,
) -> jobject {
    let path = match env.convert_byte_array(&path_array) {
        Ok(p) => p,
        Err(_) => return get_jni_result(&mut env, &log_title, -1, 0, "Failed to convert path array", 0).into_raw(),
    };

    match ls::create_server_socket(&path, backlog) {
        Ok(fd) => get_jni_result(&mut env, &log_title, 0, 0, "", fd as i32).into_raw(),
        Err(e) => get_jni_result(&mut env, &log_title, -1, e as i32, &format!("Create server socket failed: {}", e), 0).into_raw(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_closeSocketNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
) -> jobject {
    let _ = nix::unistd::close(fd as RawFd);
    get_jni_result(&mut env, &log_title, 0, 0, "", 0).into_raw()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_acceptNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
) -> jobject {
    match ls::accept_client(fd as RawFd) {
        Ok(client_fd) => get_jni_result(&mut env, &log_title, 0, 0, "", client_fd as i32).into_raw(),
        Err(e) => get_jni_result(&mut env, &log_title, -1, e as i32, &format!("Accept failed: {}", e), 0).into_raw(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_readNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
    data_array: JByteArray,
    deadline: jlong,
) -> jobject {
    let len = env.get_array_length(&data_array).unwrap_or(0) as usize;
    let mut buf = vec![0u8; len];
    
    match ls::read_socket(fd as RawFd, &mut buf, deadline) {
        Ok(n) => {
            let _ = env.set_byte_array_region(&data_array, 0, bytemuck::cast_slice(&buf[..n]));
            get_jni_result(&mut env, &log_title, 0, 0, "", n as i32).into_raw()
        }
        Err(e) => get_jni_result(&mut env, &log_title, -1, e as i32, &format!("Read failed: {}", e), 0).into_raw(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_sendNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
    data_array: JByteArray,
    deadline: jlong,
) -> jobject {
    let data = match env.convert_byte_array(&data_array) {
        Ok(d) => d,
        Err(_) => return get_jni_result(&mut env, &log_title, -1, 0, "Failed to convert data array", 0).into_raw(),
    };

    match ls::send_socket(fd as RawFd, &data, deadline) {
        Ok(_) => get_jni_result(&mut env, &log_title, 0, 0, "", 0).into_raw(),
        Err(e) => get_jni_result(&mut env, &log_title, -1, e as i32, &format!("Send failed: {}", e), 0).into_raw(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_availableNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
) -> jobject {
    let mut available = 0i32;
    const FIONREAD: libc::c_int = 0x541B;
    unsafe {
        if libc::ioctl(fd, FIONREAD, &mut available) == -1 {
            let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return get_jni_result(&mut env, &log_title, -1, e, "ioctl FIONREAD failed", 0).into_raw();
        }
    }
    get_jni_result(&mut env, &log_title, 0, 0, "", available).into_raw()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_setSocketReadTimeoutNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
    timeout_ms: jint,
) -> jobject {
    let tv = libc::timeval {
        tv_sec: (timeout_ms / 1000) as libc::time_t,
        tv_usec: ((timeout_ms % 1000) * 1000) as libc::suseconds_t,
    };
    let res = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &tv as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if res == -1 {
        let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        return get_jni_result(&mut env, &log_title, -1, e, "setsockopt SO_RCVTIMEO failed", 0).into_raw();
    }
    get_jni_result(&mut env, &log_title, 0, 0, "", 0).into_raw()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_setSocketSendTimeoutNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
    timeout_ms: jint,
) -> jobject {
    let tv = libc::timeval {
        tv_sec: (timeout_ms / 1000) as libc::time_t,
        tv_usec: ((timeout_ms % 1000) * 1000) as libc::suseconds_t,
    };
    let res = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDTIMEO,
            &tv as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if res == -1 {
        let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        return get_jni_result(&mut env, &log_title, -1, e, "setsockopt SO_SNDTIMEO failed", 0).into_raw();
    }
    get_jni_result(&mut env, &log_title, 0, 0, "", 0).into_raw()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_getPeerCredNative(
    mut env: JNIEnv,
    _class: JClass,
    log_title: JString,
    fd: jint,
    peer_cred_obj: JObject,
) -> jobject {
    match ls::get_peer_cred(fd as RawFd) {
        Ok(cred) => {
            let _ = env.set_field(&peer_cred_obj, "pid", "I", JValue::Int(cred.pid));
            let _ = env.set_field(&peer_cred_obj, "uid", "I", JValue::Int(cred.uid));
            let _ = env.set_field(&peer_cred_obj, "gid", "I", JValue::Int(cred.gid));
            
            if let Ok(pname_j) = env.new_string(cred.pname) {
                let _ = env.set_field(&peer_cred_obj, "pname", "Ljava/lang/String;", JValue::Object(pname_j.as_ref()));
            }
            
            if let Ok(cmdline_j) = env.new_string(cred.cmdline) {
                let _ = env.set_field(&peer_cred_obj, "cmdline", "Ljava/lang/String;", JValue::Object(cmdline_j.as_ref()));
            }
            
            get_jni_result(&mut env, &log_title, 0, 0, "", 0).into_raw()
        }
        Err(e) => get_jni_result(&mut env, &log_title, -1, e as i32, &format!("Get peer cred failed: {}", e), 0).into_raw(),
    }
}
