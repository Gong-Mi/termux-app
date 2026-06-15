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
import android.graphics.PixelFormat
import android.graphics.SurfaceTexture
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
import android.widget.OverScroller
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
    external fun nativeSetFontPath(path: String)
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
    private var mTerminalCursorBlinkerRate = 0
    private var mCursorInvisibleIgnoreOnce = false

    // 渲染参数批量同步（16ms 降频，避免滚动时 60fps × JNI 往返）
    private var mRenderParamsPending = false
    private val mRenderParamsHandler = Handler(Looper.getMainLooper())
    private val mRenderParamsRunnable = Runnable {
        mRenderParamsPending = false
        val emu = mEmulator ?: return@Runnable
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

    private var mScaleFactor = 1f
    private lateinit var mGestureRecognizer: GestureAndScaleRecognizer
    private lateinit var mScroller: OverScroller
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

    /// Surface 重建 workaround 状态标志
    private var mSurfaceRecreatePending = false
    /// 独立 Handler，避免 removeView 触发 onDetachedFromWindow 时清除 postDelayed 任务
    private val mSurfaceRecreateHandler = Handler(Looper.getMainLooper())
    /// 记录上次 surfaceChanged 尺寸，避免频繁重复调用 nativeOnSizeChanged
    private var mLastSurfaceWidth = 0
    private var mLastSurfaceHeight = 0

    /// 自定义字体文件路径（同步到 Rust 侧）
    private var mFontFilePath: String? = null

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

        // 启用 10-bit 广色域支持（仅当设备硬件支持且 Activity 已开启广色域模式时）。
        // 这要求 Activity 已经调用了 getWindow().setColorMode(ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT)。
        // 显式请求 RGBA_1010102 像素格式，以便 Android HWC 能分配高位深 Buffer。
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && context.resources.configuration.isScreenWideColorGamut) {
            holder.setFormat(PixelFormat.RGBA_1010102)
        } else {
            holder.setFormat(PixelFormat.RGBA_8888)
        }

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
                    
                    val algorithm = mClient?.getTouchAlgorithm() ?: "adaptive"
                    val distY = when (algorithm) {
                        "adaptive" -> {
                            // 核心逻辑：实现类似亮度调节的非线性对数阻尼 (Logarithmic Damping)
                            val absD = Math.abs(distanceY)
                            val sign = Math.signum(distanceY)
                            val k = 20.0f // 阻尼拐点
                            sign * (Math.log(1.0 + absD / k) * k).toFloat()
                        }
                        "natural" -> {
                            // 自然模式：使用平方根压缩 (Square Root Scaling)
                            // 提供一种平滑但比对数更轻盈的反馈
                            val absD = Math.abs(distanceY)
                            val sign = Math.signum(distanceY)
                            sign * (Math.sqrt(absD.toDouble() * 15.0)).toFloat()
                        }
                        "momentum" -> {
                            // 加速模式：使用幂律加速 (Power-law Acceleration)
                            // 越快越有力，适合需要快速翻阅数万行日志的用户
                            val absD = Math.abs(distanceY)
                            val sign = Math.signum(distanceY)
                            sign * (absD * (1.0f + absD / 100.0f))
                        }
                        else -> distanceY // 标准线性模式
                    }
                    
                    val totalDistY = distY + mScrollRemainder
                    val deltaRows = (totalDistY / getFontLineSpacing()).toInt()
                    mScrollRemainder = totalDistY - deltaRows * getFontLineSpacing()
                    doScroll(e, deltaRows)
                    updateRenderParamsToRust()
                }
                return true
            }

            override fun onScale(focusX: Float, focusY: Float, scale: Float): Boolean {
                if (mEmulator == null || isSelectingText()) return true
                // No visual scale layer: pass the raw gesture scale directly to the client,
                // which commits font size + reflow on each MOVE event.
                mScaleFactor = mClient?.onScale(scale) ?: mScaleFactor
                updateRenderParamsToRust()
                invalidate()
                return true
            }

            override fun onScaleEnd(focusX: Float, focusY: Float): Boolean {
                if (mEmulator == null || isSelectingText()) return true
                mScaleFactor = mClient?.onScaleEnd(mScaleFactor) ?: mScaleFactor
                updateRenderParamsToRust()
                invalidate()
                return true
            }

            override fun onFling(e2: MotionEvent, velocityX: Float, velocityY: Float): Boolean {
                val emu = mEmulator ?: return true
                if (!mScroller.isFinished) mScroller.forceFinished(true)

                val mouseTracking = emu.isMouseTrackingActive()
                val lineSpacing = getFontLineSpacing()

                val algorithm = mClient?.getTouchAlgorithm() ?: "adaptive"
                updateScrollerFriction(algorithm)
                
                val scaledVelocity = when (algorithm) {
                    "adaptive" -> {
                        // 对数压缩：适合日常使用，防止失控
                        val absV = Math.abs(velocityY)
                        val signV = Math.signum(velocityY)
                        val k = 1500.0f
                        signV * (Math.log(1.0 + absV / k) * k).toFloat() * 1.5f
                    }
                    "natural" -> {
                        // 平方根映射：手感更轻盈
                        val absV = Math.abs(velocityY)
                        val signV = Math.signum(velocityY)
                        signV * (Math.sqrt(absV.toDouble() * 2000.0)).toFloat()
                    }
                    "momentum" -> {
                        // 幂律加速：让高速滑动更远
                        velocityY * (1.0f + Math.abs(velocityY) / 8000.0f)
                    }
                    else -> velocityY * 0.25f // 标准模式
                }

                if (mouseTracking) {
                    mScroller.fling(0, 0, 0, -scaledVelocity.toInt(), 0, 0, -1000, 1000)
                } else {
                    val startYPixels = mTopRow * lineSpacing
                    val minScrollPixels = -emu.getActiveTranscriptRows() * lineSpacing

                    mScroller.fling(
                        0, startYPixels.toInt(), 
                        0, if (algorithm == "standard") scaledVelocity.toInt() else -scaledVelocity.toInt(), 
                        0, 0, 
                        minScrollPixels.toInt(), 0
                    )
                }

                post(object : Runnable {
                    var mLastYPixels = if (mouseTracking) 0f else mTopRow * lineSpacing
                    var mRemainder = 0f

                    override fun run() {
                        if (mouseTracking != mEmulator?.isMouseTrackingActive()) {
                            mScroller.abortAnimation()
                            return
                        }
                        if (mScroller.isFinished) return

                        val more = mScroller.computeScrollOffset()
                        val currYPixels = mScroller.currY.toFloat()
                        val diffPixels = (currYPixels - mLastYPixels) + mRemainder

                        // 转换为行数
                        val deltaRows = (diffPixels / lineSpacing).toInt()
                        if (deltaRows != 0) {
                            doScroll(e2, deltaRows)
                            mRemainder = diffPixels - deltaRows * lineSpacing
                        } else {
                            mRemainder = diffPixels
                        }

                        mLastYPixels = currYPixels
                        updateRenderParamsToRust()
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

        mScroller = OverScroller(context)
        val am = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        mAccessibilityEnabled = am.isEnabled
    }

    fun updateScrollerFriction(algorithm: String) {
        when (algorithm) {
            "adaptive" -> mScroller.setFriction(ViewConfiguration.getScrollFriction() * 1.5f)
            "natural" -> mScroller.setFriction(ViewConfiguration.getScrollFriction() * 1.2f)
            "momentum" -> mScroller.setFriction(ViewConfiguration.getScrollFriction() * 0.8f) // 降低摩擦力以实现长距离冲刺
            else -> mScroller.setFriction(ViewConfiguration.getScrollFriction())
        }
    }

    fun setTerminalViewClient(client: TerminalViewClient) { mClient = client }
    fun setIsTerminalViewKeyLoggingEnabled(value: Boolean) { TERMINAL_VIEW_KEY_LOGGING_ENABLED = value }

    fun attachSession(session: TerminalSession?): Boolean {
        if (session === mTermSession) return false
        mTopRow = 0
        mTermSession = session
        mEmulator = session?.mEmulator
        mEnginePointerSet = false
        mCombiningAccent = 0
        updateSize()

        // 关键修复：主动同步指针并拉取全量状态，不再等待第一次回调，避免黑屏
        mEmulator?.let { emu ->
            emu.syncState()
            val ptr = emu.getNativePointer()
            if (ptr != 0L) {
                Log.i("TerminalView-Engine", "attachSession: Calling nativeSetEnginePointer with ptr=$ptr")
                nativeSetEnginePointer(ptr)
                mEnginePointerSet = true
            }
        }

        isVerticalScrollBarEnabled = true
        return true
    }

    private fun getFontWidth(): Float = if (mNativeFontWidth > 0) mNativeFontWidth else 1.0f
    private fun getFontLineSpacing(): Float = if (mNativeFontHeight > 0) mNativeFontHeight else 1.0f
    private fun getFontLineSpacingAndAscent(): Float = 0f

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
        if (mEmulator == null) updateSize()
        val emu = mEmulator
        if (emu == null) {
            Log.w("TerminalView-Engine", "onScreenUpdated called but mEmulator is still null after updateSize()")
            return
        }
        
        // syncState() 已由 Rust 增量推送替代（TerminalEvent::StateChanged），
        // Rust 在状态变化时主动回调 onStateChanged(mask, values) 更新缓存。
        // 此处不再轮询，避免高频 JNI 往返。
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
        
        // 关键修复：即使 onDraw 不被调用（如小窗模式），也要同步渲染参数并请求重绘
        updateRenderParamsToRust()
        
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

    /**
     * 设置自定义字体文件路径，并同步到 Rust 渲染器
     * @param path 绝对路径，例如 "/data/.../.termux/font.ttf"
     */
    fun setFontFile(path: String?) {
        mFontFilePath = path
        if (path != null && path.isNotEmpty()) {
            nativeSetFontPath(path)
        }
        refreshFontMetrics()
        updateSize()
        invalidate()
    }

    /**
     * 获取当前自定义字体文件路径
     */
    fun getFontFile(): String? = mFontFilePath

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
        if (rowsDown == 0) return
        
        val up = rowsDown < 0
        val absRows = Math.abs(rowsDown)
        
        if (emu.isMouseTrackingActive()) {
            repeat(absRows) {
                sendMouseEventCode(event, if (up) TerminalEmulator.MOUSE_WHEELUP_BUTTON else TerminalEmulator.MOUSE_WHEELDOWN_BUTTON, true)
            }
        } else if (emu.isAlternateBufferActive()) {
            repeat(absRows) {
                handleKeyCode(if (up) KeyEvent.KEYCODE_DPAD_UP else KeyEvent.KEYCODE_DPAD_DOWN, 0)
            }
        } else {
            mTopRow = Math.min(0, Math.max(-emu.getActiveTranscriptRows(), mTopRow + rowsDown))
            updateRenderParamsToRust()
            if (!awakenScrollBars()) invalidate()
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

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        // 核心修复：强制 Surface buffer 尺寸与 View 布局尺寸同步。
        // 在某些系统（如 MIUI/HyperOS）上，IME 弹出或 Activity 切换时，系统可能不会自动
        // 更新 SurfaceView 的 Surface 尺寸，导致 surfaceChanged 不被调用，产生黑屏或
        // 渲染尺寸错位。通过显式调用 setFixedSize，我们强制系统分配与布局一致的 Buffer，
        // 从而触发 surfaceChanged 并确保渲染 1:1 匹配（无拉伸/模糊）。
        if (w > 0 && h > 0) {
            holder.setFixedSize(w, h)
        }
        // Update last surface dimensions so updateSizeInternal() uses the correct
        // values even if surfaceChanged fires later (or not at all on MIUI).
        mLastSurfaceWidth = w
        mLastSurfaceHeight = h
        // Fallback: notify native of potential size change directly.
        // On Android 16 (API 36), surfaceChanged may not fire during rotation,
        // so the Vulkan swapchain would never be resized.
        nativeOnSizeChanged(w, h)
        updateSize()
    }

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

    /** 由 TermuxActivity.onConfigurationChanged 调用，触发 SurfaceView 重新布局 */
    fun notifyConfigurationChanged() {
        requestLayout()
        // 强制 SurfaceView 重建 surface（和 surfaceDestroyed workaround 同样手法）
        // 这样 ANativeWindow 会更新到新方向，Vulkan surface capabilities 返回正确的 transform/extent
        try {
            onConfigurationChanged(resources.configuration)
        } catch (e: Exception) {
            Log.w("TerminalView-Surface", "notifyConfigurationChanged: onConfigurationChanged failed: ${e.message}")
        }
    }

    private fun updateSizeInternal() {
        // 使用 Surface buffer 尺寸（与 swapchain extent 一致）计算 terminal 行列数。
        // View 布局尺寸可能与 Surface buffer 尺寸不同（SurfaceView 的 Surface 层
        // 由系统分配，其像素维度不必然等于 View 的 layout 尺寸），用错会导致
        // terminal columns/rows 与实际渲染区域不匹配。
        val surfaceWidth = if (mLastSurfaceWidth > 0) mLastSurfaceWidth else width
        val surfaceHeight = if (mLastSurfaceHeight > 0) mLastSurfaceHeight else height
        val session = mTermSession
        if (surfaceWidth == 0 || surfaceHeight == 0 || session == null) return
        val newColumns = Math.max(4, (surfaceWidth / getFontWidth()).toInt())
        val newRows = Math.max(4, ((surfaceHeight - getFontLineSpacingAndAscent()) / getFontLineSpacing()).toInt())
        val cellWidth = getFontWidth().toInt()
        val cellHeight = getFontLineSpacing().toInt()

        if (!session.isEngineInitialized()) {
            session.updateSize(newColumns, newRows, cellWidth, cellHeight)
            return
        }
        val emu = mEmulator
        if (emu == null || newColumns != emu.getCols() || newRows != emu.getRows()) {
            session.updateSize(newColumns, newRows, cellWidth, cellHeight)
            mEmulator = session.mEmulator
            mClient?.onEmulatorSet()
            // 光标闪烁状态已由 Rust 渲染线程自主管理，无需 Java 定时器
            mTopRow = 0
            scrollTo(0, 0)
            updateRenderParamsToRust()
            invalidate()
        }
    }

    override fun onScrollChanged(l: Int, t: Int, oldl: Int, oldt: Int) {
        super.onScrollChanged(l, t, oldl, oldt)
        if (mSixelBitmap != null && !mSixelBitmap!!.isRecycled) invalidate()
    }

    fun updateRenderParamsToRust() {
        if (mRenderParamsPending) return
        mRenderParamsPending = true
        mRenderParamsHandler.postDelayed(mRenderParamsRunnable, 16)
    }

    // onDraw 在 TextureView 中是 final 的，无法重写。
    // 绘制逻辑已移到 draw(Canvas) 中，在 super.draw 之后执行。

    fun getCurrentSession(): TerminalSession? = mTermSession

    private val text: CharSequence
        get() = mEmulator?.getSelectedText(0, mTopRow, mEmulator!!.getCols(), mTopRow + mEmulator!!.getRows()) ?: ""

    /** 屏幕像素 → 逻辑列（考虑缩放） */
    fun getCursorX(x: Float): Int = (x / (getFontWidth() * mScaleFactor)).toInt()

    /**
     * 屏幕像素 → 逻辑行（考虑缩放）。
     * Note: `y - 40f` 中的 40px 是手指触摸的视觉偏移补偿。
     * 当用户用手指拖拽选择手柄时，手指会遮挡触摸点；向上偏移 40px
     * 可使选中的文本出现在手指上方，避免被遮挡。
     * 来源: upstream commit 35a4fdac (2019-10-05, "Add selection mode cursor controller")
     */
    fun getCursorY(y: Float): Int = ((y - 40f) / (getFontLineSpacing() * mScaleFactor)).toInt() + mTopRow

    /** 未缩放相对坐标（供 Canvas onDraw / Sixel 使用） */
    fun getPointX(cx: Int): Int {
        var c = cx
        if (mEmulator != null && c > mEmulator!!.getCols()) c = mEmulator!!.getCols()
        return Math.round(c * getFontWidth())
    }

    fun getPointY(cy: Int): Int = Math.round((cy - mTopRow) * getFontLineSpacing())

    /** 缩放后的屏幕像素坐标（供 PopupWindow / ActionMode 绝对定位使用） */
    fun getScaledPointX(cx: Int): Int {
        var c = cx
        if (mEmulator != null && c > mEmulator!!.getCols()) c = mEmulator!!.getCols()
        return Math.round(c * getFontWidth() * mScaleFactor)
    }

    fun getScaledPointY(cy: Int): Int = Math.round((cy - mTopRow) * getFontLineSpacing() * mScaleFactor)

    override fun surfaceCreated(holder: SurfaceHolder) {
        Log.i("TerminalView-Surface", ">>> surfaceCreated")
        // 取消 pending 的 Surface 重建 workaround（如果系统已自动重建）
        mSurfaceRecreatePending = false
        mLastSurfaceWidth = 0
        mLastSurfaceHeight = 0
        try {
            nativeSetSurface(holder.surface)
            refreshFontMetrics()
        } catch (e: Exception) {
            Log.e("TerminalView-Surface", "!!! surfaceCreated: nativeSetSurface() threw exception: ${e.message}", e)
        }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        Log.i("TerminalView-Surface", ">>> surfaceChanged: ${width}x${height}")
        // 性能优化：如果尺寸与上次相同，跳过 nativeOnSizeChanged
        // 避免 MIUI/HyperOS 频繁调用 surfaceChanged 导致 swapchain 反复重建
        if (width == mLastSurfaceWidth && height == mLastSurfaceHeight) {
            Log.d("TerminalView-Surface", "surfaceChanged: size unchanged, skipping nativeOnSizeChanged")
            return
        }
        mLastSurfaceWidth = width
        mLastSurfaceHeight = height
        try {
            nativeOnSizeChanged(width, height)
            updateSize()
        }
        catch (e: Exception) { Log.e("TerminalView-Surface", "!!! surfaceChanged: ${e.message}", e) }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        Log.i("TerminalView-Surface", ">>> surfaceDestroyed")
        try { nativeSetSurface(null) }
        catch (e: Exception) { Log.e("TerminalView-Surface", "!!! surfaceDestroyed: ${e.message}", e) }

        // Workaround: 某些系统（尤其 MIUI/HyperOS）在 IME 弹出/Activity transition 时
        // 销毁 Surface 后不会自动调用 surfaceCreated。通过触发 onConfigurationChanged
        // + 重新 attach 到 parent 强制系统重新评估 SurfaceView。
        mSurfaceRecreatePending = true
        mSurfaceRecreateHandler.postDelayed({
            if (!mSurfaceRecreatePending) return@postDelayed
            if (holder.surface == null || !holder.surface.isValid) {
                Log.w("TerminalView-Surface", "Surface still invalid after 800ms, forcing recreation")
                // 触发 SurfaceView 的 onConfigurationChanged，内部会调用 updateSurface()
                try {
                    onConfigurationChanged(resources.configuration)
                } catch (e: Exception) {
                    Log.w("TerminalView-Surface", "onConfigurationChanged failed: ${e.message}")
                }
                val parentView = parent as? android.view.ViewGroup
                val lp = layoutParams
                if (parentView != null && lp != null) {
                    // Preserve DrawerLayout child order. TerminalView is expected to stay below
                    // left_drawer in activity_termux.xml. Re-attaching with addView(this, lp)
                    // appends it as the last child, causing the SurfaceView to sit above the
                    // drawer and intercept/occlude drawer button touches.
                    val originalIndex = parentView.indexOfChild(this).coerceAtLeast(0)
                    parentView.removeView(this)
                    mSurfaceRecreateHandler.postDelayed({
                        if (!mSurfaceRecreatePending) return@postDelayed
                        val safeIndex = originalIndex.coerceAtMost(parentView.childCount)
                        parentView.addView(this, safeIndex, lp)
                        parentView.requestLayout()
                        Log.i("TerminalView-Surface", "Re-attached to parent at index=$safeIndex to force surface recreation")
                    }, 200)
                } else {
                    // fallback: toggle visibility
                    visibility = GONE
                    mSurfaceRecreateHandler.postDelayed({
                        if (!mSurfaceRecreatePending) return@postDelayed
                        visibility = VISIBLE
                        requestLayout()
                        invalidate()
                        Log.i("TerminalView-Surface", "Visibility toggled GONE->VISIBLE fallback")
                    }, 100)
                }
                // 关键修复：强制触发 ViewRootImpl 的 performTraversals，从而调用 SurfaceView.updateSurface()
                mSurfaceRecreateHandler.postDelayed({
                    if (!mSurfaceRecreatePending) return@postDelayed
                    val root = rootView
                    root.requestLayout()
                    root.invalidate()
                    // 强制窗口重新布局，触发 SurfaceView 的 Surface 重建
                    (context as? Activity)?.let { activity ->
                        try {
                            val attrs = activity.window.attributes
                            activity.window.attributes = attrs
                            Log.i("TerminalView-Surface", "Forced window attributes update to trigger relayout")
                        } catch (e: Exception) {
                            Log.w("TerminalView-Surface", "Window attributes update failed: ${e.message}")
                        }
                    }
                }, 300)
            }
        }, 800)
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
            // Visual scale layer removed — font size is committed during gesture.
            // canvas.scale(mScaleFactor, mScaleFactor)
            val pixelX = mSixelStartX * getFontWidth()
            val pixelY = (mSixelStartY - mTopRow) * getFontLineSpacing() + getFontLineSpacingAndAscent()
            canvas.drawBitmap(bitmap, pixelX, pixelY, mSixelPaint)
            canvas.restore()
        }
        renderTextSelection()
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

    // --- Cursor Blinker（已下沉到 Rust 渲染线程，Java 不再管理定时器）---
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
        mEmulator?.setCursorBlinkRate(blinkRate.coerceIn(0, TERMINAL_CURSOR_BLINK_RATE_MAX))
        return result
    }

    fun setTerminalCursorBlinkerState(start: Boolean, startOnlyIfCursorEnabled: Boolean) {
        val emu = mEmulator ?: return
        if (start) {
            if (mTerminalCursorBlinkerRate < TERMINAL_CURSOR_BLINK_RATE_MIN || mTerminalCursorBlinkerRate > TERMINAL_CURSOR_BLINK_RATE_MAX) return
            if (startOnlyIfCursorEnabled && !emu.isCursorEnabled()) {
                if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logVerbose(LOG_TAG, "Ignoring start - cursor not enabled")
                return
            }
            if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logVerbose(LOG_TAG, "Starting cursor blinker with rate $mTerminalCursorBlinkerRate")
            emu.setCursorBlinkingEnabled(true)
        } else {
            if (TERMINAL_VIEW_KEY_LOGGING_ENABLED) mClient?.logVerbose(LOG_TAG, "Stopping cursor blinker")
            emu.setCursorBlinkingEnabled(false)
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
    fun renderTextSelection() {
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
