//! 本地 Unix Domain Socket 服务器
//! 用于替代 Java 层的 TermuxAmSocketServer
//! 提供高性能的子进程控制和状态查询接口

use crate::coordinator::SessionCoordinator;
use crate::utils::{LogPriority, android_log};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;

/// 启动 Rust 本地 Socket 服务器
pub fn start_server(socket_path: String) {
    thread::spawn(move || {
        let path = Path::new(&socket_path);

        // 1. 创建父目录以防其不存在
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    android_log(
                        LogPriority::ERROR,
                        &format!(
                            "Failed to create socket directory {}: {}",
                            parent.display(),
                            e
                        ),
                    );
                    return;
                }
                #[cfg(target_os = "android")]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
                }
            }
        }

        // 2. 清理旧的 socket 文件
        if path.exists() {
            if let Err(e) = fs::remove_file(path) {
                android_log(
                    LogPriority::ERROR,
                    &format!("Failed to remove old socket {}: {}", socket_path, e),
                );
                return;
            }
        }

        // 3. 绑定并监听
        let listener = match UnixListener::bind(path) {
            Ok(l) => l,
            Err(e) => {
                android_log(
                    LogPriority::ERROR,
                    &format!("Failed to bind socket {}: {}", socket_path, e),
                );
                return;
            }
        };

        // 3. 设置权限：仅允许当前用户访问 (0600)
        // 在 Android 上，这对于安全至关重要
        #[cfg(target_os = "android")]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }

        android_log(
            LogPriority::INFO,
            &format!("Rust LocalSocket server started on {}", socket_path),
        );

        // 4. 接受连接循环
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    thread::spawn(|| handle_client(stream));
                }
                Err(e) => {
                    android_log(LogPriority::WARN, &format!("Socket accept failed: {}", e));
                }
            }
        }
    });
}

fn handle_client(mut stream: UnixStream) {
    let mut buffer = [0u8; 4096];
    let n = match stream.read(&mut buffer) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let request = String::from_utf8_lossy(&buffer[..n]);
    let parts: Vec<&str> = request.trim().split_whitespace().collect();

    if parts.is_empty() {
        return;
    }

    let command = parts[0];
    let args = &parts[1..];

    // 执行结果：exit_code\0stdout\0stderr\0
    let (exit_code, stdout, stderr) = match command {
        "am" => {
            // 通过 JNI 调用 Java 侧的 Am 库
            handle_am_command(args)
        }
        "status" => {
            let info = SessionCoordinator::get().get_all_session_states();
            let mut report = String::from("Rust Engine Status: Active\n");
            for (id, state) in info {
                report.push_str(&format!("  Session {}: {}\n", id, state.as_str()));
            }
            (0, report, String::new())
        }
        "ping" => (0, "pong".to_string(), String::new()),
        _ => (1, String::new(), format!("Unknown command: {}", command)),
    };

    // 按照 Termux 协议封装返回数据
    let response = format!("{}\0{}\0{}\0", exit_code, stdout, stderr);
    let _ = stream.write_all(response.as_bytes());
}

/// 核心：通过 JNI 调用 Java 的 Activity Manager 接口
fn handle_am_command(args: &[&str]) -> (i32, String, String) {
    let vm = match crate::JAVA_VM.get() {
        Some(v) => v,
        None => return (1, String::new(), "Java VM not initialized".to_string()),
    };
    let mut env = match vm.attach_current_thread_as_daemon() {
        Ok(e) => e,
        Err(e) => {
            return (
                1,
                String::new(),
                format!("Failed to attach JVM thread: {:?}", e),
            );
        }
    };

    let class_name = "com/termux/shared/termux/shell/am/RustLocalSocketBridge";
    let cls = match env.find_class(class_name) {
        Ok(c) => c,
        Err(e) => {
            return (
                1,
                String::new(),
                format!("Class {} not found: {:?}", class_name, e),
            );
        }
    };

    // 将 Rust args 转换为 Java StringArray
    let empty_jstring = match env.new_string("") {
        Ok(s) => s,
        Err(e) => {
            return (
                1,
                String::new(),
                format!("Failed to create empty string: {:?}", e),
            );
        }
    };
    let j_args = match env.new_object_array(args.len() as i32, "java/lang/String", &empty_jstring) {
        Ok(a) => a,
        Err(e) => return (1, String::new(), format!("Failed to create array: {:?}", e)),
    };
    for (i, &arg) in args.iter().enumerate() {
        let s = match env.new_string(arg) {
            Ok(js) => js,
            Err(_) => continue,
        };
        let _ = env.set_object_array_element(&j_args, i as i32, &s);
    }

    let result = match env.call_static_method(
        &cls,
        "runAmInternal",
        "([Ljava/lang/String;)Lcom/termux/shared/jni/models/JniResult;",
        &[jni::objects::JValue::Object(&j_args)],
    ) {
        Ok(r) => r,
        Err(e) => return (1, String::new(), format!("JNI call failed: {:?}", e)),
    };

    let obj = match result.l() {
        Ok(o) => o,
        Err(e) => return (1, String::new(), format!("Failed to get object: {:?}", e)),
    };

    // 解析 JniResult
    let exit_code = env
        .get_field(&obj, "retval", "I")
        .and_then(|v| v.i())
        .unwrap_or(-1);

    let stdout = match env.get_field(&obj, "stdout", "Ljava/lang/String;") {
        Ok(v) => match v.l() {
            Ok(o) => {
                let jstr = jni::objects::JString::from(o);
                env.get_string(&jstr)
                    .map(|s| String::from(s))
                    .unwrap_or_default()
            }
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    };

    let stderr = match env.get_field(&obj, "stderr", "Ljava/lang/String;") {
        Ok(v) => match v.l() {
            Ok(o) => {
                let jstr = jni::objects::JString::from(o);
                env.get_string(&jstr)
                    .map(|s| String::from(s))
                    .unwrap_or_default()
            }
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    };

    (exit_code, stdout, stderr)
}
