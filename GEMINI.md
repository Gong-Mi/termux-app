# Termux-App: Rust Integration Project Status & Next Steps

## 📍 Current State (As of Last Session)
We have successfully stabilized the foundational Java architecture to prepare for a full Rust integration. Critical fixes were implemented:
1.  **TerminalSession:** Fixed a 32KB processing hang by properly rescheduling `MSG_NEW_INPUT` when breaking the loop.
2.  **TerminalEmulator:** Removed a critical flaw where bytes processed by the native Rust layer were being re-processed by the legacy Java state machine.
3.  **TermuxInstaller:** Added concurrency protection (`sIsBootstrapInstallationRunning`) to prevent race conditions during bootstrap extraction.
4.  **TerminalView:** Improved `invalidate()` throttling for better high-refresh-rate compatibility and battery efficiency.

**Current Architecture:** The JNI bridge (`termux_rust`) is active, but it acts primarily as a parser plugin. The terminal state (Buffer, Screen, Cursor) is still owned and managed by Java.

---

## ✅ Completed: DECSET Mode Enhancement (100%)

The Rust DECSET mode implementation has been completed with the following additions:

### New DECSET Features Implemented:
1.  **DECSET 69 (DECLRMM)** - Left-right margin mode
    - Added `leftright_margin_mode` field to `ScreenState`
    - Implemented `set_left_right_margins()` method
    - CSI `s` command now handles DECSLRM when DECLRMM is enabled

2.  **DECSET 1004** - Send focus events
    - Added `send_focus_events` field to track state

3.  **Mouse Mode Exclusivity (1000 vs 1002)**
    - Implemented mutual exclusion logic between mouse tracking modes
    - Setting mode 1000 clears 1002, and vice versa

4.  **DECSET Flags Tracking**
    - Added `decset_flags` bit field for complete state tracking
    - Added `saved_decset_flags` for cursor save/restore operations
    - `save_cursor()` and `restore_cursor()` now properly save/restore AUTOWRAP and ORIGIN_MODE bits

5.  **New Constants** - All DECSET bit flags defined matching Java implementation:
    - `DECSET_BIT_APPLICATION_CURSOR_KEYS`
    - `DECSET_BIT_REVERSE_VIDEO`
    - `DECSET_BIT_ORIGIN_MODE`
    - `DECSET_BIT_AUTOWRAP`
    - `DECSET_BIT_CURSOR_ENABLED`
    - `DECSET_BIT_APPLICATION_KEYPAD`
    - `DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE`
    - `DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT`
    - `DECSET_BIT_SEND_FOCUS_EVENTS`
    - `DECSET_BIT_MOUSE_PROTOCOL_SGR`
    - `DECSET_BIT_BRACKETED_PASTE_MODE`
    - `DECSET_BIT_LEFTRIGHT_MARGIN_MODE`

### New Tests Added (4 tests):
- `test_decset_leftright_margin_mode()` - Tests DECLRMM and DECSLRM
- `test_decset_send_focus_events()` - Tests DECSET 1004
- `test_mouse_mode_exclusive()` - Tests mouse mode mutual exclusion
- `test_decset_flags_save_restore()` - Tests DECSET flag preservation during cursor save/restore

**All 44 consistency tests pass.**

---

## ✅ Completed: SGR Attribute Enhancement (100%)

The Rust SGR (Select Graphic Rendition) implementation has been completed with the following additions:

### New SGR Features Implemented:
1.  **256-Color Support (38;5;n / 48;5;n)**
    - Indexed foreground colors (0-255)
    - Indexed background colors (0-255)

2.  **24-bit Truecolor Support (38;2;R;G;B / 48;2;R;G;B)**
    - RGB foreground colors
    - RGB background colors
    - Proper `STYLE_TRUECOLOR_FG` and `STYLE_TRUECOLOR_BG` flag setting

3.  **Underline Sub-parameter Support (4:0)**
    - `4:0` clears underline
    - `4` (without sub-parameter) enables underline

4.  **Underline Color Support (58;5;n / 58;2;R;G;B)**
    - Parses underline color parameters
    - Note: Actual underline color rendering requires additional storage

5.  **All Standard SGR Codes**
    - 0-9, 21-29 (effects)
    - 30-37, 39 (foreground colors)
    - 40-47, 49 (background colors)
    - 90-97 (bright foreground)
    - 100-107 (bright background)

### New Tests Added (5 tests):
- `test_sgr_256_color_foreground()` - Tests 256-color foreground
- `test_sgr_256_color_background()` - Tests 256-color background
- `test_sgr_truecolor_foreground()` - Tests 24-bit truecolor foreground
- `test_sgr_truecolor_background()` - Tests 24-bit truecolor background
- `test_sgr_underline_subparam()` - Tests underline sub-parameter handling

**All 49 consistency tests pass.**

### Bit Layout Verification (Java ↔ Rust Compatibility):
Both Java and Rust use identical 64-bit style encoding:
- Bits 0-10: Effect flags (11 bits)
  - Bit 0: Bold
  - Bit 1: Italic
  - Bit 2: Underline
  - Bit 3: Blink
  - Bit 4: Reverse
  - Bit 5: Invisible
  - Bit 6: Strikethrough
  - Bit 7: Protected
  - Bit 8: Dim
  - Bit 9: Truecolor Foreground flag
  - Bit 10: Truecolor Background flag
- Bits 16-39: Background color (24 bits for truecolor, 9 bits for index)
- Bits 40-63: Foreground color (24 bits for truecolor, 9 bits for index)

---

## 🎯 Next Objective: "Full Rustification"
The next logical step is to shift the **ownership of the terminal state** from Java to Rust. Relying on Java to hold the state while Rust parses it creates unnecessary JNI overhead and synchronization complexity.

### The Goal:
Rust should own the `TerminalEmulator` state (Screen, History, Cursor, Colors). Java should only act as a thin View layer that:
1. Feeds raw input bytes to the Rust engine.
2. Requests rendering data (e.g., "give me line X") from the Rust engine during `onDraw`.

---

## 🛠️ Action Plan for Next Session

### Phase 1: Rust State Architecture
*   Define the core structures in the Rust library for Terminal Buffer, Lines, Cells, and Cursor state.
*   Ensure the Rust `Engine` can maintain this state across JNI calls.

### Phase 2: Refactoring `TerminalEmulator.java`
*   Strip out the legacy Java arrays (`mScreen`, `mColors`, etc.) if their Rust counterparts are ready.
*   Modify `TerminalEmulator.java` to act as a proxy to the Rust `Engine`.
*   *Warning:* Ensure `mRustEnginePtr` lifecycle (creation, destruction) is strictly managed to prevent memory leaks.

### Phase 3: Rendering Bridge
*   Establish efficient JNI methods for `TerminalRenderer` to extract text and styling data from the Rust engine line-by-line during the `TerminalView.onDraw` cycle.
*   Avoid copying the entire buffer on every frame; fetch only visible lines.

---

## ⚠️ Important Guidelines
*   **Do not mix state:** AVOID having both Java and Rust trying to update the terminal state simultaneously. Pick one (Rust) and stick to it.
*   **JNI Overhead:** Minimize JNI calls per frame. Batch operations where possible (e.g., fetch a whole line or screen segment at once, not cell by cell).
*   **Concurrency:** Terminal data comes in asynchronously via `TerminalSession`, while rendering happens on the UI thread. The Rust engine must be thread-safe (e.g., using `Mutex` or `RwLock` internally) to handle concurrent writes and reads.