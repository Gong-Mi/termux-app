use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct ProbeEnv {
    prefix: PathBuf,
    home: PathBuf,
    tmpdir: PathBuf,
    ld_preload: Option<PathBuf>,
}

struct Case {
    name: &'static str,
    required: bool,
    run: fn(&ProbeEnv) -> CaseResult,
}

struct CaseResult {
    ok: bool,
    detail: String,
}

fn main() {
    let env_cfg = ProbeEnv {
        prefix: env_path("TERMUX_EXEC_PROBE_PREFIX")
            .unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/usr")),
        home: env_path("TERMUX_EXEC_PROBE_HOME")
            .unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home")),
        tmpdir: env_path("TERMUX_EXEC_PROBE_TMPDIR")
            .unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/usr/tmp")),
        ld_preload: env_path("TERMUX_EXEC_PROBE_LD_PRELOAD").or_else(default_ld_preload),
    };

    println!("termux_exec_device_probe");
    println!("PREFIX={}", env_cfg.prefix.display());
    println!("HOME={}", env_cfg.home.display());
    println!("TMPDIR={}", env_cfg.tmpdir.display());
    println!(
        "LD_PRELOAD={}",
        env_cfg
            .ld_preload
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unset>".to_string())
    );
    println!("PATH_MODE=normal");

    let cases = [
        Case {
            name: "prefix layout",
            required: true,
            run: case_prefix_layout,
        },
        Case {
            name: "sh -c command",
            required: true,
            run: case_shell_command,
        },
        Case {
            name: "termux shebang /usr/bin/env",
            required: true,
            run: case_usr_bin_env_shebang,
        },
        Case {
            name: "termux shebang /bin/sh",
            required: true,
            run: case_bin_sh_shebang,
        },
        Case {
            name: "no shebang shell fallback",
            required: true,
            run: case_no_shebang_fallback,
        },
        Case {
            name: "git local clone",
            required: true,
            run: case_git_local_clone,
        },
        Case {
            name: "python subprocess",
            required: false,
            run: case_python_subprocess,
        },
        Case {
            name: "node child_process",
            required: false,
            run: case_node_child_process,
        },
        Case {
            name: "go os/exec",
            required: false,
            run: case_go_os_exec,
        },
        Case {
            name: "gh executable",
            required: false,
            run: case_gh_version,
        },
        Case {
            name: "gh api network",
            required: false,
            run: case_gh_api_network,
        },
        Case {
            name: "clang self-location",
            required: false,
            run: case_clang_self_location,
        },
        Case {
            name: "proc self exe",
            required: true,
            run: case_proc_self_exe,
        },
    ];

    let mut failed_required = 0usize;
    let mut failed_optional = 0usize;

    for case in cases {
        let result = (case.run)(&env_cfg);
        let status = if result.ok {
            "PASS"
        } else if case.required {
            "FAIL"
        } else {
            "XFAIL"
        };
        println!("{status}: {}", case.name);
        if !result.detail.is_empty() {
            println!("{}", indent(&result.detail));
        }
        if !result.ok && case.required {
            failed_required += 1;
        } else if !result.ok {
            failed_optional += 1;
        }
    }

    println!("summary: failed_required={failed_required} failed_optional={failed_optional}");
    if failed_required == 0 {
        std::process::exit(0);
    }
    std::process::exit(1);
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn default_ld_preload() -> Option<PathBuf> {
    [
        // Prefer bootstrap-installed lib (most up-to-date, already on device)
        "/data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so",
        "/data/data/com.termux/files/usr/lib/libtermux-exec.so",
        // multi-user path (Android 5+)
        "/data/user/0/com.termux/files/usr/lib/libtermux-exec-ld-preload.so",
        "/data/user/0/com.termux/files/usr/lib/libtermux-exec.so",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

fn base_command(env_cfg: &ProbeEnv, program: &str) -> Command {
    let mut command = Command::new(program);
    apply_env(env_cfg, &mut command);
    command
}

fn apply_env(env_cfg: &ProbeEnv, command: &mut Command) {
    let prefix = env_cfg.prefix.to_string_lossy();

    command.env_clear();
    command.env("PREFIX", prefix.as_ref());
    command.env("HOME", &env_cfg.home);
    command.env("TMPDIR", &env_cfg.tmpdir);
    command.env("PATH", format!("{prefix}/bin:/system/bin"));
    command.env("LANG", "C.UTF-8");
    command.env("ANDROID_ROOT", "/system");
    command.env("ANDROID_DATA", "/data");
    if let Some(ld_preload) = &env_cfg.ld_preload {
        command.env("LD_PRELOAD", ld_preload);
        // Also pass LD_LIBRARY_PATH so libc++_shared.so and other APK libs
        // can be found by the dynamic linker in child processes.
        if let Some(lib_dir) = ld_preload.parent() {
            command.env("LD_LIBRARY_PATH", lib_dir);
        }
    }
}

fn run(env_cfg: &ProbeEnv, program: &str, args: &[&str]) -> CaseResult {
    let mut command = base_command(env_cfg, program);
    command.args(args);
    output_to_result(command.output(), program, args, None)
}

fn run_ok(env_cfg: &ProbeEnv, program: &str, args: &[&str], expect: &str) -> CaseResult {
    let mut command = base_command(env_cfg, program);
    command.args(args);
    output_to_result(command.output(), program, args, Some(expect))
}

/// Run an ELF binary that lives under /data/... via linker64 to bypass W^X.
/// Shebang scripts do NOT need this wrapper — they are executed by the shell
/// which is itself already launched via linker64 by the PTY layer.
fn run_elf(env_cfg: &ProbeEnv, elf: &str, args: &[&str], expect: Option<&str>) -> CaseResult {
    const LINKER64: &str = "/system/bin/linker64";
    const LINKER32: &str = "/system/bin/linker";
    let linker = if Path::new(LINKER64).exists() { LINKER64 } else { LINKER32 };

    // Only wrap if the binary lives inside app-data (W^X applies there).
    // System binaries (/system/bin/*) can be exec'd directly.
    let needs_wrap = elf.contains("/com.termux/files/");

    if needs_wrap {
        let mut cmd = base_command(env_cfg, linker);
        cmd.arg(elf);
        cmd.args(args);
        let display_cmd = format!("linker64 {elf}");
        output_to_result(cmd.output(), &display_cmd, args, expect)
    } else {
        let mut cmd = base_command(env_cfg, elf);
        cmd.args(args);
        output_to_result(cmd.output(), elf, args, expect)
    }
}

fn output_to_result(
    output: std::io::Result<Output>,
    program: &str,
    args: &[&str],
    expect: Option<&str>,
) -> CaseResult {
    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let expect_ok = expect.map(|needle| stdout.contains(needle)).unwrap_or(true);
            let ok = output.status.success() && expect_ok;
            CaseResult {
                ok,
                detail: format!(
                    "$ {} {}\nstatus={}\nstdout:\n{}\nstderr:\n{}",
                    program,
                    args.join(" "),
                    output.status,
                    trim_output(&stdout),
                    trim_output(&stderr)
                ),
            }
        }
        Err(err) => CaseResult {
            ok: false,
            detail: format!("$ {} {}\nspawn failed: {err}", program, args.join(" ")),
        },
    }
}

fn case_prefix_layout(env_cfg: &ProbeEnv) -> CaseResult {
    let checks = [
        env_cfg.prefix.join("bin/sh"),
        env_cfg.prefix.join("bin/git"),
        env_cfg.tmpdir.clone(),
    ];
    let missing: Vec<_> = checks.iter().filter(|path| !path.exists()).collect();
    CaseResult {
        ok: missing.is_empty(),
        detail: if missing.is_empty() {
            "required paths exist".to_string()
        } else {
            format!(
                "missing:\n{}",
                missing
                    .into_iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        },
    }
}

fn case_shell_command(env_cfg: &ProbeEnv) -> CaseResult {
    let shell = sh(env_cfg);
    // Shell is an ELF under /data — use linker64 wrapper.
    run_elf(env_cfg, &shell, &["-c", "printf probe-shell-ok"], Some("probe-shell-ok"))
}

fn case_usr_bin_env_shebang(env_cfg: &ProbeEnv) -> CaseResult {
    let script = write_temp(
        env_cfg,
        "env-shebang.py",
        "#!/usr/bin/env python3\nprint('probe-env-shebang-ok')\n",
        0o755,
    );
    match script {
        Ok(script) => run_ok(
            env_cfg,
            script.to_string_lossy().as_ref(),
            &[],
            "probe-env-shebang-ok",
        ),
        Err(err) => fail(err),
    }
}

fn case_bin_sh_shebang(env_cfg: &ProbeEnv) -> CaseResult {
    let script = write_temp(
        env_cfg,
        "bin-sh-shebang",
        "#!/bin/sh\nprintf probe-bin-sh-ok\n",
        0o755,
    );
    match script {
        Ok(script) => run_ok(
            env_cfg,
            script.to_string_lossy().as_ref(),
            &[],
            "probe-bin-sh-ok",
        ),
        Err(err) => fail(err),
    }
}

fn case_no_shebang_fallback(env_cfg: &ProbeEnv) -> CaseResult {
    let script = write_temp(env_cfg, "no-shebang", "printf probe-no-shebang-ok\n", 0o755);
    match script {
        Ok(script) => run_ok(
            env_cfg,
            script.to_string_lossy().as_ref(),
            &[],
            "probe-no-shebang-ok",
        ),
        Err(err) => fail(err),
    }
}

fn case_git_local_clone(env_cfg: &ProbeEnv) -> CaseResult {
    let root = temp_root(env_cfg).join("git-local-clone");
    let source = root.join("source");
    let clone = root.join("clone");
    let _ = fs::remove_dir_all(&root);
    if let Err(err) = fs::create_dir_all(&source) {
        return fail(err);
    }
    if let Err(err) = fs::write(source.join("file.txt"), "probe-git-ok\n") {
        return fail(err);
    }

    let script = format!(
        "set -e\ncd '{}'\ngit init -q\ngit config user.email probe@example.invalid\ngit config user.name Probe\ngit add file.txt\ngit commit -q -m init\ngit clone -q '{}' '{}'\ncat '{}/file.txt'\n",
        shell_quote(&source),
        shell_quote(&source),
        shell_quote(&clone),
        shell_quote(&clone)
    );
    let shell = sh(env_cfg);
    // Shell is an ELF under /data — use linker64 wrapper.
    run_elf(env_cfg, &shell, &["-c", &script], Some("probe-git-ok"))
}

fn case_python_subprocess(env_cfg: &ProbeEnv) -> CaseResult {
    let code = "import subprocess; print(subprocess.check_output(['git','--version']).decode().split()[0])";
    run_ok(env_cfg, "python3", &["-c", code], "git")
}

fn case_node_child_process(env_cfg: &ProbeEnv) -> CaseResult {
    let code = "const cp=require('child_process'); process.stdout.write(cp.execFileSync('git',['--version']).toString().split(' ')[0])";
    run_ok(env_cfg, "node", &["-e", code], "git")
}

fn case_go_os_exec(env_cfg: &ProbeEnv) -> CaseResult {
    let source = write_temp(
        env_cfg,
        "go-os-exec.go",
        r#"package main
import (
  "fmt"
  "os/exec"
)
func main() {
  out, err := exec.Command("git", "--version").CombinedOutput()
  if err != nil { panic(fmt.Sprintf("%v: %s", err, out)) }
  fmt.Print(string(out))
}
"#,
        0o644,
    );
    let Ok(source) = source else {
        return fail(source.unwrap_err());
    };
    run_ok(
        env_cfg,
        "go",
        &["run", source.to_string_lossy().as_ref()],
        "git version",
    )
}

fn case_gh_version(env_cfg: &ProbeEnv) -> CaseResult {
    run_ok(env_cfg, "gh", &["version"], "gh version")
}

fn case_gh_api_network(env_cfg: &ProbeEnv) -> CaseResult {
    run(
        env_cfg,
        "gh",
        &["repo", "view", "octocat/Hello-World", "--json", "name"],
    )
}

fn case_clang_self_location(env_cfg: &ProbeEnv) -> CaseResult {
    let source = write_temp(
        env_cfg,
        "clang-smoke.c",
        "int main(void) { return 0; }\n",
        0o644,
    );
    let Ok(source) = source else {
        return fail(source.unwrap_err());
    };
    let output = temp_root(env_cfg).join("clang-smoke");
    run(
        env_cfg,
        "cc",
        &[
            source.to_string_lossy().as_ref(),
            "-o",
            output.to_string_lossy().as_ref(),
        ],
    )
}

fn case_proc_self_exe(env_cfg: &ProbeEnv) -> CaseResult {
    run(env_cfg, "readlink", &["/proc/self/exe"])
}

fn write_temp(
    env_cfg: &ProbeEnv,
    name: &str,
    content: &str,
    mode: u32,
) -> std::io::Result<PathBuf> {
    let path = temp_root(env_cfg).join(name);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, content)?;
    set_mode(&path, mode)?;
    Ok(path)
}

fn temp_root(env_cfg: &ProbeEnv) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    env_cfg
        .tmpdir
        .join(format!("termux-exec-probe-{}-{stamp}", std::process::id()))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)
}

fn sh(env_cfg: &ProbeEnv) -> String {
    env_cfg
        .prefix
        .join("bin/sh")
        .to_str()
        .unwrap_or("/data/data/com.termux/files/usr/bin/sh")
        .to_string()
}

fn shell_quote(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "'\\''")
}

fn trim_output(s: &str) -> String {
    const LIMIT: usize = 2000;
    if s.len() <= LIMIT {
        s.to_string()
    } else {
        format!("{}...<truncated {} bytes>", &s[..LIMIT], s.len() - LIMIT)
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fail(err: impl std::fmt::Display) -> CaseResult {
    CaseResult {
        ok: false,
        detail: err.to_string(),
    }
}
