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

### Runner batching and failure domains

Each tier submits its unchanged explicit target list in one locked Cargo command,
not one Cargo invocation per target. Cargo owns binary execution order (the list
is not an inter-test ordering contract); test binaries remain serial, and each
binary retains `--test-threads=1`. `--no-fail-fast` runs remaining binaries after a
runtime test failure, while preserving Cargo's nonzero exit status. Compilation
failure still prevents execution; it is not a completed runtime test suite.
No new test filters, ignored tests, parallel test execution or feature changes
are introduced. This removes repeated Cargo setup/dependency checks, not the
initial compilation of each distinct target. Measured CI savings remain separate
from this command-count reduction.

`python3 scripts/tests/test_rust_test_runner.py` checks all seven tier command
contracts, failure propagation and the complete `all` inventory against actual
Cargo metadata (including auto-discovered targets). The frozen inventory in
`scripts/tests/rust-tier-targets.json` must be updated deliberately with the runner
when registering a target. These process contracts use fake Cargo for invocation
checks, not as evidence that production Rust tests passed. A dependency-free real
Cargo fixture additionally proves a failed binary returns exit 101 while the
later binary still executes.

The host correctness/manual workflows cache dependencies with rustc/Cargo and
native-environment identity, separated by tier. The emulator separately caches
forced-source Skia dependencies for x86_64, API26/Skia35, release Rust profile and
`skia-api-experiment`, keyed by the actual cargo-ndk NDK's `source.properties`.
This is not the default-feature Android cross-build cache. Cache hits never skip
compilation checks, APK construction or A/B acceptance. Native-cache first runs
are cold; measure restore/save overhead and later warm runs before claiming a
speedup. `python3 scripts/verify-test-build-ci.py` validates these static contracts
(requires PyYAML).

## Known-child process owner (C1)

`process_owner` and `session_process_lifecycle` are registered in lifecycle/all.
They exercise real subprocess exit/reaping, retained status, concurrent wait/kill,
forced old-kernel fallback, unregister/late binding, and input rejection while
an independently held IO peer can still deliver tail bytes. The numeric-label
injection test is not actual kernel PID reuse; pidfd availability must be read
from test output rather than inferred from a passing fallback case.

`python3 scripts/verify-process-owner-boundary.py` verifies the static native/JNI/
Kotlin wiring. It is not proof of kernel syscall execution or ART interaction.
Process exit does not imply reader EOF, full output drain, UI completion, final
frame presentation or disposal; those remain separate acceptance work.

## Independent process/IO observations (C2a)

`session_completion_observation` exercises actual child processes and context IO
in both event orders: process exit with a still-open IO peer, and IO EOF with a
still-running child. Cancelled remains distinct after join. Neither an absent
process nor unstarted IO counts as completed. `IoObserver` retains terminal data
only, not fds or a thread/context owner, and observes close plus queue cleanup;
`on_stop` reporting and thread join are later boundaries.

`RustTerminal.getCompletionStatus(handle)` returns independent monotonic facts,
not an atomic cross-source snapshot or a UI completion acknowledgement. The four
fields are process kind/code and IO kind/code. Invalid or revoked handles return
null; a retained engine preserves the outcome after its runtime was removed.
No production UI callback, automatic grace deadline or disposal policy is enabled
by this observation interface.

`utf8_streaming` in core/all verifies 2/3/4-byte split points, byte-at-a-time input,
control/CSI/OSC boundaries and malformed-byte consumption. Parser pending storage
is bounded to one scalar. Incomplete UTF-8 remains pending across advance calls;
there is no new EOF flush policy. The SVE scanner is retained with bounded windows
and validated complete UTF-8 prefixes. The scanner advances only active bytes
and consumes its predicate within one asm block; this repair is not a measured
performance claim. CI x86 scalar and local ARM/SVE evidence remain separate.

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

## PTY transport slice

See [PTY IO lifecycle](docs/PTY_IO_LIFECYCLE.md). `pty_io_runtime` and
`pty_context_integration` are registered in `lifecycle` and `all`; they exercise
actual PTY/socket syscalls, production parsing, response queueing and background
join. They do not validate process exit/UI completion or a physical GPU.
`python3 scripts/verify-pty-io-boundary.py` guards the full production JNI/Kotlin
fd handoff. Existing Kotlin/JNI handle tests also check input admission statuses
and both legacy/status-returning input methods against a real shell.

## Engine ownership slice

See [Engine handle ownership](docs/ENGINE_HANDLE_OWNERSHIP.md) for the token/lease
contract, complete baseline JNI owner mapping, the registered production-registry
tests and the real Kotlin/JNI/PTY harness. Memory lifetime, async delivery, reader
cancellation and actual Surface presentation have separate acceptance boundaries.
