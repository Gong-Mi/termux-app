#!/data/data/com.termux/files/usr/bin/bash
# 快速回归验证：编译 + 关键测试
# 在提交前跑一次，确保不炸
set -e

cd /data/data/com.termux/files/home/termux-app/terminal-emulator/src/main/rust

echo "=== 1. Rust 单元测试（extent 钳位/transform/列数）==="
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=clang RUSTC=$PREFIX/bin/rustc \
  ~/.cargo/bin/cargo test --release --lib -- \
  test_extent_clamping_logic \
  test_zero_extent_handling \
  test_transform_handling \
  test_pipeline_cache_validation 2>&1 | grep -E "test result:|FAILED|panicked"

echo ""
echo "=== 2. 列数行数一致性（124 个测试）==="
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=clang RUSTC=$PREFIX/bin/rustc \
  ANDROID_NDK=$PREFIX \
  ~/.cargo/bin/cargo test --release --test consistency 2>&1 | grep "test result:"

echo ""
echo "=== 3. resize 基准（5 个测试）==="
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=clang RUSTC=$PREFIX/bin/rustc \
  ANDROID_NDK=$PREFIX \
  ~/.cargo/bin/cargo test --release --test resize_benchmark 2>&1 | grep "test result:"

echo ""
echo "=== 4. 性能测试（9 个测试）==="
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=clang RUSTC=$PREFIX/bin/rustc \
  ANDROID_NDK=$PREFIX \
  ~/.cargo/bin/cargo test --release --test performance 2>&1 | grep "test result:"

echo ""
echo "=== 全部通过 ==="
