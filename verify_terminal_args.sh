#!/data/data/com.termux/files/usr/bin/bash
# verify_terminal_args.sh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$ROOT_DIR/terminal-emulator/src/main/rust-exec"
PACKAGE="com.termux"
REMOTE_TMP="/data/local/tmp/verify_args"

# 1. 自动检测设备 ABI
echo "[*] Detecting device ABI..."
abi=$(adb shell getprop ro.product.cpu.abi | tr -d '\r')
case "$abi" in
  arm64-v8a) target="aarch64-linux-android" ;;
  armeabi-v7a) target="armv7-linux-androideabi" ;;
  x86) target="i686-linux-android" ;;
  x86_64) target="x86_64-linux-android" ;;
  *) echo "Error: Unsupported ABI $abi"; exit 1 ;;
esac
echo "Target: $target"

# 2. 编译 Rust 验证工具
echo "[*] Building verify_args for $target..."
# 假设环境变量已经配置好 NDK，如果没有，这里可能会失败，
# 但在开发机环境下通常是通过全局配置好的。
cargo build --manifest-path "$CRATE_DIR/Cargo.toml" --target "$target" --bin verify_args

BINARY="$CRATE_DIR/target/$target/debug/verify_args"

# 3. 推送并设置权限
echo "[*] Pushing binary to device..."
adb push "$BINARY" "$REMOTE_TMP"
adb shell chmod 755 "$REMOTE_TMP"

# 4. 执行多组“极限”参数测试
echo "[*] Running Argument Interpretation Tests..."

# 预先将程序拷贝到 App 私有目录，解决 run-as 对 /data/local/tmp 的权限限制
# 使用更加鲁棒的引号处理
adb shell "run-as $PACKAGE sh -c 'cp $REMOTE_TMP ./verify_args_test && chmod 700 ./verify_args_test'"

run_test() {
    local title="$1"
    shift
    echo -e "\n>>> TEST: $title"
    # 使用 ./verify_args_test 执行私有目录下的副本
    adb shell run-as "$PACKAGE" ./verify_args_test "$@"
}

# 测试 A：普通空格
run_test "Spaces" "arg 1" "arg 2 with many spaces"

# 测试 B：特殊符号（Shell 敏感字符）
run_test "Special Characters" "'; rm -rf /'" '$PATH' '`id`' '&' '|' '>'

# 测试 C：引号嵌套
run_test "Nested Quotes" '"single" inside double' "'double' inside single"

# 测试 D：Unicode / Emoji
run_test "Unicode" "🚀 Termux" "你好世界" "π ≈ 3.14"

# 测试 E：空参数与长参数
run_test "Empty & Long" "" "A$(printf '%.0sB' {1..100})C"

echo -e "\n[*] Verification Complete."
echo "If all 'argv[x]' values above exactly match the input strings (no split, no expansion),"
echo "then the terminal argument interpretation is indeed unassailable."
