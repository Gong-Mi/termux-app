#!/bin/bash
set -e

echo "Building Rust engine..."
cd terminal-emulator/src/main/rust
cargo build --release --target aarch64-linux-android
cd ../rust-exec
cargo build --release --target aarch64-linux-android
cp target/aarch64-linux-android/release/libtermux_exec.so ../jniLibs/arm64-v8a/libtermux-exec.so
cd ../../../..

echo "Building Android APK..."
./gradlew assembleDebug

echo "Build successful."
