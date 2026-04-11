package com.termux.view

import android.annotation.SuppressLint
import android.annotation.TargetApi
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.text.Editable
import android.text.InputType
import android.text.TextUtils
import android.util.AttributeSet
import android.util.Log
import android.view.*
import android.view.accessibility.AccessibilityManager
import android.view.autofill.AutofillManager
import android.view.autofill.AutofillValue
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.Scroller
import androidx.annotation.RequiresApi
import com.termux.terminal.TerminalEmulator
import com.termux.terminal.TerminalSession
import com.termux.view.textselection.TextSelectionCursorController

/** View displaying and interacting with a [TerminalSession]. */
class TerminalView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null
) : SurfaceView(context, attrs), SurfaceHolder.Callback {

    companion object {
        private var TERMINAL_VIEW_KEY_LOGGING_ENABLED = false
        private const val LOG_TAG = "TerminalView"

        const val KEY_EVENT_SOURCE_VIRTUAL_KEYBOARD = KeyCharacterMap.VIRTUAL_KEYBOARD
        const val KEY_EVENT_SOURCE_SOFT_KEYBOARD = 0

        const val TERMINAL_CURSOR_BLINK_RATE_MIN = 100
        const val TERMINAL_CURSOR_BLINK_RATE_MAX = 2000

        init {
            try {
                System.loadLibrary("termux_rust")
                Log.i("TerminalView", "libtermux_rust.so loaded successfully")
            } catch (e: UnsatisfiedLinkError) {
                Log.e("TerminalView", "!!! FATAL: Failed to load libtermux_rust.so: ${e.message}")
                throw e
            }
        }
    }

    // --- JNI methods ---
    external fun nativeSetSurface(surface: android.view.Surface?)
    external fun nativeSetEnginePointer(enginePtr: Long)
    external fun nativeUpdateRenderParams(
        scale: Float, scrollOffset: Float, topRow: Int,
        selX1: Int, selY1: Int, selX2: Int, selY2: Int, selActive: Boolean
    )
    external fun nativeOnSizeChanged(width: Int, height: Int)
    external fun nativeSetFontSize(fontSize: Float)
    external fun nativeGetFontMetrics(metrics: FloatArray)

    // --- State ---
    var mTermSession: TerminalSession? = null
    @JvmField
    var mEmulator: TerminalEmulator? = null
    var mClient: TerminalViewClient? = null
    var mTopRow: Int = 0

    private var mNativeFontWidth = 1.0f
    private var mNativeFontHeight = 1.0f
    private var mNativeFontAscent = 0f
    private val mNativeFontMetricsBuffer = FloatArray(3)
    private val mSelCoords = IntArray(4)

    private var mSixelImageData: ByteArray? = null
    private var mSixelWidth = 0
    private var mSixelHeight = 0
    private var mSixelStartX = 0
    private var mSixelStartY = 0
    private var mSixelBitmap: Bitmap? = null
    private val mSixelPaint = Paint(Paint.FILTER_BITMAP_FLAG).apply {
        isAntiAlias = true
        isDither = true
    }

    private var mTextSelectionCursorController: TextSelectionCursorController? = null
    private var mTerminalCursorBlinkerHandler: Handler? = null
    private var mTerminalCursorBlinkerRunnable: TerminalCursorBlinkerRunnable? = null
    private var mTerminalCursorBlinkerRate = 0
    private var mCursorInvisibleIgnoreOnce = false

    private var mScaleFactor = 1f
    private lateinit var mGestureRecognizer: GestureAndScaleRecognizer
    private lateinit var mScroller: Scroller
    private var mMouseScrollStartX = -1
    private var mMouseScrollStartY = -1
    private var mMouseStartDownTime = -1L
    private var mScrollRemainder = 0f
    private var mCombiningAccent = 0
    private val mAccessibilityEnabled: Boolean

    private var mEnginePointerSet = false
    private var mOnDrawCalledAtLeastOnce = false
    private var mLastInvalidateTime = 0L
    private var mMinInvalidateInterval = 16L
    private var mInvalidatePending = false
    private var mLastUpdateSizeTime = 0L

    @RequiresApi(Build.VERSION_CODES.O)
    private var mAutoFillType = AUTOFILL_TYPE_NONE

    @RequiresApi(Build.VERSION_CODES.O)
    private var mAutoFillImportance = IMPORTANT_FOR_AUTOFILL_NO

    private var mAutoFillHints = emptyArray<String>()

    private val mInvalidateRunnable = Runnable {
        mInvalidatePending = false
        invalidate()
    }

    private val mUpdateSizeRunnable = Runnable { updateSizeInternal() }

    private var scrolledWithFinger = false

    init {
        setWillNotDraw(false)
        holder.addCallback(this)
        updateRefreshRate(context)

        mGestureRecognizer = GestureAndScaleRecognizer(context, object : GestureAndScaleRecognizer.Listener {
            override fun onUp(event: MotionEvent): Boolean {
                mScrollRemainder = 0f
                val emu = mEmulator
                if (emu != null && emu.isMouseTrackingActive() &&
                    !event.isFromSource(InputDevice.SOURCE_MOUSE) &&
                    !isSelectingText() && !scrolledWithFinger) {
                    sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON, true)
                    sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON, false)
                    return true
                }
                scrolledWithFinger = false
                return false
            }

            override fun onSingleTapUp(event: MotionEvent): Boolean {
                if (mEmulator == null) return true
                if (isSelectingText()) {
                    stopTextSelectionMode()
                    return true
                }
                requestFocus()
                mClient?.onSingleTapUp(event)
                return true
            }

            override fun onScroll(e: MotionEvent, distanceX: Float, distanceY: Float): Boolean {
                val emu = mEmulator ?: return true
                if (emu.isMouseTrackingActive() && e.isFromSource(InputDevice.SOURCE_MOUSE)) {
                    sendMouseEventCode(e, TerminalEmulator.MOUSE_LEFT_BUTTON_MOVED, true)
                } else {
                    scrolledWithFinger = true
                    val distY = distanceY + mScrollRemainder
                    val deltaRows = (distY / getFontLineSpacing()).toInt()
                    mScrollRemainder = distY - deltaRows * getFontLineSpacing()
                    doScroll(e, deltaRows)
                }
                return true
            }

            override fun onScale(focusX: Float, focusY: Float, scale: Float): Boolean {
                if (mEmulator == null || isSelectingText()) return true
                mScaleFactor *= scale
                mScaleFactor = mClient?.onScale(mScaleFactor) ?: mScaleFactor
                invalidate()
                return true
            }

            override fun onFling(e2: MotionEvent, velocityX: Float, velocityY: Float): Boolean {
                val emu = mEmulator ?: return true
                if (!mScroller.isFinished) return true
                val mouseTracking = emu.isMouseTrackingActive()
                val SCALE = 0.25f
                if (mouseTracking) {
                    mScroller.fling(0, 0, 0, -(velocityY * SCALE).toInt(), 0, 0, -emu.getCols() / 2, emu.getCols() / 2)
                } else {
                    mScroller.fling(0, mTopRow, 0, -(velocityY * SCALE).toInt(), 0, 0, -emu.getActiveTranscriptRows(), 0)
                }
                post(object : Runnable {
                    var mLastY = 0
                    override fun run() {
                        if (mouseTracking != mEmulator?.isMouseTrackingActive()) {
                            mScroller.abortAnimation()
                            return
                        }
                        if (mScroller.isFinished) return
                        val more = mScroller.computeScrollOffset()
                        val newY = mScroller.currY
                        val diff = if (mouseTracking) (newY - mLastY) else (newY - mTopRow)
                        doScroll(e2, diff)
                        mLastY = newY
                        if (more) post(this)
                    }
                })
                return true
            }

            override fun onDown(x: Float, y: Float): Boolean = false
            override fun onDoubleTap(e: MotionEvent): Boolean = false

            override fun onLongPress(event: MotionEvent) {
                if (mGestureRecognizer.isInProgress()) return
                if (mClient?.onLongPress(event) == true) return
                if (!isSelectingText()) {
                    performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                    startTextSelectionMode(event)
                }
            }
        })

        mScroller = Scroller(context)
        val am = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        mAccessibilityEnabled = am.isEnabled
    }

    fun setTerminalViewClient(client: TerminalViewClient) { mClient = client }
    fun setIsTerminalViewKeyLoggingEnabled(value: Boolean) { TERMINAL_VIEW_KEY_LOGGING_ENABLED = value }

    fun attachSession(session: TerminalSession?): Boolean {
        if (session === mTermSession) return false
        mTopRow = 0
        mTermSession = session
        mEmulator = null
        mCombiningAccent = 0
        updateSize()
        isVerticalScrollBarEnabled = true
        return true
    }

    private fun getFontWidth(): Float = if (mNativeFontWidth > 0) mNativeFontWidth else 1.0f
    private fun getFontLineSpacing(): Float = if (mNativeFontHeight > 0) mNativeFontHeight else 1.0f
    private fun getFontLineSpacingAndAscent(): Float = mNativeFontHeight + mNativeFontAscent

    private fun refreshFontMetrics() {
        nativeGetFontMetrics(mNativeFontMetricsBuffer)
        mNativeFontWidth = mNativeFontMetricsBuffer[0]
        mNativeFontHeight = mNativeFontMetricsBuffer[1]
        mNativeFontAscent = mNativeFontMetricsBuffer[2]
    }

    private fun updateRefreshRate(context: Context) {
        try {
            val refreshRate = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                context.display.refreshRate
            } else {
                val wm = context.getSystemService(Context.WINDOW_SERVICE) as android.view.WindowManager
                wm.defaultDisplay.refreshRate
            }
            if (refreshRate > 0) mMinInvalidateInterval = (1000 / refreshRate).toLong()
        } catch (_: Exception) {
            mMinInvalidateInterval = 16
        }
    }

    override fun invalidate() {
        if (mInvalidatePending) return
        val currentTime = SystemClock.elapsedRealtime()
        val timeSinceLast = currentTime - mLastInvalidateTime
        if (timeSinceLast >= mMinInvalidateInterval) {
            mLastInvalidateTime = currentTime
            mInvalidatePending = false
            removeCallbacks(mInvalidateRunnable)
            super.invalidate()
        } else {
            mInvalidatePending = true
            postDelayed(mInvalidateRunnable, mMinInvalidateInterval - timeSinceLast)
        }
    }

    override fun invalidate(l: Int, t: Int, r: Int, b: Int) { invalidate() }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        if (mClient?.isTerminalViewSelected() == true) {
            outAttrs.inputType = if (mClient?.shouldEnforceCharBasedInput() == true) {
                InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            } else {
                InputType.TYPE_NULL
            }
        } else {
            outAttrs.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_NORMAL
        }
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN

        return object : BaseInputConnection(this, true) {
            override fun finishComposingText(): Boolean {
                if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logInfo(LOG_TAG, "IME: finishComposingText()")
                super.finishComposingText()
                sendTextToTerminal(editable ?: "")
                editable?.clear()
                return true
            }

            override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
                if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) {
                    mClient?.logInfo(LOG_TAG, "IME: commitText(\"$text\", $newCursorPosition)")
                }
                super.commitText(text, newCursorPosition)
                if (mEmulator == null) return true
                val content = editable
                if (content != null) {
                    sendTextToTerminal(content)
                    content.clear()
                }
                return true
            }

            override fun deleteSurroundingText(leftLength: Int, rightLength: Int): Boolean {
                if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) {
                    mClient?.logInfo(LOG_TAG, "IME: deleteSurroundingText($leftLength, $rightLength)")
                }
                val deleteKey = KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL)
                for (i in 0 until leftLength) sendKeyEvent(deleteKey)
                return super.deleteSurroundingText(leftLength, rightLength)
            }

            private fun sendTextToTerminal(text: CharSequence) {
                stopTextSelectionMode()
                val textLengthInChars = text.length
                var i = 0
                while (i < textLengthInChars) {
                    val firstChar = text[i]
                    val codePoint: Int
                    if (Character.isHighSurrogate(firstChar)) {
                        if (++i < textLengthInChars) {
                            codePoint = Character.toCodePoint(firstChar, text[i])
                        } else {
                            codePoint = TerminalEmulator.UNICODE_REPLACEMENT_CHAR
                        }
                    } else {
                        codePoint = firstChar.code
                    }
                    val finalCodePoint = if (mClient?.readShiftKey() == true) Character.toUpperCase(codePoint) else codePoint
                    var ctrlHeld = false
                    var cp = finalCodePoint
                    if (cp <= 31 && cp != 27) {
                        if (cp == '\n'.code) cp = '\r'.code
                        ctrlHeld = true
                        cp = when (cp) {
                            31 -> '_'.code
                            30 -> '^'.code
                            29 -> ']'.code
                            28 -> '\\'.code
                            else -> cp + 96
                        }
                    }
                    inputCodePoint(KEY_EVENT_SOURCE_SOFT_KEYBOARD, cp, ctrlHeld, false)
                    i++
                }
            }
        }
    }

    override fun computeVerticalScrollRange(): Int = mEmulator?.getActiveRows() ?: 1
    override fun computeVerticalScrollExtent(): Int = mEmulator?.getRows() ?: 1
    override fun computeVerticalScrollOffset(): Int = mEmulator?.let { it.getActiveRows() + mTopRow - it.getRows() } ?: 1

    fun onScreenUpdated() = onScreenUpdated(false)

    fun onScreenUpdated(skipScrolling: Boolean) {
        val emu = mEmulator
        if (emu == null) {
            Log.w("TerminalView-Engine", "onScreenUpdated called but mEmulator is null")
            return
        }
        if (!mEnginePointerSet) {
            mEnginePointerSet = true
            val enginePtr = emu.getNativePointer()
            Log.i("TerminalView-Engine", ">>> FIRST onScreenUpdated - Calling nativeSetEnginePointer with ptr=$enginePtr")
            nativeSetEnginePointer(enginePtr)
        }
        val rowsInHistory = emu.getActiveTranscriptRows()
        if (mTopRow < -rowsInHistory) mTopRow = -rowsInHistory

        var skipping = skipScrolling
        if (isSelectingText() || emu.isAutoScrollDisabled()) {
            val rowShift = emu.getScrollCounter()
            if (-mTopRow + rowShift > rowsInHistory) {
                if (isSelectingText()) stopTextSelectionMode()
                if (emu.isAutoScrollDisabled()) { mTopRow = -rowsInHistory; skipping = true }
            } else {
                skipping = true
                mTopRow -= rowShift
                decrementYTextSelectionCursors(rowShift)
            }
        }
        if (!skipping && mTopRow != 0) {
            if (mTopRow < -3) awakenScrollBars()
            mTopRow = 0
        }
        emu.clearScrollCounter()
        invalidate()
        if (mAccessibilityEnabled) contentDescription = text
    }

    fun onContextMenuClosed(menu: Menu) { unsetStoredSelectedText() }

    fun setTextSize(textSize: Int) {
        mScaleFactor = 1.0f
        nativeSetFontSize(textSize.toFloat())
        refreshFontMetrics()
        updateSize()
    }

    fun setTypeface(newTypeface: Typeface?) {
        refreshFontMetrics()
        updateSize()
        invalidate()
    }

    override fun onCheckIsTextEditor(): Boolean = true
    override fun isOpaque(): Boolean = true

    fun getColumnAndRow(event: MotionEvent, relativeToScroll: Boolean): IntArray {
        val column = (event.x / getFontWidth()).toInt()
        var row = ((event.y - getFontLineSpacingAndAscent()) / getFontLineSpacing()).toInt()
        if (relativeToScroll) row += mTopRow
        return intArrayOf(column, row)
    }

    private fun sendMouseEventCode(e: MotionEvent, button: Int, pressed: Boolean) {
        val emu = mEmulator ?: return
        val columnAndRow = getColumnAndRow(e, false)
        var x = columnAndRow[0] + 1
        var y = columnAndRow[1] + 1
        if (pressed && (button == TerminalEmulator.MOUSE_WHEELDOWN_BUTTON || button == TerminalEmulator.MOUSE_WHEELUP_BUTTON)) {
            if (mMouseStartDownTime == e.downTime) {
                x = mMouseScrollStartX
                y = mMouseScrollStartY
            } else {
                mMouseStartDownTime = e.downTime
                mMouseScrollStartX = x
                mMouseScrollStartY = y
            }
        }
        emu.sendMouseEvent(button, x, y, pressed)
    }

    private fun doScroll(event: MotionEvent, rowsDown: Int) {
        val emu = mEmulator ?: return
        val up = rowsDown < 0
        repeat(Math.abs(rowsDown)) {
            if (emu.isMouseTrackingActive()) {
                sendMouseEventCode(event, if (up) TerminalEmulator.MOUSE_WHEELUP_BUTTON else TerminalEmulator.MOUSE_WHEELDOWN_BUTTON, true)
            } else if (emu.isAlternateBufferActive()) {
                handleKeyCode(if (up) KeyEvent.KEYCODE_DPAD_UP else KeyEvent.KEYCODE_DPAD_DOWN, 0)
            } else {
                mTopRow = Math.min(0, Math.max(-emu.getActiveTranscriptRows(), mTopRow + if (up) -1 else 1))
                if (!awakenScrollBars()) invalidate()
            }
        }
    }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (mEmulator != null && event.isFromSource(InputDevice.SOURCE_MOUSE) && event.action == MotionEvent.ACTION_SCROLL) {
            val up = event.getAxisValue(MotionEvent.AXIS_VSCROLL) > 0f
            doScroll(event, if (up) -3 else 3)
            return true
        }
        return false
    }

    @SuppressLint("ClickableViewAccessibility")
    @TargetApi(23)
    override fun onTouchEvent(event: MotionEvent): Boolean {
        val emu = mEmulator ?: return true
        val action = event.action
        if (isSelectingText()) {
            updateFloatingToolbarVisibility(event)
            mGestureRecognizer.onTouchEvent(event)
            return true
        }
        if (event.isFromSource(InputDevice.SOURCE_MOUSE)) {
            if (event.isButtonPressed(MotionEvent.BUTTON_SECONDARY)) {
                if (action == MotionEvent.ACTION_DOWN) showContextMenu()
                return true
            }
            if (event.isButtonPressed(MotionEvent.BUTTON_TERTIARY)) {
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                val clipData = clipboard.primaryClip
                val text = clipData?.getItemAt(0)?.coerceToText(context)
                if (!TextUtils.isEmpty(text)) emu.paste(text.toString())
            }
            if (emu.isMouseTrackingActive()) {
                when (event.action) {
                    MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP ->
                        sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON, event.action == MotionEvent.ACTION_DOWN)
                    MotionEvent.ACTION_MOVE ->
                        sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON_MOVED, true)
                }
            }
        }
        mGestureRecognizer.onTouchEvent(event)
        return true
    }

    override fun onKeyPreIme(keyCode: Int, event: KeyEvent): Boolean {
        if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logInfo(LOG_TAG, "onKeyPreIme(keyCode=$keyCode, event=$event)")
        val emu = mEmulator
        if (emu != null) {
            if (keyCode == KeyEvent.KEYCODE_BACK) {
                cancelRequestAutoFill()
                if (isSelectingText()) {
                    stopTextSelectionMode()
                    return true
                }
                if (mClient?.shouldBackButtonBeMappedToEscape() == true) {
                    return when (event.action) {
                        KeyEvent.ACTION_DOWN -> onKeyDown(keyCode, event)
                        KeyEvent.ACTION_UP -> onKeyUp(keyCode, event)
                        else -> false
                    }
                }
            }
            if (mClient?.shouldUseCtrlSpaceWorkaround() == true && keyCode == KeyEvent.KEYCODE_SPACE && event.isCtrlPressed) {
                return onKeyDown(keyCode, event)
            }
        }
        return super.onKeyPreIme(keyCode, event)
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logInfo(LOG_TAG, "onKeyDown(keyCode=$keyCode, isSystem=${event.isSystem}, event=$event)")
        val emu = mEmulator ?: return true
        if (isSelectingText()) stopTextSelectionMode()

        if (mClient?.onKeyDown(keyCode, event, mTermSession) == true) {
            invalidate()
            return true
        }
        if (event.isSystem && !(mClient?.shouldBackButtonBeMappedToEscape() == true) && keyCode != KeyEvent.KEYCODE_BACK) {
            return super.onKeyDown(keyCode, event)
        }
        if (event.action == KeyEvent.ACTION_MULTIPLE && keyCode == KeyEvent.KEYCODE_UNKNOWN) {
            mTermSession?.write(event.characters)
            return true
        }
        if (keyCode == KeyEvent.KEYCODE_LANGUAGE_SWITCH) return super.onKeyDown(keyCode, event)

        val metaState = event.metaState
        val controlDown = event.isCtrlPressed || (mClient?.readControlKey() == true)
        val leftAltDown = (metaState and KeyEvent.META_ALT_LEFT_ON) != 0 || (mClient?.readAltKey() == true)
        val shiftDown = event.isShiftPressed || (mClient?.readShiftKey() == true)
        val rightAltDownFromEvent = (metaState and KeyEvent.META_ALT_RIGHT_ON) != 0

        var keyMod = 0
        if (controlDown) keyMod = keyMod or 0x40000000.toInt()
        if (event.isAltPressed || leftAltDown) keyMod = keyMod or 0x80000000.toInt()
        if (shiftDown) keyMod = keyMod or 0x20000000.toInt()
        if (event.isNumLockOn) keyMod = keyMod or 0x10000000.toInt()

        if (!event.isFunctionPressed && handleKeyCode(keyCode, keyMod)) {
            if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logInfo(LOG_TAG, "handleKeyCode() took key event")
            return true
        }

        var bitsToClear = KeyEvent.META_CTRL_MASK
        if (!rightAltDownFromEvent) bitsToClear = bitsToClear or KeyEvent.META_ALT_ON or KeyEvent.META_ALT_LEFT_ON
        var effectiveMetaState = event.metaState and bitsToClear.inv()
        if (shiftDown) effectiveMetaState = effectiveMetaState or KeyEvent.META_SHIFT_ON or KeyEvent.META_SHIFT_LEFT_ON
        if (mClient?.readFnKey() == true) effectiveMetaState = effectiveMetaState or KeyEvent.META_FUNCTION_ON

        val result = event.getUnicodeChar(effectiveMetaState)
        if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logInfo(LOG_TAG, "KeyEvent#getUnicodeChar($effectiveMetaState) returned: $result")
        if (result == 0) return false

        val oldCombiningAccent = mCombiningAccent
        if ((result and KeyCharacterMap.COMBINING_ACCENT) != 0) {
            if (mCombiningAccent != 0) inputCodePoint(event.deviceId, mCombiningAccent, controlDown, leftAltDown)
            mCombiningAccent = result and KeyCharacterMap.COMBINING_ACCENT_MASK
        } else {
            if (mCombiningAccent != 0) {
                val combinedChar = KeyCharacterMap.getDeadChar(mCombiningAccent, result)
                if (combinedChar > 0) mCombiningAccent = combinedChar
                mCombiningAccent = 0
            }
            inputCodePoint(event.deviceId, result, controlDown, leftAltDown)
        }
        if (mCombiningAccent != oldCombiningAccent) invalidate()
        return true
    }

    fun inputCodePoint(eventSource: Int, codePoint: Int, controlDownFromEvent: Boolean, leftAltDownFromEvent: Boolean) {
        if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) {
            mClient?.logInfo(LOG_TAG, "inputCodePoint(eventSource=$eventSource, codePoint=$codePoint, controlDown=$controlDownFromEvent, leftAltDown=$leftAltDownFromEvent)")
        }
        val session = mTermSession ?: return
        mEmulator?.setCursorBlinkState(true)
        val controlDown = controlDownFromEvent || (mClient?.readControlKey() == true)
        val altDown = leftAltDownFromEvent || (mClient?.readAltKey() == true)
        if (mClient?.onCodePoint(codePoint, controlDown, session) == true) return

        var cp = codePoint
        if (controlDown) {
            cp = when {
                cp in 'a'.code..'z'.code -> cp - 'a'.code + 1
                cp in 'A'.code..'Z'.code -> cp - 'A'.code + 1
                cp == ' '.code || cp == '2'.code -> 0
                cp == '['.code || cp == '3'.code -> 27
                cp == '\\'.code || cp == '4'.code -> 28
                cp == ']'.code || cp == '5'.code -> 29
                cp == '^'.code || cp == '6'.code -> 30
                cp == '_'.code || cp == '7'.code || cp == '/'.code -> 31
                cp == '8'.code -> 127
                else -> cp
            }
        }
        if (cp > -1) {
            if (eventSource > KEY_EVENT_SOURCE_SOFT_KEYBOARD) {
                cp = when (cp) {
                    0x02DC -> 0x007E
                    0x02CB -> 0x0060
                    0x02C6 -> 0x005E
                    else -> cp
                }
            }
            session.writeCodePoint(altDown, cp)
        }
    }

    fun handleKeyCode(keyCode: Int, keyMod: Int): Boolean {
        mEmulator?.setCursorBlinkState(true)
        if (handleKeyCodeAction(keyCode, keyMod)) return true
        val seq = mEmulator?.sendKeyEvent(keyCode, keyMod)
        if (seq != null) {
            mTermSession?.write(seq)
            return true
        }
        return false
    }

    fun handleKeyCodeAction(keyCode: Int, keyMod: Int): Boolean {
        val shiftDown = (keyMod and 0x20000000.toInt()) != 0
        if ((keyCode == KeyEvent.KEYCODE_PAGE_UP || keyCode == KeyEvent.KEYCODE_PAGE_DOWN) && shiftDown) {
            val time = SystemClock.uptimeMillis()
            val motionEvent = MotionEvent.obtain(time, time, MotionEvent.ACTION_DOWN, 0f, 0f, 0)
            val rows = mEmulator?.getRows() ?: 24
            doScroll(motionEvent, if (keyCode == KeyEvent.KEYCODE_PAGE_UP) -rows else rows)
            motionEvent.recycle()
            return true
        }
        return false
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logInfo(LOG_TAG, "onKeyUp(keyCode=$keyCode, event=$event)")
        if (mEmulator == null && keyCode != KeyEvent.KEYCODE_BACK) return true
        if (mClient?.onKeyUp(keyCode, event) == true) {
            invalidate()
            return true
        }
        if (event.isSystem) return super.onKeyUp(keyCode, event)
        return true
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) { updateSize() }

    fun updateSize() {
        val currentTime = SystemClock.elapsedRealtime()
        if (currentTime - mLastUpdateSizeTime < 50) {
            removeCallbacks(mUpdateSizeRunnable)
            postDelayed(mUpdateSizeRunnable, 50)
            return
        }
        mLastUpdateSizeTime = currentTime
        updateSizeInternal()
    }

    private fun updateSizeInternal() {
        val viewWidth = width
        val viewHeight = height
        val session = mTermSession
        if (viewWidth == 0 || viewHeight == 0 || session == null) return
        val newColumns = Math.max(4, (viewWidth / (getFontWidth() * mScaleFactor)).toInt())
        val newRows = Math.max(4, ((viewHeight / mScaleFactor - getFontLineSpacingAndAscent()) / getFontLineSpacing()).toInt())

        if (!session.isEngineInitialized()) {
            session.updateSize(newColumns, newRows, (getFontWidth() * mScaleFactor).toInt(), (getFontLineSpacing() * mScaleFactor).toInt())
            return
        }
        val emu = mEmulator
        if (emu == null || newColumns != emu.getCols() || newRows != emu.getRows()) {
            session.updateSize(newColumns, newRows, (getFontWidth() * mScaleFactor).toInt(), (getFontLineSpacing() * mScaleFactor).toInt())
            mEmulator = session.mEmulator
            mClient?.onEmulatorSet()
            mTerminalCursorBlinkerRunnable?.setEmulator(mEmulator)
            mTopRow = 0
            scrollTo(0, 0)
            invalidate()
        }
    }

    override fun onScrollChanged(l: Int, t: Int, oldl: Int, oldt: Int) {
        super.onScrollChanged(l, t, oldl, oldt)
        if (mSixelBitmap != null && !mSixelBitmap!!.isRecycled) invalidate()
    }

    fun updateRenderParamsToRust() {
        val emu = mEmulator ?: return
        var selActive = false
        var selX1 = 0; var selY1 = 0; var selX2 = 0; var selY2 = 0
        if (isSelectingText()) {
            mTextSelectionCursorController?.getSelectors(mSelCoords)
            selY1 = mSelCoords[0]; selY2 = mSelCoords[1]; selX1 = mSelCoords[2]; selX2 = mSelCoords[3]
            selActive = true
        }
        nativeUpdateRenderParams(mScaleFactor, mTopRow * getFontLineSpacing(), mTopRow,
            selX1, selY1, selX2, selY2, selActive)
    }

    override fun onDraw(canvas: Canvas) {
        if (!mOnDrawCalledAtLeastOnce) {
            mOnDrawCalledAtLeastOnce = true
            Log.i("TerminalView-onDraw", ">>> FIRST onDraw call - emulator=${mEmulator != null}, font metrics ok=${mNativeFontWidth > 0}")
        }
        updateRenderParamsToRust()
        val bitmap = mSixelBitmap
        if (bitmap != null && !bitmap.isRecycled) {
            canvas.save()
            canvas.scale(mScaleFactor, mScaleFactor)
            val pixelX = mSixelStartX * getFontWidth()
            val pixelY = (mSixelStartY - mTopRow) * getFontLineSpacing() + getFontLineSpacingAndAscent()
            canvas.drawBitmap(bitmap, pixelX, pixelY, mSixelPaint)
            canvas.restore()
        }
        renderTextSelection()
    }

    fun getCurrentSession(): TerminalSession? = mTermSession

    private val text: CharSequence
        get() = mEmulator?.getSelectedText(0, mTopRow, mEmulator!!.getCols(), mTopRow + mEmulator!!.getRows()) ?: ""

    fun getCursorX(x: Float): Int = (x / (getFontWidth() * mScaleFactor)).toInt()
    fun getCursorY(y: Float): Int = ((y / mScaleFactor - getFontLineSpacingAndAscent()) / getFontLineSpacing()).toInt() + mTopRow

    fun getPointX(cx: Int): Int {
        var c = cx
        if (mEmulator != null && c > mEmulator!!.getCols()) c = mEmulator!!.getCols()
        return Math.round(c * getFontWidth() * mScaleFactor)
    }

    fun getPointY(cy: Int): Int = Math.round(((cy - mTopRow) * getFontLineSpacing() + getFontLineSpacingAndAscent()) * mScaleFactor)

    override fun surfaceCreated(holder: SurfaceHolder) {
        Log.i("TerminalView-Surface", ">>> surfaceCreated")
        try {
            nativeSetSurface(holder.surface)
            refreshFontMetrics()
        } catch (e: Exception) {
            Log.e("TerminalView-Surface", "!!! surfaceCreated: nativeSetSurface() threw exception: ${e.message}", e)
        }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        Log.i("TerminalView-Surface", ">>> surfaceChanged: ${width}x${height}")
        try { nativeOnSizeChanged(width, height) }
        catch (e: Exception) { Log.e("TerminalView-Surface", "!!! surfaceChanged: ${e.message}", e) }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        Log.i("TerminalView-Surface", ">>> surfaceDestroyed")
        try { nativeSetSurface(null) }
        catch (e: Exception) { Log.e("TerminalView-Surface", "!!! surfaceDestroyed: ${e.message}", e) }
    }

    // --- AutoFill API ---
    @RequiresApi(Build.VERSION_CODES.O)
    override fun autofill(value: AutofillValue) {
        if (value.isText) mTermSession?.write(value.textValue.toString())
        resetAutoFill()
    }

    @RequiresApi(Build.VERSION_CODES.O)
    override fun getAutofillType(): Int = mAutoFillType

    @RequiresApi(Build.VERSION_CODES.O)
    override fun getAutofillHints(): Array<String> = mAutoFillHints

    @RequiresApi(Build.VERSION_CODES.O)
    override fun getAutofillValue(): AutofillValue = AutofillValue.forText("")

    @RequiresApi(Build.VERSION_CODES.O)
    override fun getImportantForAutofill(): Int = mAutoFillImportance

    @RequiresApi(Build.VERSION_CODES.O)
    private fun resetAutoFill() {
        mAutoFillType = AUTOFILL_TYPE_NONE
        mAutoFillImportance = IMPORTANT_FOR_AUTOFILL_NO
        mAutoFillHints = emptyArray()
    }

    @RequiresApi(Build.VERSION_CODES.O)
    fun getAutoFillManagerService(): AutofillManager? = runCatching {
        context.getSystemService(AutofillManager::class.java)
    }.onFailure { mClient?.logStackTraceWithMessage(LOG_TAG, "Failed to get AutofillManager service", it as? Exception) }.getOrNull()

    @RequiresApi(Build.VERSION_CODES.O)
    fun isAutoFillEnabled(): Boolean = runCatching {
        val m = getAutoFillManagerService()
        m != null && m.isEnabled
    }.onFailure { mClient?.logStackTraceWithMessage(LOG_TAG, "Failed to check Autofill", it as? Exception) }.getOrNull() ?: false

    @RequiresApi(Build.VERSION_CODES.O)
    fun requestAutoFill(autoFillHints: Array<String>?) {
        if (autoFillHints == null || autoFillHints.isEmpty()) return
        runCatching {
            val m = getAutoFillManagerService()
            if (m != null && m.isEnabled) {
                mAutoFillType = AUTOFILL_TYPE_TEXT
                mAutoFillImportance = IMPORTANT_FOR_AUTOFILL_YES
                mAutoFillHints = autoFillHints
                m.requestAutofill(this)
            }
        }.onFailure { mClient?.logStackTraceWithMessage(LOG_TAG, "Failed to request Autofill", it as? Exception) }
    }

    fun requestAutoFillUsername() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) requestAutoFill(arrayOf(AUTOFILL_HINT_USERNAME))
    }

    fun requestAutoFillPassword() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) requestAutoFill(arrayOf(AUTOFILL_HINT_PASSWORD))
    }

    @RequiresApi(Build.VERSION_CODES.O)
    fun cancelRequestAutoFill() {
        if (mAutoFillType == AUTOFILL_TYPE_NONE) return
        runCatching {
            val m = getAutoFillManagerService()
            if (m != null && m.isEnabled) {
                resetAutoFill()
                m.cancel()
            }
        }.onFailure { mClient?.logStackTraceWithMessage(LOG_TAG, "Failed to cancel Autofill", it as? Exception) }
    }

    // --- Cursor Blinker ---
    fun setTerminalCursorBlinkerRate(blinkRate: Int): Boolean {
        val result = if (blinkRate != 0 && (blinkRate < TERMINAL_CURSOR_BLINK_RATE_MIN || blinkRate > TERMINAL_CURSOR_BLINK_RATE_MAX)) {
            mClient?.logError(LOG_TAG, "Cursor blink rate must be $TERMINAL_CURSOR_BLINK_RATE_MIN-$TERMINAL_CURSOR_BLINK_RATE_MAX: $blinkRate")
            mTerminalCursorBlinkerRate = 0
            false
        } else {
            mClient?.logVerbose(LOG_TAG, "Setting cursor blinker rate to $blinkRate")
            mTerminalCursorBlinkerRate = blinkRate
            true
        }
        if (mTerminalCursorBlinkerRate == 0) {
            mClient?.logVerbose(LOG_TAG, "Cursor blinker disabled")
            stopTerminalCursorBlinker()
        }
        return result
    }

    fun setTerminalCursorBlinkerState(start: Boolean, startOnlyIfCursorEnabled: Boolean) {
        stopTerminalCursorBlinker()
        val emu = mEmulator ?: return
        emu.setCursorBlinkingEnabled(false)
        if (start) {
            if (mTerminalCursorBlinkerRate < TERMINAL_CURSOR_BLINK_RATE_MIN || mTerminalCursorBlinkerRate > TERMINAL_CURSOR_BLINK_RATE_MAX) return
            if (startOnlyIfCursorEnabled && !emu.isCursorEnabled()) {
                if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logVerbose(LOG_TAG, "Ignoring start - cursor not enabled")
                return
            }
            if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logVerbose(LOG_TAG, "Starting cursor blinker with rate $mTerminalCursorBlinkerRate")
            if (mTerminalCursorBlinkerHandler == null) mTerminalCursorBlinkerHandler = Handler(Looper.getMainLooper())
            mTerminalCursorBlinkerRunnable = TerminalCursorBlinkerRunnable(emu, mTerminalCursorBlinkerRate)
            emu.setCursorBlinkingEnabled(true)
            mTerminalCursorBlinkerRunnable!!.run()
        }
    }

    private fun stopTerminalCursorBlinker() {
        val handler = mTerminalCursorBlinkerHandler
        val runnable = mTerminalCursorBlinkerRunnable
        if (handler != null && runnable != null) {
            if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logVerbose(LOG_TAG, "Stopping cursor blinker")
            handler.removeCallbacks(runnable)
        }
    }

    private inner class TerminalCursorBlinkerRunnable(
        private var emulator: TerminalEmulator?,
        private val blinkRate: Int
    ) : Runnable {
        private var cursorVisible = false
        fun setEmulator(emu: TerminalEmulator?) { emulator = emu }
        override fun run() {
            try {
                val emu = emulator ?: return
                cursorVisible = !cursorVisible
                emu.setCursorBlinkState(cursorVisible)
                val cursorX = emu.getCursorCol()
                val cursorY = emu.getCursorRow()
                if (cursorY >= mTopRow && cursorY < mTopRow + emu.getRows()) {
                    val left = cursorX * getFontWidth()
                    val top = (cursorY - mTopRow) * getFontLineSpacing()
                    val right = left + getFontWidth() * 2
                    val bottom = top + getFontLineSpacing()
                    invalidate(left.toInt(), top.toInt(), right.toInt(), bottom.toInt())
                } else {
                    invalidate()
                }
            } finally {
                mTerminalCursorBlinkerHandler?.postDelayed(this, blinkRate.toLong())
            }
        }
    }

    // --- Text Selection ---
    private fun getTextSelectionCursorController(): TextSelectionCursorController {
        if (mTextSelectionCursorController == null) {
            mTextSelectionCursorController = TextSelectionCursorController(this)
            viewTreeObserver?.addOnTouchModeChangeListener(mTextSelectionCursorController)
        }
        return mTextSelectionCursorController!!
    }

    private fun showTextSelectionCursors(event: MotionEvent) { getTextSelectionCursorController().show(event) }
    private fun hideTextSelectionCursors(): Boolean = getTextSelectionCursorController().hide()
    private fun renderTextSelection() {
        if (mEmulator != null) mTextSelectionCursorController?.render()
    }

    fun isSelectingText(): Boolean = mTextSelectionCursorController?.isActive() == true

    fun getSelectedText(): String? = if (isSelectingText()) mTextSelectionCursorController?.selectedText else null
    fun getStoredSelectedText(): String? = mTextSelectionCursorController?.getStoredSelectedText()
    fun unsetStoredSelectedText() { mTextSelectionCursorController?.unsetStoredSelectedText() }

    fun startTextSelectionMode(event: MotionEvent) {
        if (!requestFocus()) return
        showTextSelectionCursors(event)
        mClient?.copyModeChanged(isSelectingText())
        invalidate()
    }

    fun stopTextSelectionMode() {
        if (hideTextSelectionCursors()) {
            mClient?.copyModeChanged(isSelectingText())
            invalidate()
        }
    }

    private fun decrementYTextSelectionCursors(decrement: Int) {
        mTextSelectionCursorController?.decrementYTextSelectionCursors(decrement)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (mTextSelectionCursorController != null) {
            viewTreeObserver?.addOnTouchModeChangeListener(mTextSelectionCursorController)
        }
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        if (mTextSelectionCursorController != null) {
            stopTextSelectionMode()
            viewTreeObserver?.removeOnTouchModeChangeListener(mTextSelectionCursorController)
            mTextSelectionCursorController?.onDetached()
        }
    }

    // --- Floating Toolbar ---
    private val mShowFloatingToolbar = Runnable {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            getTextSelectionActionMode()?.hide(0)
        }
    }

    @RequiresApi(Build.VERSION_CODES.M)
    private fun showFloatingToolbar() {
        getTextSelectionActionMode()?.let { postDelayed(mShowFloatingToolbar, ViewConfiguration.getDoubleTapTimeout().toLong()) }
    }

    @RequiresApi(Build.VERSION_CODES.M)
    private fun hideFloatingToolbar() {
        getTextSelectionActionMode()?.let { removeCallbacks(mShowFloatingToolbar); it.hide(-1) }
    }

    fun updateFloatingToolbarVisibility(event: MotionEvent) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            when (event.actionMasked) {
                MotionEvent.ACTION_MOVE -> hideFloatingToolbar()
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> showFloatingToolbar()
            }
        }
    }

    private fun getTextSelectionActionMode(): ActionMode? = mTextSelectionCursorController?.actionMode

    // --- Sixel Image ---
    fun onSixelImage(rgbaData: ByteArray, width: Int, height: Int, startX: Int, startY: Int) {
        mSixelImageData = rgbaData
        mSixelWidth = width; mSixelHeight = height
        mSixelStartX = startX; mSixelStartY = startY
        createSixelBitmap()
        invalidate()
        mClient?.logDebug("SixelImage", "Sixel image: ${width}x${height} at ($startX,$startY)")
    }

    private fun createSixelBitmap() {
        val data = mSixelImageData ?: run { mSixelBitmap = null; return }
        val pixelCount = data.size / 4
        if (pixelCount != mSixelWidth * mSixelHeight) {
            mClient?.logError("SixelImage", "Invalid RGBA data size")
            return
        }
        val pixels = IntArray(pixelCount)
        for (i in 0 until pixelCount) {
            val r = data[i * 4].toInt() and 0xFF
            val g = data[i * 4 + 1].toInt() and 0xFF
            val b = data[i * 4 + 2].toInt() and 0xFF
            val a = data[i * 4 + 3].toInt() and 0xFF
            pixels[i] = (a shl 24) or (r shl 16) or (g shl 8) or b
        }
        mSixelBitmap = Bitmap.createBitmap(pixels, mSixelWidth, mSixelHeight, Bitmap.Config.ARGB_8888)
    }

    fun clearSixelImage() {
        mSixelBitmap?.takeIf { !it.isRecycled }?.recycle()
        mSixelBitmap = null
        mSixelImageData = null
        invalidate()
    }

    fun onClearScreen() { clearSixelImage() }

    fun onClearScreenRegion(top: Int, bottom: Int) {
        if (mSixelBitmap != null && !mSixelBitmap!!.isRecycled) {
            if (mSixelStartY in top..bottom) {
                clearSixelImage()
                mClient?.logDebug("SixelImage", "Sixel image cleared (region $top-$bottom contains row $mSixelStartY)")
            }
        }
    }
}
