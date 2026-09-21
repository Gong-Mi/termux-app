package com.termux.terminal;

/**
 * Compile-only stand-in for the real `TerminalSession`.
 *
 * The real class owns a PTY and needs the Android framework (`Handler`, `Looper`, ...), which
 * the oracle harness must not require. Upstream `TerminalEmulator` never touches a
 * `TerminalSession` directly -- sessions only appear in `TerminalSessionClient` callback
 * signatures -- so an empty type is enough for the reference implementation to compile and
 * run its terminal-state semantics unchanged.
 */
public class TerminalSession {
}