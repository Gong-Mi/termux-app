package com.termux.view.textselection

import android.content.ClipboardManager
import android.content.Context
import android.graphics.Rect
import android.os.Build
import android.text.TextUtils
import android.view.ActionMode
import android.view.Menu
import android.view.MenuItem
import android.view.MotionEvent
import android.view.View
import com.termux.terminal.WcWidth
import com.termux.view.R
import com.termux.view.TerminalView

class TextSelectionCursorController(
    private val terminalView: TerminalView
) : CursorController {

    companion object {
        private const val ACTION_COPY = 1
        private const val ACTION_PASTE = 2
        private const val ACTION_MORE = 3
    }

    private val mStartHandle: TextSelectionHandleView
    private val mEndHandle: TextSelectionHandleView
    private var mStoredSelectedText: String? = null
    private var mIsSelectingText = false
    private var mShowStartTime = System.currentTimeMillis()
    private val mHandleHeight: Int
    private var mSelX1 = -1
    private var mSelX2 = -1
    private var mSelY1 = -1
    private var mSelY2 = -1
    private var mActionMode: ActionMode? = null

    init {
        mStartHandle = TextSelectionHandleView(terminalView, this, TextSelectionHandleView.LEFT)
        mEndHandle = TextSelectionHandleView(terminalView, this, TextSelectionHandleView.RIGHT)
        mHandleHeight = maxOf(mStartHandle.handleHeight, mEndHandle.handleHeight)
    }

    override fun show(event: MotionEvent) {
        setInitialTextSelectionPosition(event)
        mStartHandle.positionAtCursor(mSelX1, mSelY1, true)
        mEndHandle.positionAtCursor(mSelX2 + 1, mSelY2, true)
        setActionModeCallBacks()
        mShowStartTime = System.currentTimeMillis()
        mIsSelectingText = true
    }

    override fun hide(): Boolean {
        if (!isActive()) return false
        if (System.currentTimeMillis() - mShowStartTime < 300) return false
        mStartHandle.hide()
        mEndHandle.hide()
        mActionMode?.finish()
        mSelX1 = -1
        mSelY1 = -1
        mSelX2 = -1
        mSelY2 = -1
        mIsSelectingText = false
        return true
    }

    override fun render() {
        if (!isActive()) return
        mStartHandle.positionAtCursor(mSelX1, mSelY1, false)
        mEndHandle.positionAtCursor(mSelX2 + 1, mSelY2, false)
        mActionMode?.invalidate()
    }

    private fun setInitialTextSelectionPosition(event: MotionEvent) {
        val columnAndRow = terminalView.getColumnAndRow(event, true)
        mSelX1 = columnAndRow[0]
        mSelX2 = columnAndRow[0]
        mSelY1 = columnAndRow[1]
        mSelY2 = columnAndRow[1]

        val emulator = terminalView.mEmulator ?: return
        val textAtCursor = emulator.getSelectedText(mSelX1, mSelY1, mSelX1, mSelY1)
        if (textAtCursor != null && textAtCursor != " ") {
            while (mSelX1 > 0) {
                val prev = emulator.getSelectedText(mSelX1 - 1, mSelY1, mSelX1 - 1, mSelY1)
                if (prev == null || prev.isEmpty() || prev == " ") break
                mSelX1--
            }
            while (mSelX2 < emulator.getCols() - 1) {
                val next = emulator.getSelectedText(mSelX2 + 1, mSelY1, mSelX2 + 1, mSelY1)
                if (next == null || next.isEmpty() || next == " ") break
                mSelX2++
            }
        }
    }

    private fun setActionModeCallBacks() {
        val callback = object : ActionMode.Callback {
            override fun onCreateActionMode(mode: ActionMode, menu: Menu): Boolean {
                val show = MenuItem.SHOW_AS_ACTION_IF_ROOM or MenuItem.SHOW_AS_ACTION_WITH_TEXT
                val clipboard = terminalView.context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                menu.add(Menu.NONE, ACTION_COPY, Menu.NONE, R.string.copy_text).setShowAsAction(show)
                menu.add(Menu.NONE, ACTION_PASTE, Menu.NONE, R.string.paste_text)
                    .setEnabled(clipboard != null && clipboard.hasPrimaryClip()).setShowAsAction(show)
                menu.add(Menu.NONE, ACTION_MORE, Menu.NONE, R.string.text_selection_more)
                return true
            }

            override fun onPrepareActionMode(mode: ActionMode, menu: Menu): Boolean = false

            override fun onActionItemClicked(mode: ActionMode, item: MenuItem): Boolean {
                if (!isActive) return true
                when (item.itemId) {
                    ACTION_COPY -> {
                        terminalView.mTermSession?.onCopyTextToClipboard(selectedText)
                        terminalView.stopTextSelectionMode()
                    }
                    ACTION_PASTE -> {
                        terminalView.stopTextSelectionMode()
                        terminalView.mTermSession?.onPasteTextFromClipboard()
                    }
                    ACTION_MORE -> {
                        mStoredSelectedText = selectedText
                        terminalView.stopTextSelectionMode()
                        terminalView.showContextMenu()
                    }
                }
                return true
            }

            override fun onDestroyActionMode(mode: ActionMode) {}
        }

        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            mActionMode = terminalView.startActionMode(callback)
            return
        }

        mActionMode = terminalView.startActionMode(object : ActionMode.Callback2() {
            override fun onCreateActionMode(mode: ActionMode, menu: Menu): Boolean =
                callback.onCreateActionMode(mode, menu)

            override fun onPrepareActionMode(mode: ActionMode, menu: Menu): Boolean = false

            override fun onActionItemClicked(mode: ActionMode, item: MenuItem): Boolean =
                callback.onActionItemClicked(mode, item)

            override fun onDestroyActionMode(mode: ActionMode) {}

            override fun onGetContentRect(mode: ActionMode, view: View, outRect: Rect) {
                var x1 = terminalView.getPointX(mSelX1)
                var x2 = terminalView.getPointX(mSelX2)
                val y1 = terminalView.getPointY(mSelY1 - 1)
                val y2 = terminalView.getPointY(mSelY2 + 1)
                if (x1 > x2) {
                    val tmp = x1; x1 = x2; x2 = tmp
                }
                val terminalBottom = terminalView.bottom
                var top = y1 + mHandleHeight
                var bottom = y2 + mHandleHeight
                if (top > terminalBottom) top = terminalBottom
                if (bottom > terminalBottom) bottom = terminalBottom
                outRect.set(x1, top, x2, bottom)
            }
        }, ActionMode.TYPE_FLOATING)
    }

    override fun updatePosition(handle: TextSelectionHandleView, x: Int, y: Int) {
        val emulator = terminalView.mEmulator ?: return
        val scrollRows = emulator.getActiveTranscriptRows()
        if (handle == mStartHandle) {
            mSelX1 = terminalView.getCursorX(x.toFloat())
            mSelY1 = terminalView.getCursorY(y.toFloat())
            if (mSelX1 < 0) mSelX1 = 0
            if (mSelY1 < -scrollRows) mSelY1 = -scrollRows
            else if (mSelY1 > emulator.getRows() - 1) mSelY1 = emulator.getRows() - 1
            if (mSelY1 > mSelY2) mSelY1 = mSelY2
            if (mSelY1 == mSelY2 && mSelX1 > mSelX2) mSelX1 = mSelX2
            if (!emulator.isAlternateBufferActive()) {
                var topRow = terminalView.mTopRow
                if (mSelY1 <= topRow) {
                    topRow--
                    if (topRow < -scrollRows) topRow = -scrollRows
                } else if (mSelY1 >= topRow + emulator.getRows()) {
                    topRow++
                    if (topRow > 0) topRow = 0
                }
                terminalView.mTopRow = topRow
            }
            mSelX1 = getValidCurX(emulator, mSelY1, mSelX1)
        } else {
            mSelX2 = terminalView.getCursorX(x.toFloat())
            mSelY2 = terminalView.getCursorY(y.toFloat())
            if (mSelX2 < 0) mSelX2 = 0
            if (mSelY2 < -scrollRows) mSelY2 = -scrollRows
            else if (mSelY2 > emulator.getRows() - 1) mSelY2 = emulator.getRows() - 1
            if (mSelY1 > mSelY2) mSelY2 = mSelY1
            if (mSelY1 == mSelY2 && mSelX1 > mSelX2) mSelX2 = mSelX1
            if (!emulator.isAlternateBufferActive()) {
                var topRow = terminalView.mTopRow
                if (mSelY2 <= topRow) {
                    topRow--
                    if (topRow < -scrollRows) topRow = -scrollRows
                } else if (mSelY2 >= topRow + emulator.getRows()) {
                    topRow++
                    if (topRow > 0) topRow = 0
                }
                terminalView.mTopRow = topRow
            }
            mSelX2 = getValidCurX(emulator, mSelY2, mSelX2)
        }
        terminalView.invalidate()
    }

    private fun getValidCurX(emulator: com.termux.terminal.TerminalEmulator, cy: Int, cx: Int): Int {
        val line = emulator.getSelectedText(0, cy, cx, cy)
        if (!TextUtils.isEmpty(line)) {
            var col = 0
            var i = 0
            val len = line.length
            while (i < len) {
                val ch1 = line[i]
                if (ch1.code == 0) break
                val wc: Int
                if (Character.isHighSurrogate(ch1) && i + 1 < len) {
                    wc = WcWidth.width(Character.toCodePoint(ch1, line[i + 1]))
                    i += 2
                } else {
                    wc = WcWidth.width(ch1.code)
                    i++
                }
                val cend = col + wc
                if (cx > col && cx < cend) return cend
                if (cend == col) return col
                col = cend
            }
        }
        return cx
    }

    fun decrementYTextSelectionCursors(decrement: Int) {
        mSelY1 -= decrement
        mSelY2 -= decrement
    }

    override fun onTouchEvent(event: MotionEvent): Boolean = false

    override fun onTouchModeChanged(isInTouchMode: Boolean) {
        if (!isInTouchMode) terminalView.stopTextSelectionMode()
    }

    override fun onDetached() {}

    override fun isActive(): Boolean = mIsSelectingText

    fun getSelectors(sel: IntArray) {
        if (sel.size != 4) return
        sel[0] = mSelY1
        sel[1] = mSelY2
        sel[2] = mSelX1
        sel[3] = mSelX2
    }

    val selectedText: String
        get() = terminalView.mEmulator?.getSelectedText(mSelX1, mSelY1, mSelX2, mSelY2) ?: ""

    fun getStoredSelectedText(): String? = mStoredSelectedText

    fun unsetStoredSelectedText() { mStoredSelectedText = null }

    val actionMode: ActionMode? get() = mActionMode

    val isSelectionStartDragged: Boolean get() = mStartHandle.isDragging
    val isSelectionEndDragged: Boolean get() = mEndHandle.isDragging
}
