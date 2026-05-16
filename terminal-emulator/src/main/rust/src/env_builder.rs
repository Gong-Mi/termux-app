//! Termux 环境变量构建器
//!
//! 负责从零构建完整的子进程环境变量表。
//! 设计理念：Rust 层是环境变量的唯一权威，不再依赖 Java 层传递已构建好的环境数组。
//! 这消除了 Java ↔ Native 之间的“中间状态”不一致问题，并为未来应对更高 Android 版本限制做准备。

use std::collections::HashMap;
use std::ffi::CString;

use crate::utils::{LogPriority, android_log};

/// 构建完整的 Termux 子进程环境变量表。
///
/// # 参数
/// - `cwd`: 工作目录（用于设置 PWD）
/// - `is_failsafe`: 是否为 failsafe 模式。failsafe 模式下保留系统 PATH/TMPDIR，不注入 Termux 路径。
///
/// # 返回
/// 可直接用于 `clearenv()` + `putenv()` 的 `CString` 向量。
pub fn build_termux_environment(cwd: &str, is_failsafe: bool) -> Vec<CString> {
    let mut env = HashMap::new();

    // ------------------------------------------------------------------
    // 1. 继承必要的 Android 系统环境变量（从当前进程读取）
    //    只保留对 Termux shell 实际有用的，过滤掉 Android framework 内部变量。
    // ------------------------------------------------------------------
    inherit_system_var(&mut env, "ANDROID_DATA");
    inherit_system_var(&mut env, "ANDROID_ROOT");
    inherit_system_var(&mut env, "ANDROID_STORAGE");
    inherit_system_var(&mut env, "EXTERNAL_STORAGE");
    inherit_system_var(&mut env, "ANDROID_RUNTIME_ROOT");
    inherit_system_var(&mut env, "ANDROID_ART_ROOT");
    inherit_system_var(&mut env, "ANDROID_I18N_ROOT");
    inherit_system_var(&mut env, "ANDROID_TZDATA_ROOT");

    // BUG FIX: On Android 12+, app_process (am command) REQUIRES these variables
    // to find core Java classes. Without them, it will Abort immediately.
    inherit_system_var(&mut env, "BOOTCLASSPATH");
    inherit_system_var(&mut env, "DEX2OATBOOTCLASSPATH");
    inherit_system_var(&mut env, "SYSTEMSERVERCLASSPATH");

    // ------------------------------------------------------------------
    // 2. 基础终端环境（与 upstream Java 的 AndroidShellEnvironment 对齐）
    // ------------------------------------------------------------------
    env.insert("LANG".to_string(), "en_US.UTF-8".to_string());
    env.insert("COLORTERM".to_string(), "truecolor".to_string());
    env.insert("TERM".to_string(), "xterm-256color".to_string());

    // ------------------------------------------------------------------
    // 3. Termux 核心路径（与 upstream Java 的 TermuxShellEnvironment 对齐）
    // ------------------------------------------------------------------
    let termux_home = crate::get_termux_home();
    let termux_prefix = crate::get_termux_prefix();
    let termux_tmp = format!("{}/tmp", termux_prefix);

    env.insert("HOME".to_string(), termux_home.clone());
    env.insert("PREFIX".to_string(), termux_prefix.clone());

    // ------------------------------------------------------------------
    // 4. PATH / TMPDIR / LD_PRELOAD 的 failsafe 差异化处理
    // ------------------------------------------------------------------
    if is_failsafe {
        // Failsafe 模式：保留系统默认值，让 /system/bin/sh 等系统命令可用
        // 不设置 Termux 特有的 PATH、TMPDIR，也不注入 LD_PRELOAD
        if let Ok(sys_path) = std::env::var("PATH") {
            env.insert("PATH".to_string(), sys_path);
        } else {
            env.insert("PATH".to_string(), "/system/bin".to_string());
        }
        // TMPDIR 保持系统默认 /data/local/tmp（如果系统环境里有，前面已继承；否则不强制设置）
        android_log(
            LogPriority::INFO,
            "[env_builder] Failsafe mode: using system PATH/TMPDIR, no Termux injection",
        );
    } else {
        // 正常模式：注入 Termux 路径
        // Android 7+ 的 upstream 只放 TERMUX_BIN_PREFIX_DIR_PATH，不加 applets。
        // 但当前项目代码历史原因 hardcoded 了 applets，为兼容保留。
        let termux_bin_path = format!("{}/bin:{}/bin/applets", termux_prefix, termux_prefix);
        if let Ok(sys_path) = std::env::var("PATH") {
            // Prepend Termux 路径，保持系统路径作为 fallback
            env.insert(
                "PATH".to_string(),
                format!("{}:{}", termux_bin_path, sys_path),
            );
        } else {
            env.insert(
                "PATH".to_string(),
                format!("{}:/system/bin", termux_bin_path),
            );
        }

        env.insert("TMPDIR".to_string(), termux_tmp.to_string());

        // LD_PRELOAD 用于 termux-exec 的 shebang 修复和 W^X 绕过。
        // 我们强制确保 LD_PRELOAD 指向有效的 libtermux-exec.so
        let termux_app_lib = format!("{}/applib", crate::get_termux_files_dir());
        let ld_preload_variants = [
            "libtermux-exec-linker-ld-preload.so",
            "libtermux-exec-ld-preload.so",
            "libtermux-exec.so",
            "libtermux-exec-direct-ld-preload.so",
        ];

        let mut found_ld_preload = false;
        
        // 1. 优先检查 applib (APK 原生库目录，API 29+ 必需)
        // 注意：必须使用真实路径而非 /data/data/ 下的软链接，否则 Android Linker 会因为安全策略拒绝加载
        let termux_files_dir = crate::get_termux_files_dir();
        let termux_app_lib = format!("{}/applib", termux_files_dir);
        let app_lib_path = std::path::Path::new(&termux_app_lib);
        
        let real_app_lib = if app_lib_path.is_symlink() {
            std::fs::read_link(app_lib_path).unwrap_or_else(|_| app_lib_path.to_path_buf())
        } else {
            app_lib_path.to_path_buf()
        };
        
        // Ensure absolute path
        let real_app_lib = if real_app_lib.is_absolute() {
            real_app_lib
        } else {
            // If it's a relative symlink (unlikely for applib but safe to handle), make it absolute
            std::path::Path::new(&termux_files_dir).join(real_app_lib)
        };

        for variant in &ld_preload_variants {
            let ld_preload_path = real_app_lib.join(variant);
            if ld_preload_path.exists() {
                let path_str = ld_preload_path.to_string_lossy().to_string();
                env.insert("LD_PRELOAD".to_string(), path_str.clone());
                android_log(
                    LogPriority::INFO,
                    &format!("[env_builder] Using LD_PRELOAD from real applib path: {}", path_str),
                );
                found_ld_preload = true;
                break;
            }
        }

        // 2. Fallback 到传统的 prefix/lib
        if !found_ld_preload {
            for variant in &ld_preload_variants {
                let ld_preload_path = format!("{}/lib/{}", termux_prefix, variant);
                if std::path::Path::new(&ld_preload_path).exists() {
                    env.insert("LD_PRELOAD".to_string(), ld_preload_path.to_string());
                    android_log(
                        LogPriority::INFO,
                        &format!("[env_builder] Using LD_PRELOAD from prefix: {}", ld_preload_path),
                    );
                    found_ld_preload = true;
                    break;
                }
            }
        }

        // 3. 强制兜底：如果都没找到，尝试使用标准 symlink 路径
        if !found_ld_preload {
            let ld_preload_path = format!("{}/lib/libtermux-exec.so", termux_prefix);
            env.insert("LD_PRELOAD".to_string(), ld_preload_path.to_string());
            android_log(
                LogPriority::WARN,
                &format!("[env_builder] Forced LD_PRELOAD fallback to: {}", ld_preload_path),
            );
        }
    }

    // ------------------------------------------------------------------
    // 5. Termux 版本号（clipboardy 等库依赖此变量检测 Termux 环境）
    // ------------------------------------------------------------------
    if let Some(version_mutex) = crate::TERMUX_VERSION.get() {
        if let Ok(version) = version_mutex.lock() {
            env.insert("TERMUX_VERSION".to_string(), version.clone());
        }
    }

    // ------------------------------------------------------------------
    // 6. SHELL（默认 bash）
    // ------------------------------------------------------------------
    env.insert("SHELL".to_string(), format!("{}/bin/bash", termux_prefix));

    // ------------------------------------------------------------------
    // 7. PWD（工作目录）
    // ------------------------------------------------------------------
    let pwd = if !cwd.is_empty() {
        // 尽量使用绝对路径；如果 cwd 是相对路径，保留原样（shell 自己会处理）
        cwd.to_string()
    } else {
        termux_home.to_string()
    };
    env.insert("PWD".to_string(), pwd);

    // ------------------------------------------------------------------
    // 8. 合并 Java 层传递的扩展环境变量（TERMUX_APP__* 等）
    // ------------------------------------------------------------------
    if let Some(ext_mutex) = crate::EXTENDED_ENV.get() {
        if let Ok(ext_map) = ext_mutex.lock() {
            for (k, v) in ext_map.iter() {
                env.insert(k.clone(), v.clone());
            }
        }
    }

    // ------------------------------------------------------------------
    // 9. 转换为 CString 数组（按 key 排序，便于日志对比）
    // ------------------------------------------------------------------
    let mut pairs: Vec<(String, String)> = env.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let c_envs: Vec<CString> = pairs
        .into_iter()
        .map(|(k, v)| {
            let s = format!("{}={}", k, v);
            CString::new(s).unwrap_or_else(|_| CString::new(format!("{}=", k)).unwrap())
        })
        .collect();

    android_log(
        LogPriority::DEBUG,
        &format!(
            "[env_builder] Built {} env vars (failsafe={})",
            c_envs.len(),
            is_failsafe
        ),
    );

    c_envs
}

/// 从当前进程环境变量中继承指定 key（如果存在）。
fn inherit_system_var(map: &mut HashMap<String, String>, key: &str) {
    if let Ok(val) = std::env::var(key) {
        if !val.is_empty() {
            map.insert(key.to_string(), val);
        }
    }
}

// ------------------------------------------------------------------
// 可选：若未来 Java 层仍需传递少量扩展变量（如 TERMUX_APP__*），
// 可在此提供 merge 接口。目前 Rust 完全自主构建，不依赖 Java 层 envp。
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failsafe_no_termux_injection() {
        let envs = build_termux_environment("/data/data/com.termux/files/home", true);
        let env_strs: Vec<String> = envs
            .iter()
            .map(|c| c.to_string_lossy().to_string())
            .collect();

        // failsafe 模式下不应主动设置 Termux 特有的 LD_PRELOAD
        assert!(!env_strs.iter().any(|s| s.starts_with("LD_PRELOAD=")));

        // failsafe 模式下 TMPDIR 不应是 Termux 的 tmp
        let tmpdir = env_strs.iter().find(|s| s.starts_with("TMPDIR="));
        if let Some(t) = tmpdir {
            assert!(
                !t.contains("/data/data/com.termux/files/usr/tmp"),
                "Failsafe TMPDIR should not be Termux tmp: {}",
                t
            );
        }
    }

    #[test]
    fn test_normal_has_termux_paths() {
        let envs = build_termux_environment("/data/data/com.termux/files/home", false);
        let env_strs: Vec<String> = envs
            .iter()
            .map(|c| c.to_string_lossy().to_string())
            .collect();

        let path_entry = env_strs
            .iter()
            .find(|s| s.starts_with("PATH="))
            .expect("PATH must exist");
        assert!(
            path_entry.contains("/data/data/com.termux/files/usr/bin"),
            "Normal PATH should contain Termux bin: {}",
            path_entry
        );

        let tmpdir = env_strs
            .iter()
            .find(|s| s.starts_with("TMPDIR="))
            .expect("TMPDIR must exist");
        assert_eq!(tmpdir, "TMPDIR=/data/data/com.termux/files/usr/tmp");

        assert!(
            env_strs
                .iter()
                .any(|s| s.starts_with("HOME=/data/data/com.termux/files/home"))
        );
        assert!(
            env_strs
                .iter()
                .any(|s| s.starts_with("PREFIX=/data/data/com.termux/files/usr"))
        );
    }
}
