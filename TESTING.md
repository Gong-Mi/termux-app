# Test policy

This repository contains two implementation surfaces: the legacy Java terminal
implementation and the experimental Rust implementation. Test names and test
file count are not acceptance evidence by themselves. A suite is accepted only
when it is registered, executed, and fails closed.

## Rust test tiers

Run from the repository root:

```bash
bash ./scripts/run-rust-tests.sh core
bash ./scripts/run-rust-tests.sh regressions
bash ./scripts/run-rust-tests.sh terminal
bash ./scripts/run-rust-tests.sh lifecycle
bash ./scripts/run-rust-tests.sh render
bash ./scripts/run-rust-tests.sh perf
bash ./scripts/run-rust-tests.sh all
```

| Tier | Purpose | Evidence boundary |
|---|---|---|
| `core` | parser, VT, key, OSC, selection contracts | Rust host test only |
| `regressions` | resize, reflow, scrollback, content retention | Rust host test only |
| `terminal` | Unicode, CJK, block/box, sixel, color behavior | Rust host test only; not Android pixels |
| `lifecycle` | lock/session/JNI/Surface simulations | Rust host test; simulation unless the test names a production object |
| `render` | Skia/Vulkan renderer experiments | Rust/Skia/Vulkan host test; not a physical Android GPU |
| `perf` | benchmark and stress measurements | measurement only; no correctness claim unless an assertion exists |
| `all` | every Cargo integration target | broad compile/run gate; expensive and platform-sensitive |

The `core` and `regressions` tiers are the default correctness gates. The
renderer and benchmark tiers must not be used to claim Android GPU correctness.

## Required CI behavior

- `cargo fmt -- --check` and `cargo clippy --all-targets --all-features -- -D warnings`
  remain separate static-quality gates in `rust-ci.yml`.
- Correctness tests must use `set -euo pipefail` and must not end with
  `|| true`, `|| echo`, or an unconditional warning path.
- A benchmark that only prints a number is an observation, not a pass/fail
  correctness test.
- A test that catches a missing native library or skips the implementation
  under test must be reported as skipped, not passed.
- CI summaries must include the exact `headSha` and the invoked test command.
- Android ABI builds, Android emulator tests, and physical-device evidence are
  separate gates and must not be collapsed into the Rust host-test result.

## Adding a new test

1. Put deterministic correctness tests in the narrowest applicable tier.
2. Use a fixed seed for generated input and print the seed on failure.
3. Assert state/content/style, not only that a call did not panic.
4. If the test is a benchmark, keep it in `perf` and record repetitions and
   warmups.
5. If the test uses Skia/Vulkan/JNI, state the runtime boundary in its module
   documentation.
6. Add the test name to `run-rust-tests.sh`; do not rely on directory discovery
   as the only registration mechanism.

## Engine ownership slice

See [Engine handle ownership](docs/ENGINE_HANDLE_OWNERSHIP.md) for the token/lease
contract, complete baseline JNI owner mapping, the registered production-registry
tests and the real Kotlin/JNI/PTY harness. Memory lifetime, async delivery, reader
cancellation and actual Surface presentation have separate acceptance boundaries.
