#!/bin/bash
set -e

echo "Building Rust engine..."
cd terminal-emulator/src/main/rust
cargo build --release --target aarch64-linux-android
cd ../../../..

echo "Building Android APK..."
./gradlew assembleDebug

echo "Build successful."
