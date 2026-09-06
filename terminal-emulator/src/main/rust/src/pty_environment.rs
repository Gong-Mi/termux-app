//! Preserve the caller's environment; supply only missing PATH and an available
//! exec hook. In particular, never force LD_LIBRARY_PATH or rewrite arbitrary values.

pub(crate) fn prepare(env: &[String], prefix: &str) -> Vec<String> {
    // termux-exec 2.x ships separate direct/linker builds; bootstrap's primary
    // copy can still be direct until postinst runs. Match its setup policy without
    // mutating the bootstrap or overriding an explicit non-empty LD_PRELOAD.
    let lib = std::path::Path::new(prefix).join("lib");
    let preferred = lib.join(if platform_linker_required(env) {
        "libtermux-exec-linker-ld-preload.so"
    } else {
        "libtermux-exec-direct-ld-preload.so"
    });
    let hook = if preferred.is_file() { preferred } else { lib.join("libtermux-exec.so") };
    let hook = if hook.is_file() {
        Some(
            hook.canonicalize()
                .unwrap_or(hook)
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    };
    apply(env, prefix, hook)
}

// Matches bootstrap bin/termux-exec-system-linker-exec (2.4.0): disable,
// force on API>=29, otherwise exempt only unavailable SELinux or app_25/app_27.
fn linker_required(mode: &str, api: u32, context: &str) -> bool {
    if api < 29 || mode == "disable" { return false; }
    if mode == "force" { return true; }
    !context.trim_matches(['\0', '\n', ' ']).is_empty()
        && !context.starts_with("u:r:untrusted_app_25:")
        && !context.starts_with("u:r:untrusted_app_27:")
}

fn platform_linker_required(env: &[String]) -> bool {
    let mode = env.iter().rev().find_map(|entry| entry.strip_prefix("TERMUX_EXEC__SYSTEM_LINKER_EXEC__MODE=")).unwrap_or("enable");
    #[cfg(target_os = "android")]
    let api = {
        unsafe extern "C" { fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> i32; }
        let mut value = [0 as std::ffi::c_char; 92];
        let n = unsafe { __system_property_get(c"ro.build.version.sdk".as_ptr(), value.as_mut_ptr()) };
        if n > 0 { unsafe { std::ffi::CStr::from_ptr(value.as_ptr()) }.to_str().ok().and_then(|s| s.parse().ok()).unwrap_or(0) } else { 0 }
    };
    #[cfg(not(target_os = "android"))]
    let api = 0;
    let context = std::fs::read_to_string("/proc/self/attr/current").unwrap_or_default();
    linker_required(mode, api, &context)
}

fn apply(env: &[String], prefix: &str, available_hook: Option<String>) -> Vec<String> {
    let mut result = env.to_vec();
    if !result.iter().any(|entry| entry.starts_with("PATH=")) {
        result.push(format!("PATH={prefix}/bin:/system/bin:/system/xbin"));
    }
    // putenv applies entries in order. Respect the effective LAST value, not
    // an earlier non-empty duplicate that the caller later replaced with empty.
    let preload = result
        .iter()
        .rposition(|entry| entry.starts_with("LD_PRELOAD="));
    if let Some(hook) = available_hook {
        match preload {
            Some(index) if result[index] == "LD_PRELOAD=" => {
                result[index] = format!("LD_PRELOAD={hook}")
            }
            None => result.push(format!("LD_PRELOAD={hook}")),
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entries(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn linker_variant_selection_matches_android_and_selinux_policy() {
        assert!(!linker_required("enable", 28, "u:r:untrusted_app_30:s0"));
        assert!(!linker_required("enable", 36, "u:r:untrusted_app_25:s0"));
        assert!(!linker_required("enable", 36, "u:r:untrusted_app_27:s0"));
        assert!(linker_required("enable", 36, "u:r:untrusted_app_30:s0"));
        assert!(!linker_required("enable", 36, ""));
        assert!(linker_required("force", 36, ""));
        assert!(!linker_required("disable", 36, "u:r:untrusted_app_30:s0"));
    }

    #[test]
    fn preserves_supplied_values_including_empty_and_non_path_payload() {
        let env = entries(&[
            "PATH=",
            "LD_LIBRARY_PATH=/chosen/lib",
            "LD_PRELOAD=/chosen/hook.so",
            "PAYLOAD=literal:/data/data/com.termux/example",
        ]);
        assert_eq!(
            apply(&env, "/different/prefix", Some("/resolved/exec.so".into())),
            env
        );
    }

    #[test]
    fn available_hook_fills_absent_or_empty_preload_without_injecting_library_path() {
        for input in [
            entries(&["PATH=/system/bin"]),
            entries(&["PATH=/system/bin", "LD_PRELOAD="]),
        ] {
            let output = apply(&input, "/prefix", Some("/resolved/exec.so".into()));
            assert_eq!(
                output,
                entries(&["PATH=/system/bin", "LD_PRELOAD=/resolved/exec.so"])
            );
            assert!(
                !output
                    .iter()
                    .any(|entry| entry.starts_with("LD_LIBRARY_PATH="))
            );
        }
    }

    #[test]
    fn unavailable_hook_preserves_absence_and_empty_preload() {
        for input in [
            entries(&["PATH=/system/bin"]),
            entries(&["PATH=/system/bin", "LD_PRELOAD="]),
        ] {
            assert_eq!(apply(&input, "/prefix", None), input);
        }
        let missing = prepare(
            &entries(&["PATH=/system/bin"]),
            "/definitely-missing-termux-prefix",
        );
        assert_eq!(missing, entries(&["PATH=/system/bin"]));
    }

    #[test]
    fn supplies_default_path_only_when_absent() {
        assert_eq!(
            apply(&[], "/prefix", None),
            entries(&["PATH=/prefix/bin:/system/bin:/system/xbin"])
        );
        for path in ["PATH=", "PATH=/system/bin", "PATH=/custom:/prefix/bin"] {
            let env = entries(&[path]);
            assert_eq!(apply(&env, "/prefix", None), env);
        }
    }

    #[test]
    fn last_preload_entry_controls_empty_fallback() {
        let hook = Some("/resolved/exec.so".into());
        let env = entries(&["PATH=/system/bin", "LD_PRELOAD=/first", "LD_PRELOAD="]);
        assert_eq!(
            apply(&env, "/prefix", hook.clone()),
            entries(&[
                "PATH=/system/bin",
                "LD_PRELOAD=/first",
                "LD_PRELOAD=/resolved/exec.so"
            ])
        );
        let env = entries(&["PATH=/system/bin", "LD_PRELOAD=", "LD_PRELOAD=/last"]);
        assert_eq!(apply(&env, "/prefix", hook), env);
    }
}
