#!/system/bin/sh
# Runs only in the CI emulator's app-owned prefix, through the production PTY.
set -u
: "${PREFIX:?missing prefix}"
: "${CASE_DIR:?missing evidence directory}"
export DEBIAN_FRONTEND=noninteractive

"$PREFIX/bin/apt-get" -o Acquire::Retries=2 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30 -o APT::Update::Error-Mode=any update >"$CASE_DIR/apt-update.log" 2>&1
status=$?
if [ "$status" -ne 0 ]; then
    /system/bin/toybox tail -n 60 "$CASE_DIR/apt-update.log"
    exit 94
fi

"$PREFIX/bin/apt-get" -y -o Acquire::Retries=2 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30 -o Dpkg::Options::=--force-confold install python >"$CASE_DIR/apt-install.log" 2>&1
status=$?
if [ "$status" -ne 0 ]; then
    /system/bin/toybox tail -n 60 "$CASE_DIR/apt-install.log"
    exit 95
fi

"$PREFIX/bin/dpkg-query" -W '-f=${Status}\t${Version}\n' python >"$CASE_DIR/python-package.txt" 2>&1 || exit 96
"$PREFIX/bin/python" "$CASE_DIR/python-subprocess-probe.py" >"$CASE_DIR/python-result.json" 2>"$CASE_DIR/python-stderr.log"
status=$?
if [ "$status" -ne 0 ]; then
    /system/bin/toybox cat "$CASE_DIR/python-stderr.log"
    exit 97
fi
/system/bin/toybox cat "$CASE_DIR/python-result.json" || exit 98
"$PREFIX/bin/printf" 'python-subprocess-ok\n' || exit 99
