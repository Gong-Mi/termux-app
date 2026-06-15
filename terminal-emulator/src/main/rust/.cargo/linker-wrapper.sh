#!/data/data/com.termux/files/usr/bin/bash
# --sysroot not needed — DEFAULT_SYSROOT is baked into the system clang-23
exec /data/data/com.termux/files/usr/bin/clang "$@"
