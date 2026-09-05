# Engine handle ownership

This slice builds on `50e3b57d5fd48daacc324da5adada19f004efc47` (construction/adoption separation).
It fixes native memory ownership, not the complete session-exit or Vulkan pipeline.

## Identity and ownership

- Java `enginePtr: Long` and the first element of `pollEngineData` are now opaque positive i64 tokens, not addresses. JNI descriptors and Kotlin parameter names are preserved. Never cast a token to TerminalContext, truncate it to a pointer-width integer, or use it as a direct-buffer address.
- Each creation inserts one Arc owner in the global EngineHandles registry. Handles are monotonic and never reused within the process. Exhaustion returns creation failure instead of wrapping.
- Every JNI operation clones its own lease. Destroy atomically removes future access and the matching render binding; existing JNI/frame leases may complete. Double destroy is a no-op.
- The registry map and current render binding share one mutex. Publish validates under that same lock; a late revoked token cannot replace a newer session. Displaced payload references are dropped outside the lock. No engine lock, GPU operation, IO, callback or join runs under it.
- RenderFrame still copies engine state under its existing read lock. The frame loop now owns an Arc while accessing that lock. This does not reject all stale frames at present time; attachment/Surface generation is a later slice.
- Removed Rust `get_engine_pointer/get_engine_ready` mutable global access is intentionally replaced by the registry API. There were no tracked callers outside the migrated render JNI. Native out-of-tree callers must migrate. SharedScreenBuffer and ANativeWindow remain real addresses and are unchanged.

## Creation delivery

`createEngine` returns the new token directly. `createSessionAsync` uses exactly one delivery mode:

1. Non-null callback: push delivery only. It no longer also exposes the owner through the polling cache. A failed GlobalRef rejects creation; dup failure and failed/unwinding JNI delivery revoke the registered handle. RustEngineCallback without a session and Handler.post(false) destroy the undeliverable token.
2. Null callback: legacy polling mode. The coordinator retains the pending token until it is taken or the session is unregistered. Poll result allocation happens before take; a failed result write revokes the unclaimed token. Kotlin's existing non-null annotation is unchanged; native/Java callers can select null polling, and the real JNI harness exercises it reflectively.

This explicitly narrows the old ambiguous non-null-callback-plus-poll dual-owner behavior. Tracked production Kotlin used only callback delivery. Poll data carries token/fd/pid, not multiple pointer owners.

Successful JNI return/Handler.post(true) is still not an acknowledgement that a queued adoption actually ran. Creation-generation, duplicate/stale adoption, Looper termination after accepted post, and coordinated initialization cancellation remain open. The new guard is not described as a complete delivery state machine.

## Return compatibility

Revoked/unknown handles use each method's existing zero-handle result: void no-op, numeric/boolean zero, object null, or the existing destroyed debug string. Invalid readRow leaves arrays untouched. Valid parsing/getter/mutator bodies and callback order are preserved; no protocol/selection/width algorithm is changed.

## IO boundary

Destroy retains the old running=false/original-master-close behavior. A duplicated reader fd remains independently owned and may be blocked. Arc memory safety does not make fd load/write/close race-free, wake a blocked read, drain output, deliver process exit, or guarantee eventual thread cleanup. The next slice must handle cancellation and writing together (including parser responses).

## Verification

- `python scripts/verify-engine-handle-boundary.py`: inventory all current RustTerminal engine methods, reject reintroduced raw Arc/address access, and verify test registration. Static evidence only.
- `cargo test --locked --test engine_handle_lifecycle`: compile the production std-only registry source including private exhaustion/drop tests; barrier-controlled lifetime/revocation tests, no fake registry. Included in lifecycle/all.
- `cargo test --locked --test engine_handle_integration`: real global registry + TerminalContext, retained RenderFrame generation, original-fd close vs independent duplicate, pending polling cleanup. No Surface/GPU.
- `scripts/verify-emulator-construction.py --mode contract`: recording JNI constructor-call contract, explicitly not native ownership.
- The same script `--mode native`: production Kotlin + real JNI constructor/adoption/parse/resize/destroy.
- The same script `--mode handles`: all declared native handle methods on invalid/revoked tokens, neutral return and output-array contract, duplicate destroy, unowned callback, concurrent JNI smoke, plus actual async PTY creation/poll, offset/count input and reader parse. Supply the compiler/compile/runtime classpaths, actual native library, and output directory shown by `--help`. JVM stdin is isolated; real PTY child exits after the test input.
- engine-construction.yml builds the native library and runs each mode in a separate JVM; other workflows retain independent correctness/static/ABI/emulator gates and accept the stacked base branch.

Concurrent JNI smoke does not prove a lease was acquired before destroy; the deterministic registry barriers provide that evidence at the registry layer. JNI exception/attach failure and real Android Handler.post(false) injection have not yet been exercised. No ART/physical-GPU display success is claimed.

## Complete baseline raw-owner mapping

Paths below refer to `terminal-emulator/src/main/rust/src/jni/terminal_emulator.rs` at the baseline. All original from_raw borrow/destroy sites and both creators are migrated; the constant isInsertModeActive stub never dereferenced its argument and remains unchanged.

- `Java_com_termux_terminal_RustTerminal_createEngine`: 89
- `Java_com_termux_terminal_RustTerminal_processBatch`: 115
- `Java_com_termux_terminal_RustTerminal_processInput`: 149
- `Java_com_termux_terminal_RustTerminal_startIoThread`: 185
- `Java_com_termux_terminal_RustTerminal_destroyEngine`: 201
- `Java_com_termux_terminal_RustTerminal_processCodePoint`: 230
- `Java_com_termux_terminal_RustTerminal_setTranscriptRows`: 254
- `Java_com_termux_terminal_RustTerminal_resize`: 273
- `Java_com_termux_terminal_RustTerminal_getTitle`: 299
- `Java_com_termux_terminal_RustTerminal_getCursorRow`: 323
- `Java_com_termux_terminal_RustTerminal_getCursorCol`: 342
- `Java_com_termux_terminal_RustTerminal_getCursorStyle`: 361
- `Java_com_termux_terminal_RustTerminal_setCursorStyle`: 380
- `Java_com_termux_terminal_RustTerminal_doDecSetOrReset`: 402
- `Java_com_termux_terminal_RustTerminal_shouldCursorBeVisible`: 427
- `Java_com_termux_terminal_RustTerminal_isCursorEnabled`: 453
- `Java_com_termux_terminal_RustTerminal_isReverseVideo`: 471
- `Java_com_termux_terminal_RustTerminal_isAlternateBufferActive`: 493
- `Java_com_termux_terminal_RustTerminal_isCursorKeysApplicationMode`: 515
- `Java_com_termux_terminal_RustTerminal_isKeypadApplicationMode`: 537
- `Java_com_termux_terminal_RustTerminal_isMouseTrackingActive`: 559
- `Java_com_termux_terminal_RustTerminal_getScrollCounter`: 586
- `Java_com_termux_terminal_RustTerminal_getRows`: 604
- `Java_com_termux_terminal_RustTerminal_getCols`: 622
- `Java_com_termux_terminal_RustTerminal_readRow`: 641
- `Java_com_termux_terminal_RustTerminal_getSelectedText`: 674
- `Java_com_termux_terminal_RustTerminal_getWordAtLocation`: 713
- `Java_com_termux_terminal_RustTerminal_getTranscriptText`: 743
- `Java_com_termux_terminal_RustTerminal_clearScrollCounter`: 767
- `Java_com_termux_terminal_RustTerminal_isAutoScrollDisabled`: 787
- `Java_com_termux_terminal_RustTerminal_toggleAutoScrollDisabled`: 809
- `Java_com_termux_terminal_RustTerminal_sendMouseEvent`: 829
- `Java_com_termux_terminal_RustTerminal_sendKeyCode`: 855
- `Java_com_termux_terminal_RustTerminal_pasteText`: 896
- `Java_com_termux_terminal_RustTerminal_getActiveTranscriptRows`: 926
- `Java_com_termux_terminal_RustTerminal_getColors`: 945
- `Java_com_termux_terminal_RustTerminal_resetColors`: 977
- `Java_com_termux_terminal_RustTerminal_updateColors`: 1001
- `Java_com_termux_terminal_RustTerminal_setCursorColorForBackground`: 1108
- `Java_com_termux_terminal_RustTerminal_updateTerminalSessionClient`: 1145
- `Java_com_termux_terminal_RustTerminal_setCursorBlinkState`: 1172
- `Java_com_termux_terminal_RustTerminal_setCursorBlinkingEnabled`: 1193
- `Java_com_termux_terminal_RustTerminal_getDebugInfo`: 1215
- `Java_com_termux_terminal_JNI_createSessionAsync`: 1258
