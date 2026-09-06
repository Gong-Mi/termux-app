//! Preserve the caller's environment; supply only missing PATH and an available
//! exec hook. In particular, never force LD_LIBRARY_PATH or rewrite arbitrary values.

pub(crate) fn prepare(env: &[String], prefix: &str) -> Vec<String> {
    let hook = std::path::Path::new(prefix).join("lib/libtermux-exec.so");
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
