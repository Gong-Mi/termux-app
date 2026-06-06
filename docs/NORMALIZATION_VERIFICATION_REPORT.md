# Normalization Verification Report

Generated for the local Termux checkout:

- repository: `/data/user/0/com.termux/files/home/termux-app`
- branch observed: `work/googleplay-base`

## 1. Files added by this normalization pass

Only documentation and verification scripts were added. No Rust, Java, Kotlin, Gradle, CMake, or Cargo source file was modified by this pass.

Added files:

- `docs/PROJECT_NORMALIZATION.md`
- `docs/NORMALIZATION_VERIFICATION_REPORT.md`
- `scripts/verify_rust_core.sh`
- `scripts/verify_vulkan_basic.sh`
- `scripts/verify_vulkan_experimental.sh`

Existing unrelated dirty files may still exist in the working tree; check with:

```sh
git status --short
```

## 2. Script syntax verification

Command run:

```sh
for f in scripts/verify_rust_core.sh scripts/verify_vulkan_basic.sh scripts/verify_vulkan_experimental.sh; do
  echo "== bash -n $f =="
  bash -n "$f" || exit 1
done
```

Observed result:

```text
== bash -n scripts/verify_rust_core.sh ==
== bash -n scripts/verify_vulkan_basic.sh ==
== bash -n scripts/verify_vulkan_experimental.sh ==
```

Exit code: `0`

Meaning: all three shell scripts are syntactically valid for the local `bash`.

## 3. Termux shebang note

The initial portable-looking shebang failed on this device:

```text
#!/usr/bin/env bash
```

Failure:

```text
/data/data/com.termux/files/usr/bin/bash: scripts/verify_rust_core.sh: /usr/bin/env: bad interpreter: No such file or directory
```

Environment check:

```text
bash=/data/data/com.termux/files/usr/bin/bash
env=/data/data/com.termux/files/usr/bin/env
/bin/bash no
/usr/bin/env no
/data/data/com.termux/files/usr/bin/env yes
```

Therefore the scripts currently use:

```text
#!/data/data/com.termux/files/usr/bin/bash
```

This is correct for direct execution in this Termux environment. On normal Linux/GitHub Actions, invoke the scripts explicitly with `bash scripts/name.sh` if the absolute Termux shebang is not valid.

## 4. Rust core gate verification

Command run:

```sh
scripts/verify_rust_core.sh
```

Observed result:

```text
Rust core gate passed.
```

Important observed details:

- `cargo test --lib --features test-helpers --` passed.
- Unit test count: `295 passed; 0 failed`.
- Targeted integration tests also passed:
  - `child_watcher_regression`
  - `color_pipeline_test`
  - `consistency`
  - `key_event_handling`
  - `performance`
  - `reflow_600_lines`
  - `reflow_stress`
  - `reflow_trap_repro`
  - `render_deadlock_test`
  - `render_params_batching`
  - `resize_benchmark`
  - `resize_column_change`
  - `resize_history_bug`
  - `resize_zoom_simulation`
  - `selection_pipeline_test`
  - `session_coordinator_test`
  - `state_incremental_push`
  - `vt_compatibility`

Meaning: parser/screen/reflow/selection/session/render-state core behavior is currently green under the selected local gate.

## 5. Vulkan basic gate verification

Command run:

```sh
scripts/verify_vulkan_basic.sh
```

Observed result:

```text
Vulkan basic gate passed.
```

Tests passed:

- `skia_basic_test`
- `skia_render_test`
- `vulkan_10bit_test`
- `vulkan_format_probe`
- `vulkan_render_benchmark`

Meaning: the basic Skia/Vulkan tests selected for the non-HDR gate currently pass on this device.

## 6. Vulkan experimental/HDR gate verification

Command run:

```sh
scripts/verify_vulkan_experimental.sh
```

Observed result: expected non-zero exit because this gate records known experimental/HDR failures.

Passed:

- `hdr_pipeline_integrity`

Failed:

- `vulkan_gamma_test`
  - `Failed to create Skia surface`
  - `Failed to create Skia surface A`
- `vulkan_hdr_simulation`
  - `B channel mismatch: scalar=0xe02003ff, vector=0xe0e003ff`
- `vulkan_hdr_verification`
  - `Failed to create Skia HDR Surface`
- `vulkan_memory_stress_test`
  - `Failed to create Skia context`

Meaning:

- HDR/ColorSpace/Skia surface tests are not part of the basic release gate yet.
- `vulkan_hdr_simulation` contains a likely algorithmic scalar/vector mismatch and should be fixed separately if HDR conversion is in scope.

## 7. What this report does not prove

This report does not prove:

- Android APK builds, because local Java/Gradle verification is currently blocked by missing Java/JAVA_HOME.
- The Vulkan GPU synchronization fix is present on `work/googleplay-base`; that still needs a targeted branch diff or cherry-pick from `feature/rust-integration`.
- Runtime Android lifecycle correctness during real background/foreground Surface destruction; that needs a device run after the app builds.

## 8. Safe rollback

Because this pass only added docs/scripts, rollback is simple:

```sh
rm docs/PROJECT_NORMALIZATION.md \
   docs/NORMALIZATION_VERIFICATION_REPORT.md \
   scripts/verify_rust_core.sh \
   scripts/verify_vulkan_basic.sh \
   scripts/verify_vulkan_experimental.sh
```

No source-code behavior will change by removing these files.

## 9. Minimal trust rule

If you cannot personally verify everything, use this rule:

- Trust `scripts/verify_rust_core.sh` only when it exits `0` and prints `Rust core gate passed.`
- Trust `scripts/verify_vulkan_basic.sh` only when it exits `0` and prints `Vulkan basic gate passed.`
- Do not require `scripts/verify_vulkan_experimental.sh` to pass unless you are specifically working on HDR/colorspace/memory-stress behavior.
- Do not claim Android build success until `java -version` and Gradle commands actually run.
