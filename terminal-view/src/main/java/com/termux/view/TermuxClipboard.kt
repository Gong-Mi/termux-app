package com.termux.view

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context

/**
 * Kotlin implementation of Termux clipboard "duties".
 * This class only handles the interaction with Android System API.
 * The core logic (sanitization, protocol handling) remains in Rust.
 */
object TermuxClipboard {

    /**
     * Get text from the system clipboard.
     */
    @JvmStatic
    fun getText(context: Context): String? {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        val clip = clipboard?.primaryClip ?: return null
        if (clip.itemCount > 0) {
            return clip.getItemAt(0).text?.toString()
        }
        return null
    }

    /**
     * Set text to the system clipboard.
     */
    @JvmStatic
    fun setText(context: Context, text: String?) {
        if (text == null) return
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        val clip = ClipData.newPlainText("Termux", text)
        clipboard?.setPrimaryClip(clip)
    }
}
