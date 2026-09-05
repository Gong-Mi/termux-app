# Test/build rectification acceptance ledger

## Scope

Continue PR8 on `fix/skia-vulkan-api-contract`; do not merge the stacked PRs.
Production Rust/Kotlin behavior is unchanged by this test/build-only batch.

- Preserve all seven explicit Rust tiers and all 74 integration targets (checked
  against Cargo metadata, including auto-discovery). Submit one Cargo command per
  tier instead of one per target. The four automatic tiers previously invoked
  Cargo 10/8/16/12 times; each now invokes it once.
- Keep test binaries serial and `--test-threads=1`. Cargo now owns binary order.
  Runtime failure does not hide later binaries (`--no-fail-fast`), and the final
  result remains nonzero. Compile failures still block runtime execution.
- Replace the host correctness OS+lockfile whole-target cache with rust-toolchain/
  native-environment-aware dependency caching, per tier; add manual-tier caching.
- Add separate emulator native dependency caching keyed by actual NDK identity,
  x86_64, API26/Skia35, release profile and experimental feature. Preserve the
  existing Gradle dependency cache. Never skip build/A-B gates on a cache hit.
- Keep all four Android ABI builds and existing static/JNI/SwiftShader gates.

## Local evidence for this batch

- Runner RED: all seven tiers made multiple Cargo invocations; the same exact
  target/argument contracts pass after batching.
- Six runner tests include exact coverage, default/invalid tier, status
  propagation and a real dependency-free Cargo fixture: first binary fails,
  later binary runs, overall exit 101. Fake-Cargo checks are not app acceptance.
- All 35 Python discovery tests pass (runner plus existing A/B oracle tests).
- Cache/topology contracts 2, Skia wiring 2, Android CI contracts 4 pass.
- NDK identity fixture hashes actual source.properties and exports selected
  path; absent NDK fails closed. Workflow YAML and shell syntax checks pass.
- Actual production `core`, `regressions`, `terminal`, `lifecycle` tiers pass
  with the new runner on the Termux host. Prior intermittent std-only backpressure
  failure is not declared fixed; no Rust assertion or production code changed.

## Verified predecessor: 3af5466eba1793367278eb35cf85e09e2fa625bc

Downloaded run 33973030241 artifacts; source-identity matches the exact head
(event/merge SHA d64f743b9e436ee7379cb7a47dc8cf6bd2a8121e is separately recorded).
Same installed APK, property-only A/B:

- Baseline PID2665: created Vulkan 1.1, Skia max=0; null device procs include
  vkCreateRenderPass2/vkCmdBindVertexBuffers2; `Skia make_vulkan failed` reproduced.
- Capped PID3116: max=actual created API; Skia context initialized,
  `SKIA_BACKEND_READBACK: PASS`, `RenderThread: Frame 0 completed`.
- Original empty property restored and read back. No cross-PID marker stitching.

This supports the API-cap defect as the causal condition in this SwiftShader
experiment, not a claim that Vulkan is absent. It does not establish physical
GPU behavior, visible terminal text correctness, or sustained interactive performance.

Other exact-head results: correctness and Kotlin/real JNI construction succeed;
all four Android ABI builds, rustfmt and rustdoc succeed. Clippy still fails with
143 source diagnostics, counted from actual job101324799549 log; no blanket allow.

## Cost evidence and next acceptance

Predecessor emulator `Build debug APKs` step was 562 seconds (GitHub job timestamps).
Rust Android build steps: aarch64 67s, x86_64 67s, i686 55s, armv7 421s. These are
baseline observations, not a controlled comparison or predicted savings.

The next exact-head CI must verify the new cache restore/save path, unchanged
production tests and same-APK A/B. First use of each new key is cold; only later
warm runs can demonstrate cache speedup. Do not launch extra full builds only
for a cache benchmark. Check normal subsequent authorized runs instead.

Open production lines remain: IO cancellation/partial writes, process identity
and exit/drain, Kotlin delivery/final-screen retention, session/Surface generation,
extension metadata, old test oracle/Clippy debt and full project inventory.
