# Termux App Rustification Project Normalization

This document is the local operating manual for this fork. Its goal is to stop the project from becoming a pile of heroic one-off fixes.

The project is not "just Termux with some Rust". The current branch is a Rustification fork of Termux App: the terminal hot path is being moved from Java/Kotlin into Rust, and the high-throughput rendering path is Rust + Skia + Vulkan.

## 0. Current branch reality

Repository:

- `/data/user/0/com.termux/files/home/termux-app`

Main working branch at the time this manual was written:

- `work/googleplay-base`

Important nearby branches:

- `feature/rust-integration`: contains the Vulkan GPU synchronization fix commit.
- `rust-integration-refactored`: contains other Rust/terminal-view fixes.

Do not assume a fix exists on the current branch just because it exists in another local branch. Always check the exact branch before claiming a bug is fixed.

## 1. Ownership boundary

### Java/Kotlin should own

- Activity lifecycle
- permissions
- settings UI
- IME / keyboard integration
- extra keys
- Android `SurfaceView` / `SurfaceHolder.Callback`
- coarse JNI calls into Rust
- low-frequency clipboard / selection / configuration glue

### Rust should own

- PTY batching and session-side hot path
- VTE escape parsing
- terminal state machine
- screen grid
- scrollback
- resize and reflow
- dirty row / dirty rect tracking
- render snapshots
- glyph/style batching
- render scheduling
- Skia/Vulkan context and swapchain lifecycle
- GPU synchronization

### Forbidden drift

Avoid reintroducing these patterns:

- per-byte JNI callbacks
- per-cell JNI rendering
- Java-side full-screen reflow
- Java-side screen truth that competes with Rust state
- high-frequency `View.invalidate()` as the primary terminal renderer
- treating Vulkan as an optional decorative backend while Java still owns the terminal hot path

## 2. Change categories

Every non-trivial change should belong to exactly one category.

### A. Rust terminal correctness

Examples:

- VTE parser behavior
- screen grid behavior
- scrollback
- selection correctness
- wcwidth
- color/style state
- sixel behavior

Required local gate:

```sh
scripts/verify_rust_core.sh
```

### B. Resize / reflow / performance

Examples:

- font zoom
- column count changes
- large scrollback reflow
- high-output CLI bursts
- active transcript behavior

Required local gate:

```sh
scripts/verify_rust_core.sh
```

If the change is specifically performance-sensitive, also record before/after numbers from the relevant benchmark test.

### C. Vulkan / Skia rendering

Examples:

- `vulkan_context.rs`
- `render_thread.rs`
- swapchain recreation
- Skia surface/context creation
- present mode / image format / color space
- GPU synchronization

Required local gate:

```sh
scripts/verify_vulkan_basic.sh
```

If touching synchronization, also compare against the fix in `feature/rust-integration` and verify the current branch has:

- `in_flight_fence`
- fence created with `SIGNALED`
- finite fence wait timeout
- finite `acquire_next_image` timeout
- submit signals `render_finished_semaphore`
- present waits on `render_finished_semaphore`

### D. Android/JNI lifecycle

Examples:

- `TerminalView.kt`
- `RustTerminal.kt`
- surface attach/detach
- native callbacks into Java
- thread parking
- lock strategy around JNI entry points

Required rules:

- Main thread JNI entry points must not block forever.
- Prefer `try_lock` + short bounded retry over unbounded `.lock()`.
- No Java callback while holding a Rust lock.
- Render thread must park when surface is unavailable.
- Surface loss must not become ANR.

Required local gate:

```sh
scripts/verify_rust_core.sh
scripts/verify_vulkan_basic.sh
```

Then perform an actual Android build/device run when Java/Gradle are available.

### E. Gradle / Android build chain

Examples:

- `gradle.properties`
- AGP version
- compileSdk / targetSdk / minSdk
- NDK version
- cargo-ndk integration
- APK packaging

Required checks:

- `targetSdkVersion` must not exceed `compileSdkVersion` unless there is a documented reason and a verified build.
- Rust `.so` files must be built before JNI folders are merged.
- Java/Gradle availability must be verified before claiming Android build success.

When Java is available:

```sh
java -version
./gradlew :terminal-emulator:buildAllRust
./gradlew assembleDebug
```

## 3. Branch discipline

Before modifying code:

```sh
git branch --show-current
git status --short
git log --oneline -8
```

If borrowing a fix from another local branch:

```sh
git diff work/googleplay-base..feature/rust-integration -- path/to/file
```

Do not blindly merge a whole branch if the current branch is carrying unrelated rust-exec / Google Play work. Prefer targeted cherry-pick or manual patch for isolated fixes.

## 4. Verification layers

Use these layers instead of one giant ambiguous `cargo test`.

### Layer 1: Rust core gate

```sh
scripts/verify_rust_core.sh
```

This should be the default gate for parser/screen/reflow/JNI-state changes.

### Layer 2: Vulkan basic gate

```sh
scripts/verify_vulkan_basic.sh
```

This checks the currently usable Vulkan/Skia tests that should not be treated as experimental HDR work.

### Layer 3: Experimental Vulkan/HDR gate

```sh
scripts/verify_vulkan_experimental.sh
```

This is allowed to fail for device-dependent HDR/Skia surface reasons, but failures must be recorded. Algorithmic mismatches are real bugs and should not be dismissed as device issues.

### Layer 4: Android build/device gate

Only valid when Java/Gradle/SDK/NDK are available.

```sh
java -version
./gradlew lint
./gradlew test
./gradlew :terminal-emulator:buildAllRust
./gradlew assembleDebug
```

## 5. Known current risks

### Vulkan sync split across branches

The current `work/googleplay-base` branch may not contain the GPU sync fix that exists on `feature/rust-integration`.

Before doing more Vulkan work, inspect:

```sh
git diff work/googleplay-base..feature/rust-integration -- \
  terminal-emulator/src/main/rust/src/vulkan_context.rs \
  terminal-emulator/src/main/rust/src/render_thread.rs
```

### Gradle SDK mismatch risk

If `gradle.properties` has `targetSdkVersion` greater than `compileSdkVersion`, treat that as suspicious until an Android build proves it works.

### Java unavailable in Termux session

If `java` is not in `PATH` and `JAVA_HOME` is unset, do not claim Gradle build verification. Rust-only verification is still valid.

### HDR/experimental Vulkan tests

HDR/color-space tests may fail because of Skia/Vulkan surface availability on the current device. However, scalar/vector channel mismatches are algorithmic correctness failures and should be fixed.

## 6. Definition of done

A change is not done when the code compiles once. It is done when:

1. The affected ownership boundary is still clean.
2. The relevant verification script passed, or the failure is documented as pre-existing/device-dependent.
3. Branch and working-tree state are known.
4. No generated target/build artifacts are accidentally staged.
5. The next maintainer can understand why the change exists.

## 7. Human rule

This project is complicated because Android lifecycle + terminal emulation + Rust FFI + Vulkan synchronization are all hard at the same time. Do not use personal self-insults as project management. Replace them with checklists, gates, and small reversible patches.
