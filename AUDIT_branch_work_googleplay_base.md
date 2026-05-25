# Branch `work/googleplay-base` Code Audit Report

**Auditor:** Kimi Code CLI  
**Date:** 2026-05-25  
**Branch:** `work/googleplay-base`  
**Original HEAD:** `cb8ba033` — "Add SVE2 performance benchmark binary"  
**Updated HEAD:** `8d110071` — "Truly commit the deletion of performance_sve2 bin and android-only logic for analyze_exceptions"  
**Repository:** `https://github.com/Gong-Mi/termux-app`

---

## 1. Executive Summary

Out of the 9 new commits on this branch, **only one feature is genuinely incomplete** to the point of breaking the build:

| # | Feature / Commit | Status | Severity |
|---|------------------|--------|----------|
| 1 | `cb8ba033` — SVE2 performance benchmark + SIMD module | **INCOMPLETE** | 🔴 High |
| 2 | `d1726aac` — Vulkan HDR10 / Display P3 color space | Complete (with intentional WCG fallback) | 🟢 Low |
| 3 | `9d37770e` — Pipeline cache persistence | Complete | 🟢 Low |
| 4 | `887d31af` — Bootstrap initialization fix | Complete | 🟢 Low |
| 5 | `623196c1` — Preserve LD_PRELOAD across seccomp exec | Complete | 🟢 Low |
| 6 | `4091b2bf` — Harden seccomp execve wrapper | Complete | 🟢 Low |
| 7 | `73edc451` — Resolve shebang before linker wrapper | Complete | 🟢 Low |
| 8 | `9a5c5aa9` — Strengthen in-app debug log export | Complete | 🟢 Low |
| 9 | `8903d5b1` — Fix linker argv & bootstrap second stage | Complete | 🟢 Low |

The branch **cannot pass `cargo check --all-targets`** because the SVE2 benchmark references modules and functions that do not exist in the repository.

---

## 2. Incomplete / Broken Items (Evidence)

### 2.1 SVE2 SIMD — `simd` and `pixel` modules are completely missing

**File:** `terminal-emulator/src/main/rust/src/bin/performance_sve2.rs`

This binary imports:

```rust
use termux_rust::simd;
use termux_rust::pixel::Pixel8;
```

and calls:

```rust
simd::scalar::convert_rgba8_to_rgba10_scalar(...)
simd::sve2::convert_rgba8_to_rgba10_sve2(...)   // unsafe
```

**What is missing:**
- `terminal-emulator/src/main/rust/src/simd.rs` (or `src/simd/mod.rs`)
- `terminal-emulator/src/main/rust/src/pixel.rs`
- `pub mod simd;` and `pub mod pixel;` in `src/lib.rs`

**Build failure (reproducible):**

```bash
$ cd terminal-emulator/src/main/rust
$ cargo check --all-targets
error[E0432]: unresolved import `termux_rust::simd`
 --> src/bin/performance_sve2.rs:5:5
error[E0432]: unresolved import `termux_rust::pixel`
 --> src/bin/performance_sve2.rs:6:18
error: could not compile `termux-rust-new` (bin "performance_sve2") due to 2 previous errors
error: could not compile `termux-rust-new` (bin "performance_sve2" test) due to 2 previous errors
```

**Impact:**
- `cargo test` fails before any real tests run.
- The binary cannot be compiled, let alone executed.
- This is **not** a partial implementation; it is a **missing implementation** with only the caller side present.

**Note on `Cargo.toml`:**  
The binary is **not** declared in `Cargo.toml`. Because it lives under `src/bin/`, Cargo auto-discovers it. Therefore it is unconditionally compiled during `--all-targets`.

---

### 2.2 `test_sigsys.rs` uses hard-coded ucontext offsets inconsistent with production code

**File:** `terminal-emulator/src/main/rust-exec/src/bin/test_sigsys.rs`

Lines 201–208 hard-code aarch64 register offsets:

```rust
println!("offset(23) x0 (path): 0x{:016x}", dump[23]);
println!("offset(24) x1 (argv): 0x{:016x}", dump[24]);
println!("offset(25) x2 (envp): 0x{:016x}", dump[25]);
```

However, the production code in `lib.rs` + `get_regs.c` (commit `4091b2bf` / `73edc451`) already switched to **`offsetof(ucontext_t, uc_mcontext.regs[0])`** to avoid architecture/NDK-version-specific layout drift.

**Impact:**
- Low on runtime (it is a standalone diagnostic binary, not linked into the preload library).
- Medium on debuggability: if the ucontext layout ever changes, this diagnostic tool will print misleading offsets, making seccomp debugging harder.

---

### 2.3 `tests/analyze_exceptions.rs` is not a test

**File:** `terminal-emulator/src/main/rust/tests/analyze_exceptions.rs`

It defines `fn main()`, not `#[test] fn ...`. When running `cargo test`, Cargo discovers the file but finds zero test functions, so it executes zero assertions.

**Impact:**
- Low. It may have been intended as a standalone analysis utility, but it currently sits in the `tests/` directory where readers expect test coverage.

---

## 3. Items That Look Incomplete But Are Actually Complete

### 3.1 HDR10 fallback to Display P3

**File:** `terminal-emulator/src/main/rust/src/vulkan_context.rs` (lines 1110–1127)

The `_` match arm maps a 10-bit swapchain to `Display P3 + SRGB` when the Vulkan color space is **not** `HDR10_ST2084_EXT`:

```rust
_ => {
    if self.skia_format == skia_safe::gpu::vk::Format::A2B10G10R10_UNORM_PACK32 {
        skia_safe::ColorSpace::new_cicp(
            skia_safe::named_primaries::CicpId::SMPTE_EG_432_1,
            skia_safe::named_transfer_fn::CicpId::SRGB,
        )
    } else {
        Some(skia_safe::ColorSpace::new_srgb())
    }
}
```

**Why this is NOT incomplete:**
- The dedicated `HDR10_ST2084_EXT` branch (lines 1099–1105) already implements **true HDR10** (`Rec2020` primaries + `PQ` transfer function).
- The fallback to Display P3 is an **intentional compatibility shim** for Android devices whose SurfaceFlinger does not expose `HDR10_ST2084_EXT` even when `COLOR_MODE_HDR` is requested. This is common on many OEM skins.
- The swapchain selection logic (lines 784–787) still **prioritizes** `HDR10_ST2084_EXT` when available.

---

### 3.2 `tests/test_jni_null.rs` is a trivial test

**File:** `terminal-emulator/src/main/rust/tests/test_jni_null.rs`

Contains only `assert!(true);`. While it provides no real coverage, it compiles and passes, and does not block the build.

---

## 4. Compilation Matrix

| Crate / Target | `cargo check` | `cargo test` | Notes |
|----------------|---------------|--------------|-------|
| `termux-rust-new` (lib only) | ✅ Pass | — | Library itself is clean. |
| `termux-rust-new` (all targets) | ❌ **Fail** | ❌ **Fail** | Blocked by `performance_sve2.rs` |
| `termux-exec-rs` (lib + bins) | ✅ Pass | ✅ Pass | 10/10 unit tests pass. |
| `termux_exec_device_probe` | ✅ Pass | — | Standalone diagnostic binary. |
| `test_sigsys` | ✅ Pass | — | Standalone diagnostic binary. |

**Java side:**
- `TermuxInstaller.java` native method `getZip()` has a corresponding Rust JNI implementation in `bootstrap.rs`.
- `TermuxLogCollector.java` methods `collect()`, `collectEnvConfig()`, `collectCommandAvailability()` are all fully implemented.
- `TermuxShellUtils.java` is fully implemented.
- No missing class references detected in `TermuxActivity.java`.

---

## 5. Recommended Actions

### Immediate (blocks CI / merge)
1. **Fix `performance_sve2.rs` compilation:**
   - Option A: Remove `src/bin/performance_sve2.rs` from the branch until SVE2 is implemented.
   - Option B: Add `src/simd.rs` + `src/pixel.rs` stubs with a scalar fallback and an empty SVE2 placeholder so the crate compiles.

### Short-term (quality)
2. **Update `test_sigsys.rs`** to use `get_regs.c` helpers instead of hard-coded offsets 23/24/25, or add a compile-time warning that it assumes a specific NDK ucontext layout.
3. **Move `analyze_exceptions.rs`** out of `tests/` into `tools/` or `examples/` if it is meant to be a standalone utility, or convert it into real `#[test]` functions.

### Not required
4. **HDR10 Display P3 fallback** — no action needed; the design is intentional and correct.

---

## 6. Audit Methodology

1. `git fetch https://github.com/Gong-Mi/termux-app.git work/googleplay-base`
2. `git reset --hard FETCH_HEAD` (HEAD at `cb8ba033`)
3. `cargo check --all-targets` on both `termux-rust-new` and `termux-exec-rs`
4. `cargo test` on `termux-exec-rs`
5. `grep -rn "TODO\|FIXME\|unimplemented\|placeholder\|stub"` across Rust and Java sources
6. Verified every `pub mod` declaration has a corresponding `.rs` file
7. Verified native method declarations in Java have corresponding `pub extern "system" fn Java_...` in Rust
8. Manually inspected `vulkan_context.rs`, `bootstrap.rs`, `TermuxLogCollector.java`, `TermuxInstaller.java`

---

---

## 7. Round 2 Audit (Post-Update)

**Date:** 2026-05-25 (follow-up)  
**New commits reviewed:** `db5f025f`, `33c9c979`, `8d110071`

### 7.1 Fixes Applied ✅

| Issue | Fix Commit | Status |
|-------|-----------|--------|
| `performance_sve2.rs` unresolved imports | `8d110071` | **Deleted** the orphan binary |
| `analyze_exceptions.rs` not a test | `8d110071` | Rewrote as `#[test]` with `#[cfg(target_os = "android")]` |
| Skia test platform-specific pixel assertions | `33c9c979` | Replaced raw pointer reads with `pixmap.get_color()` |
| Missing scalar SIMD fallback | `db5f025f` | Added `simd/scalar.rs`, `pixel.rs`, `cpu_features.rs` |
| `anyhow` dev-dependency missing | `33c9c979` | Added to `Cargo.toml` |

### 7.2 New / Remaining Issues ❌

#### 7.2.1 New modules are NOT registered in `lib.rs` (dead code)

**Files added but unlinked:**
- `src/simd/mod.rs`
- `src/simd/scalar.rs`
- `src/pixel.rs`
- `src/cpu_features.rs`

**Evidence:** `src/lib.rs` lines 103–117 still lists only the original modules; no `pub mod simd;`, `pub mod pixel;`, or `pub mod cpu_features;` was added.

**Consequence:** `cargo check --all-targets` passes only because these files are invisible to the compiler. They are currently **dead code**.

#### 7.2.2 `simd/mod.rs` references a non-existent `sve2` submodule

**File:** `src/simd/mod.rs` lines 4–5

```rust
#[cfg(target_arch = "aarch64")]
pub mod sve2;
```

**Missing file:** `src/simd/sve2.rs`

**Consequence:** The moment `pub mod simd;` is added to `lib.rs`, builds on aarch64 will fail with:
```
error[E0583]: file not found for module `sve2`
```

#### 7.2.3 Inconsistent color-space mapping formulas in `vulkan_hdr_simulation.rs`

**File:** `tests/vulkan_hdr_simulation.rs`

| Implementation | Formula | Approximate multiplier |
|----------------|---------|------------------------|
| Scalar | `(val * 1023 + 127) / 255` | 4.01176 |
| "Vectorized" | `((val * 263) >> 6).min(1023)` | 4.109375 |

These formulas are **not mathematically equivalent**. The test allows ±1 LSB tolerance, but the discrepancy is a design-level inconsistency, not just a rounding difference. Furthermore, the "vectorized" function is plain Rust with no SVE2/NEON intrinsics; it relies entirely on LLVM auto-vectorization, which is not guaranteed.

---

## 8. Updated Compilation Matrix

| Crate / Target | `cargo check` | Notes |
|----------------|---------------|-------|
| `termux-rust-new` (lib only) | ✅ Pass | Library compiles because new modules are unlinked. |
| `termux-rust-new` (all targets) | ✅ Pass | `performance_sve2.rs` removed; no compile errors. |
| `termux-exec-rs` | ✅ Pass | 10/10 tests pass. |

**Critical caveat:** The "all targets" check passes only because `simd/`, `pixel.rs`, and `cpu_features.rs` are **not included in the build**. If they were registered in `lib.rs`, the missing `sve2.rs` would break aarch64 builds.

---

## 9. Recommended Actions (Updated)

### Immediate
1. **Register new modules in `lib.rs`:**
   ```rust
   pub mod cpu_features;
   pub mod pixel;
   pub mod simd;
   ```
2. **Create `src/simd/sve2.rs`** (even if it is a stub that panics or falls back to scalar), **or** remove the `pub mod sve2;` declaration from `simd/mod.rs` until SVE2 is implemented.

### Short-term
3. **Unify the RGBA8→RGBA10 mapping formula** in `vulkan_hdr_simulation.rs` so the scalar and "vectorized" implementations use the same arithmetic, or document why a different approximation is acceptable.
4. **Add a real SVE2 implementation** using `std::arch::aarch64::*` intrinsics, gated behind `#[cfg(target_arch = "aarch64")]` and runtime feature detection via `cpu_features::has_sve2()`.

---

*End of report.*
