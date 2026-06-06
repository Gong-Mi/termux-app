#!/data/data/com.termux/files/usr/bin/bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_DIR="$ROOT/terminal-emulator/src/main/rust"

cd "$RUST_DIR"

echo "== Vulkan experimental/HDR gate =="
echo "This script records failures but does not stop at first failure."
echo "Device-dependent Skia/Vulkan surface creation failures are possible."
echo "Scalar/vector channel mismatches are algorithmic bugs and should be fixed."
echo

tests=(
  hdr_pipeline_integrity
  vulkan_gamma_test
  vulkan_hdr_simulation
  vulkan_hdr_verification
  vulkan_memory_stress_test
)

failed=()
for test_name in "${tests[@]}"; do
  echo
  echo "== cargo test --test $test_name --features test-helpers -- =="
  if cargo test --test "$test_name" --features test-helpers --; then
    echo "PASS: $test_name"
  else
    echo "FAIL: $test_name"
    failed+=("$test_name")
  fi
done

echo
if [ "${#failed[@]}" -eq 0 ]; then
  echo "Vulkan experimental/HDR gate passed."
  exit 0
fi

echo "Vulkan experimental/HDR failures:"
printf ' - %s\n' "${failed[@]}"
echo
echo "This gate is informational unless the touched change claims to fix HDR/colorspace/memory stress behavior."
exit 1
