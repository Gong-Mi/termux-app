# Rust Integration Status

## Overview

This document describes the current status of the Rust integration in the Termux terminal emulator, including what has been implemented, what is missing, and the path forward for completing the integration.

## Architecture

### Current Design

The terminal emulator uses a hybrid architecture:

```
┌─────────────────────────────────────────────────────────────┐
│                    TerminalEmulator.java                     │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              Java State Machine                        │  │
│  │  - Full ANSI/VT100 sequence handling                  │  │
│  │  - Screen buffer (mScreen)                            │  │
│  │  - Cursor state, colors, modes                        │  │
│  └───────────────────────────────────────────────────────┘  │
│                            ↑                                 │
│                            │ JNI callbacks (optional)        │
│                            │                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │           Rust Fast Path (processBatchRust)            │  │
│  │  - Scans ASCII bytes for control characters           │  │
│  │  - Line drawing character mapping                     │  │
│  │  - Returns count of bytes that can be written         │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Disabled Features

```
┌─────────────────────────────────────────────────────────────┐
│           Rust TerminalEngine (DISABLED)                     │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  vte::Parser                                          │  │
│  │  - Parses escape sequences                            │  │
│  │  - Calls Perform trait methods                        │  │
│  └───────────────────────────────────────────────────────┘  │
│                            ↓                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  ScreenState (Rust)                                   │  │
│  │  - Independent terminal state                         │  │
│  │  - NOT synchronized with Java mScreen                 │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                             │
│  ⚠️  DISABLED because: Incomplete ANSI sequence handling   │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Status

### ✅ Implemented

#### Rust Library (`terminal-emulator/src/main/rust/`)

1. **JNI Bindings** (`src/lib.rs`):
   - `processBatchRust` - Fast ASCII scanning
   - `mapLineDrawingNative` - VT100 line drawing character mapping
   - `writeASCIIBatchNative` - Batch write with style
   - `createEngineRust` / `destroyEngineRust` - Engine lifecycle
   - `processEngineRust` - Full data processing (disabled)
   - `readRowFromRust` - Read screen content (disabled)
   - `resizeEngineRust` - Handle resize (disabled)

2. **Terminal Engine** (`src/engine.rs`):
   - `TerminalEngine` - Stateful terminal emulator
   - `ScreenState` - Circular buffer implementation
   - `PurePerformHandler` - vte `Perform` trait implementation
   - O(1) scroll using circular buffer

3. **Utilities** (`src/utils.rs`):
   - `map_line_drawing` - VT100 line drawing mapping
   - `get_char_width` - Unicode width calculation

4. **Fast Path** (`src/fastpath.rs`):
   - `scan_ascii_batch` - Fast ASCII scanning

5. **PTY Management** (`src/pty.rs`):
   - `create_subprocess` - Fork and exec subprocess
   - `set_pty_window_size` - Update PTY window size
   - `wait_for` - Wait for process exit

#### Java Integration (`terminal-emulator/src/main/java/...`)

1. **Fast Path Integration**:
   - `processBatchRust` called for ASCII batches
   - Falls back to Java for non-ASCII and escape sequences

2. **Rendering**:
   - `getRowContent` reads from Java `mScreen` buffer
   - Rust engine data not used (disabled)

### ❌ Not Implemented / Incomplete

#### Rust Engine ANSI Sequence Support

The `PurePerformHandler` in `src/engine.rs` only handles a minimal subset:

**CSI Sequences (`ESC [`)**:
- ✅ `m` - SGR (Set Graphics Rendition) - basic colors only
- ✅ `J` - ED (Erase in Display)
- ✅ `K` - EL (Erase in Line)
- ✅ `H` / `f` - CUP (Cursor Position)
- ✅ `A` - CUU (Cursor Up)
- ✅ `B` - CUD (Cursor Down)
- ✅ `C` - CUF (Cursor Forward)
- ✅ `D` - CUB (Cursor Back)
- ❌ `G` - CHA (Cursor Horizontal Absolute)
- ❌ `L` - IL (Insert Line)
- ❌ `M` - DL (Delete Line)
- ❌ `@` - ICH (Insert Character)
- ❌ `P` - DCH (Delete Character)
- ❌ `S` - SU (Scroll Up)
- ❌ `T` - SD (Scroll Down)
- ❌ `X` - ECH (Erase Character)
- ❌ `d` - VPA (Vertical Position Absolute)
- ❌ `?` - DECSET/DECRST private modes
- ❌ `>` - DECKPAM (Numeric Keypad)
- ❌ `<` - DECKPNM (Normal Keypad)
- ❌ `=` - DECSASD (Select Active Status Display)
- ❌ `>` - DECPUS (Push Status)
- ❌ `!` - DECSTR (Soft Reset)
- ❌ `" q` - DECSCUSR (Set Cursor Style)
- ❌ ` r` - DECSTBM (Set Top and Bottom Margins)
- ❌ ` s` - DECSLRM (Set Left and Right Margins)
- ❌ `$ p` - DECRQM (Request Mode)
- ❌ `$ y` - DECCARA (Change Attributes in Rectangular Area)

**ESC Sequences**:
- ✅ `7` - DECSC (Save Cursor)
- ✅ `8` - DECRC (Restore Cursor)
- ❌ `# 8` - DECALN (Screen Alignment Test)
- ❌ `(` - Designate G0 Character Set
- ❌ `)` - Designate G1 Character Set
- ❌ `*` - Designate G2 Character Set
- ❌ `+` - Designate G3 Character Set
- ❌ `=` - DECPAM (Application Program Command)
- ❌ `>` - DECPNM (Program Numeric Keypad)
- ❌ `D` - IND (Index)
- ❌ `E` - NEL (Next Line)
- ❌ `F` - CNL (Cursor Next Line)
- ❌ `G` - CPL (Cursor Previous Line)
- ❌ `H` - HTS (Horizontal Tab Set)
- ❌ `M` - RI (Reverse Index)
- ❌ `Z` - DECID (Identify Device)
- ❌ `c` - RIS (Reset to Initial State)
- ❌ `n` - LS2 (Locking Shift G2)
- ❌ `o` - LS3 (Locking Shift G3)
- ❌ `|` - LS3R (Locking Shift G3 Right)
- ❌ `}` - LS2R (Locking Shift G2 Right)
- ❌ `~` - LS1R (Locking Shift G1 Right)

**OSC Sequences (`ESC ]`)**:
- ❌ `0` - Set Icon Name and Window Title
- ❌ `2` - Set Window Title
- ❌ `4` - Set Color
- ❌ `5` - Set Special Color
- ❌ `6` - Enable/Disable Special Color
- ❌ `10`-`19` - Set/Query Dynamic Colors
- ❌ `52` - Clipboard Operations

**APC/DCS Sequences**:
- ❌ `ESC _` - APC (Application Program Command)
- ❌ `ESC P` - DCS (Device Control String)
- ❌ `ESC X` - SOS (Start of String)
- ❌ `ESC ^` - PM (Privacy Message)

**Other Missing Features**:
- ❌ Insert mode
- ❌ Delete mode
- ❌ Tab stops
- ❌ Margins (DECTCEM, DECLRMM)
- ❌ Character attributes (bold, underline, blink, etc.)
- ❌ 256-color and truecolor support
- ❌ Mouse tracking
- ❌ Bracketed paste mode
- ❌ Synchronized output (DECSET 2026)

## Known Issues

### 1. State Desynchronization

When `FULL TAKEOVER` mode is enabled, the Rust engine maintains its own state independently from Java. This causes:

- Screen content mismatch between Rust and Java buffers
- Cursor position differences
- Missing character attributes
- Incorrect margin handling
- Title changes not propagated

### 2. Incomplete ANSI Support

The Rust engine only handles basic text output and cursor movement. Complex sequences are silently ignored, leading to:

- Applications not displaying correctly (e.g., vim, nano, htop)
- Color schemes not applied
- Screen clearing not working properly
- Tab completion issues in shells

## Path Forward

### Option 1: Complete Rust Implementation (Recommended for Performance)

**Goal**: Make Rust engine feature-complete and re-enable `FULL TAKEOVER` mode.

**Steps**:
1. Implement all missing CSI sequences (see list above)
2. Implement all missing ESC sequences
3. Implement OSC sequence handling
4. Add callback mechanism to sync Rust state to Java `mScreen`
5. Update consistency tests to verify full compatibility
6. Re-enable `FULL TAKEOVER` mode in `TerminalEmulator.append()`

**Estimated Effort**: 200-400 hours

**Benefits**:
- Maximum performance (single-pass parsing in Rust)
- Clean architecture (Rust owns terminal state)
- Better memory efficiency (no duplicate buffers)

### Option 2: Hybrid Parser Mode (Recommended for Quick Fix)

**Goal**: Use Rust as a parser, callback to Java for execution.

**Steps**:
1. Modify `PurePerformHandler` to callback Java via JNI
2. Java executes all state changes on `mScreen`
3. Rust only provides fast parsing, not state storage
4. Keep `getRowContent` reading from Java buffer

**Estimated Effort**: 40-80 hours

**Benefits**:
- Leverages existing Java implementation
- Guaranteed compatibility
- Faster to implement

**Drawbacks**:
- JNI overhead for each sequence
- More complex code (two execution paths)

### Option 3: Fast Path Only (Current Status)

**Goal**: Keep current implementation, only optimize ASCII batches.

**Steps**:
1. Maintain current fast path for ASCII
2. Java handles everything else
3. Optionally improve fast path coverage

**Estimated Effort**: Already implemented

**Benefits**:
- Stable and working
- No compatibility issues

**Drawbacks**:
- Limited performance improvement
- Rust code partially unused

## Code Locations

### Rust Code
```
terminal-emulator/src/main/rust/
├── Cargo.toml                    # Rust package configuration
├── src/
│   ├── lib.rs                    # JNI entry points
│   ├── engine.rs                 # Terminal engine (DISABLED)
│   ├── fastpath.rs               # ASCII fast scanning
│   ├── utils.rs                  # Utility functions
│   └── pty.rs                    # Process management
└── target/                       # Build output
```

### Java Code
```
terminal-emulator/src/main/java/com/termux/terminal/
├── TerminalEmulator.java         # Main emulator logic
├── TerminalBuffer.java           # Screen buffer
├── TerminalRow.java              # Row storage
└── JNI.java                      # Native library loading
```

### Test Code
```
terminal-emulator/src/androidTest/java/com/termux/terminal/
└── ConsistencyTest.java          # Java vs Rust consistency tests
```

## Testing

### Consistency Test

The `ConsistencyTest` class compares Java and Rust output:

```java
// Java-only processing
TerminalEmulator javaEmulator = new TerminalEmulator(...);
TerminalEmulator.sForceDisableRust = true;
javaEmulator.append(input.getBytes(), length);

// Rust-enabled processing
TerminalEmulator rustEmulator = new TerminalEmulator(...);
TerminalEmulator.sForceDisableRust = false;
rustEmulator.append(input.getBytes(), length);

// Compare cursor position and screen content
Assert.assertEquals(javaEmulator.getCursorCol(), rustEmulator.getCursorCol());
Assert.assertEquals(javaEmulator.getCursorRow(), rustEmulator.getCursorRow());
```

### Manual Testing

Test applications that use various terminal features:

| Application | Features Used | Status |
|-------------|---------------|--------|
| `bash` | Basic I/O, cursor movement | ✅ Works |
| `vim` | Full screen, colors, cursor style | ❌ Broken |
| `nano` | Full screen, status bar | ❌ Broken |
| `htop` | Colors, bars, real-time update | ❌ Broken |
| `top` | Cursor movement, clearing | ⚠️ Partial |
| `mc` | Colors, mouse, full screen | ❌ Broken |
| `neofetch` | Colors, ASCII art | ⚠️ Partial |
| `tmux` | Alternate screen, colors | ⚠️ Partial |

## Configuration

### Enable/Disable Rust Features

```java
// In TerminalEmulator.java

// Force disable all Rust optimizations
TerminalEmulator.sForceDisableRust = true;

// Check if Rust library is loaded
boolean loaded = TerminalEmulator.isRustLibLoaded();
```

### Build Configuration

```toml
# In terminal-emulator/src/main/rust/Cargo.toml

[package]
edition = "2024"  # Requires Rust 1.85+

[dependencies]
jni = "0.21.1"
vte = "0.15.0"
unicode-width = "0.2.2"
```

## References

- [VT100 User Guide](https://vt100.net/docs/vt100-ug/)
- [Xterm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- [ANSI Escape Codes](https://en.wikipedia.org/wiki/ANSI_escape_code)
- [VTE Parser Documentation](https://docs.rs/vte/latest/vte/)
- [JNI Specification](https://docs.oracle.com/javase/8/docs/technotes/guides/jni/spec/jniTOC.html)

## Contact

For questions or contributions, please open an issue on the GitHub repository.
