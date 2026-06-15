#!/data/data/com.termux/files/usr/bin/bash
set -u
fail=0
check() {
  name="$1"; shift
  printf '\n## %s\n' "$name"
  if "$@"; then echo "PASS:$name"; else rc=$?; echo "FAIL:$name rc=$rc"; fail=$((fail+1)); fi
}

echo REGTEST_START
echo "uid=$(id)"
echo "PREFIX=${PREFIX:-}"
echo "HOME=${HOME:-}"
echo "PATH=${PATH:-}"
echo "LD_PRELOAD=${LD_PRELOAD:-}"
command -v bash || true
command -v sh || true
command -v env || true
command -v pkg || true
command -v apt || true
command -v python || true

check bash_login bash -lc 'echo bash-login-ok'
check dash_sh sh -c 'echo sh-ok'
check system_sh /system/bin/sh -c 'echo system-sh-ok'
check toybox /system/bin/toybox true
check env_exec env true
check nested_bash bash -lc 'sh -c "echo nested-sh-ok" && /system/bin/sh -c "echo nested-system-sh-ok"'
check python_child python - <<'PY'
import subprocess, os
print('python-ok')
print(subprocess.check_output(['sh','-c','echo child-sh-ok']).decode().strip())
print(subprocess.check_output(['/system/bin/sh','-c','echo system-child-ok']).decode().strip())
PY

cat > "$HOME/regtest-shebang.sh" <<'SH'
#!/data/data/com.termux/files/usr/bin/sh
echo shebang-ok
/system/bin/sh -c 'echo shebang-system-child-ok'
SH
chmod 700 "$HOME/regtest-shebang.sh"
check shebang "$HOME/regtest-shebang.sh"

cat > "$HOME/regtest-env-shebang.py" <<'PY'
#!/data/data/com.termux/files/usr/bin/env python
print('env-shebang-python-ok')
PY
chmod 700 "$HOME/regtest-env-shebang.py"
check env_shebang "$HOME/regtest-env-shebang.py"

# Package commands: only check startup/help to avoid network side effects.
check apt_version apt --version
check pkg_help pkg --help

echo "REGTEST_FAIL_COUNT=$fail"
echo REGTEST_END
exit "$fail"
