package com.termux.view.textselection

import android.view.MotionEvent
import android.view.ViewTreeObserver

/**
 * A CursorController instance can be used to control cursors in the text.
 * Not used outside of [TerminalView].
 */
interface CursorController : ViewTreeObserver.OnTouchModeChangeListener {

    /** Show the cursors on screen. Will be drawn by [render] during onDraw. */
    fun show(event: MotionEvent)

    /** Hide the cursors from screen. */
    fun hide(): Boolean

    /** Render the cursors. */
    fun render()

    /** Update the cursor positions. */
    fun updatePosition(handle: TextSelectionHandleView, x: Int, y: Int)

    /** Called when the view is detached from window. */
    fun onDetached()

    /** @return true if the cursors are currently active. */
    fun isActive(): Boolean
}
