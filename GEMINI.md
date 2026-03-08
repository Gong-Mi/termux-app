# Termux-App: Rust Integration Project Status & Next Steps

## 📍 Current State (As of Last Session)
We have successfully stabilized the foundational Java architecture to prepare for a full Rust integration. Critical fixes were implemented:
1.  **TerminalSession:** Fixed a 32KB processing hang by properly rescheduling `MSG_NEW_INPUT` when breaking the loop.
2.  **TerminalEmulator:** Removed a critical flaw where bytes processed by the native Rust layer were being re-processed by the legacy Java state machine.
3.  **TermuxInstaller:** Added concurrency protection (`sIsBootstrapInstallationRunning`) to prevent race conditions during bootstrap extraction.
4.  **TerminalView:** Improved `invalidate()` throttling for better high-refresh-rate compatibility and battery efficiency.

**Current Architecture:** The JNI bridge (`termux_rust`) is active, but it acts primarily as a parser plugin. The terminal state (Buffer, Screen, Cursor) is still owned and managed by Java.

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