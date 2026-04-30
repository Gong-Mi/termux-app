use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::jint;
use std::os::unix::io::RawFd;
use std::os::fd::BorrowedFd;
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};

/// Rust implementation of LocalSocketManager.getPeerCredNative
#[no_mangle]
pub extern "system" fn Java_com_termux_shared_net_socket_local_LocalSocketManager_getPeerCredNative<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
    _log_title: JString<'a>,
    fd: jint,
    peer_cred: JObject<'a>,
) -> JObject<'a> {
    let fd = unsafe { BorrowedFd::borrow_raw(fd as RawFd) };
    let creds = match getsockopt(&fd, PeerCredentials) {
        Ok(c) => c,
        Err(e) => {
            return create_jni_result(&mut env, -1, e as i32, "getsockopt failed");
        }
    };

    // Set fields in the PeerCred Java object
    let peer_cred_class = env.get_object_class(&peer_cred).unwrap();
    
    let _ = env.set_field(&peer_cred, "pid", "I", (creds.pid() as i32).into());
    let _ = env.set_field(&peer_cred, "uid", "I", (creds.uid() as i32).into());
    let _ = env.set_field(&peer_cred, "gid", "I", (creds.gid() as i32).into());

    // Try to get process name from /proc/[pid]/cmdline (simplified)
    if let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{}/cmdline", creds.pid())) {
        let pname = cmdline.split('\0').next().unwrap_or("");
        let j_pname = env.new_string(pname).unwrap();
        let _ = env.set_field(&peer_cred, "pname", "Ljava/lang/String;", (&j_pname).into());
        
        let spaced = cmdline.replace('\0', " ");
        let j_cmdline = env.new_string(spaced.trim()).unwrap();
        let _ = env.set_field(&peer_cred, "cmdline", "Ljava/lang/String;", (&j_cmdline).into());
    }

    create_jni_result(&mut env, 0, 0, "")
}

/// Helper to create com.termux.shared.jni.models.JniResult
fn create_jni_result<'a>(env: &mut JNIEnv<'a>, retval: i32, errno: i32, errmsg: &str) -> JObject<'a> {
    let class = env.find_class("com/termux/shared/jni/models/JniResult").unwrap();
    let j_errmsg = env.new_string(errmsg).unwrap();
    
    env.new_object(
        class,
        "(IILjava/lang/String;I)V",
        &[retval.into(), errno.into(), (&j_errmsg).into(), 0.into()],
    ).unwrap()
}
