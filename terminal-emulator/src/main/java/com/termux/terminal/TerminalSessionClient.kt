package com.termux.terminal

import androidx.annotation.NonNull
import androidx.annotation.Nullable

/**
 * Interface for receiving callbacks from a [TerminalSession].
 *
 * Used by the Rust JNI layer via [RustEngineCallback] to communicate
 * terminal events back to the UI/client layer.
 */
interface TerminalSessionClient {

    /** Called when the terminal text changes. */
    fun onTextChanged(@NonNull changedSession: TerminalSession)

    /** Called when the terminal title changes. */
    fun onTitleChanged(@NonNull changedSession: TerminalSession)

    /** Called when the session finishes (process exits). */
    fun onSessionFinished(@NonNull finishedSession: TerminalSession)

    /** Called when text should be copied to clipboard. */
    fun onCopyTextToClipboard(@NonNull session: TerminalSession, text: String)

    /** Called when text should be pasted from clipboard. */
    fun onPasteTextFromClipboard(@Nullable session: TerminalSession?)

    /** Called when a bell character is received. */
    fun onBell(@NonNull session: TerminalSession)

    /** Called when terminal colors change. */
    fun onColorsChanged(@NonNull session: TerminalSession)

    /** Called when cursor visibility changes. */
    fun onTerminalCursorStateChange(state: Boolean)

    /** Called when clear screen is requested. */
    fun onClearScreen() {}

    /** Called about a Sixel image. */
    fun onSixelImage(rgbaData: ByteArray?, width: Int, height: Int, startX: Int, startY: Int) {}

    /** Called to set the terminal shell PID. */
    fun setTerminalShellPid(@NonNull session: TerminalSession, pid: Int)

    /** Returns the terminal cursor style (nullable). */
    @Nullable
    fun getTerminalCursorStyle(): Int?

    // --- Logging methods ---

    fun logError(tag: String, message: String)
    fun logWarn(tag: String, message: String)
    fun logInfo(tag: String, message: String)
    fun logDebug(tag: String, message: String)
    fun logVerbose(tag: String, message: String)
    fun logStackTraceWithMessage(tag: String, message: String, e: Exception?)
    fun logStackTrace(tag: String, e: Exception?)

    // --- Optional convenience methods ---

    /** Report title change (convenience method). */
    fun reportTitleChange(title: String?) {}
}
