package com.termux.terminal

import java.nio.charset.StandardCharsets

/**
 * A client which receives callbacks from events triggered by feeding input to a [TerminalEmulator].
 *
 * @deprecated Callbacks are now dispatched via Rust's [RustEngineCallback] interface through JNI.
 * This class is kept for backward compatibility only.
 */
@Deprecated("Callbacks now use RustEngineCallback via JNI")
abstract class TerminalOutput {

    /** Write a string using the UTF-8 encoding to the terminal client. */
    fun write(data: String) {
        if (data == null) return
        val bytes = data.toByteArray(StandardCharsets.UTF_8)
        write(bytes, 0, bytes.size)
    }

    /** Write bytes to the terminal client. */
    abstract fun write(data: ByteArray, offset: Int, count: Int)

    /** Write bytes to the terminal client (convenience method). */
    open fun write(data: ByteArray) {
        if (data == null) return
        write(data, 0, data.size)
    }

    /** Notify the terminal client that the terminal title has changed. */
    abstract fun titleChanged(oldTitle: String?, newTitle: String?)

    /** Notify the terminal client that text should be copied to clipboard. */
    abstract fun onCopyTextToClipboard(text: String?)

    /** Notify the terminal client that text should be pasted from clipboard. */
    abstract fun onPasteTextFromClipboard()

    /** Notify the terminal client that a bell character has been received. */
    abstract fun onBell()

    abstract fun onColorsChanged()

    /** Notify the terminal client that the terminal cursor visibility has changed. */
    abstract fun onTerminalCursorStateChange(visible: Boolean)

    /** Notify the terminal client about a Sixel image. */
    open fun onSixelImage(rgbaData: ByteArray?, width: Int, height: Int, startX: Int, startY: Int) {}
}
