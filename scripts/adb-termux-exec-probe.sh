#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE_DIR="$ROOT_DIR/terminal-emulator/src/main/rust-exec"
PACKAGE="${TERMUX_EXEC_PROBE_PACKAGE:-com.termux}"
REMOTE_TMP="/data/local/tmp/termux_exec_device_probe"
REMOTE_APP="files/home/termux_exec_device_probe"
MIN_SDK="${TERMUX_EXEC_PROBE_MIN_SDK:-24}"

abi="$(adb shell getprop ro.product.cpu.abi | tr -d '\r')"
case "$abi" in
  arm64-v8a) target="aarch64-linux-android" ;;
  armeabi-v7a) target="armv7-linux-androideabi" ;;
  x86) target="i686-linux-android" ;;
  x86_64) target="x86_64-linux-android" ;;
  *)
    echo "Unsupported device ABI: $abi" >&2
    exit 2
    ;;
esac

ndk_dir="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [[ -z "$ndk_dir" && -n "${ANDROID_HOME:-}" ]]; then
  ndk_dir="$(find "$ANDROID_HOME/ndk" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1 || true)"
fi
if [[ -z "$ndk_dir" || ! -d "$ndk_dir" ]]; then
  echo "Android NDK not found. Set ANDROID_NDK_HOME or ANDROID_NDK_ROOT." >&2
  exit 2
fi

toolchain="$ndk_dir/toolchains/llvm/prebuilt/linux-x86_64/bin"
case "$target" in
  aarch64-linux-android)
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$toolchain/aarch64-linux-android${MIN_SDK}-clang"
    ;;
  armv7-linux-androideabi)
    export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$toolchain/armv7a-linux-androideabi${MIN_SDK}-clang"
    ;;
  i686-linux-android)
    export CARGO_TARGET_I686_LINUX_ANDROID_LINKER="$toolchain/i686-linux-android${MIN_SDK}-clang"
    ;;
  x86_64-linux-android)
    export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$toolchain/x86_64-linux-android${MIN_SDK}-clang"
    ;;
esac

echo "Building termux_exec_device_probe for $target"
cargo build --manifest-path "$CRATE_DIR/Cargo.toml" --target "$target" --bin termux_exec_device_probe

binary="$CRATE_DIR/target/$target/debug/termux_exec_device_probe"
if [[ ! -f "$binary" ]]; then
  echo "Built binary not found: $binary" >&2
  exit 2
fi

echo "Pushing probe to device"
adb push "$binary" "$REMOTE_TMP" >/dev/null
adb shell chmod 755 "$REMOTE_TMP"

echo "Copying probe into $PACKAGE app data"
adb shell run-as "$PACKAGE" sh -c "'cp $REMOTE_TMP $REMOTE_APP && chmod 700 $REMOTE_APP'"

echo "Running probe as $PACKAGE"
adb shell run-as "$PACKAGE" sh -c "'
PREFIX=/data/data/$PACKAGE/files/usr
HOME=/data/data/$PACKAGE/files/home
TMPDIR=/data/data/$PACKAGE/files/usr/tmp
LD_PRELOAD_PATH=
for candidate in \
  \"\$PREFIX/lib/libtermux-exec-ld-preload.so\" \
  \"\$PREFIX/lib/libtermux-exec.so\" \
  \"\$PREFIX/lib/libtermux_exec.so\"; do
  if [ -f \"\$candidate\" ]; then
    LD_PRELOAD_PATH=\"\$candidate\"
    break
  fi
done
if [ -n \"\$LD_PRELOAD_PATH\" ]; then
  export LD_PRELOAD=\"\$LD_PRELOAD_PATH\"
fi
export PREFIX HOME TMPDIR
./$REMOTE_APP
'"
