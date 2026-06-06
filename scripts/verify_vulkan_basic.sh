#!/data/data/com.termux/files/usr/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_DIR="$ROOT/terminal-emulator/src/main/rust"

cd "$RUST_DIR"

echo "== Vulkan basic gate =="
echo "repo: $ROOT"
echo "rust dir: $RUST_DIR"
echo

tests=(
  skia_basic_test
  skia_render_test
  vulkan_10bit_test
  vulkan_format_probe
  vulkan_render_benchmark
)

for test_name in "${tests[@]}"; do
  echo
  echo "== cargo test --test $test_name --features test-helpers -- =="
  cargo test --test "$test_name" --features test-helpers --
done

echo
echo "Vulkan basic gate passed."
