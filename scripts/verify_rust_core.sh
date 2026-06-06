#!/data/data/com.termux/files/usr/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_DIR="$ROOT/terminal-emulator/src/main/rust"

cd "$RUST_DIR"

echo "== Rust core gate =="
echo "repo: $ROOT"
echo "rust dir: $RUST_DIR"
echo

echo "-- cargo test --lib --features test-helpers --"
cargo test --lib --features test-helpers --

echo
echo "-- targeted integration tests --"
tests=(
  child_watcher_regression
  color_pipeline_test
  consistency
  key_event_handling
  performance
  reflow_600_lines
  reflow_stress
  reflow_trap_repro
  render_deadlock_test
  render_params_batching
  resize_benchmark
  resize_column_change
  resize_history_bug
  resize_zoom_simulation
  selection_pipeline_test
  session_coordinator_test
  state_incremental_push
  vt_compatibility
)

for test_name in "${tests[@]}"; do
  echo
  echo "== cargo test --test $test_name --features test-helpers -- =="
  cargo test --test "$test_name" --features test-helpers --
done

echo
echo "Rust core gate passed."
