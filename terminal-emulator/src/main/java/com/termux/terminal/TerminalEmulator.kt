package com.termux.terminal

/**
 * Terminal Emulator — Rust 实现的 Java/Kotlin 包装类
 *
 * 所有实际逻辑都在 Rust 中实现，此类仅通过 [RustTerminal] 转发调用。
 * 所有公开 API 与原始 Java 版本完全兼容。
 *
 * @see RustTerminal 集中管理的 JNI 调用封装
 */
class TerminalEmulator(
    session: TerminalOutput?,
    columns: Int,
    rows: Int,
    cellWidthPixels: Int,
    cellHeightPixels: Int,
    transcriptRows: Int?,
    ptyFd: Int,
    client: TerminalSessionClient
) {

    @JvmOverloads
    constructor(
        session: TerminalSession?,
        enginePtr: Long,
        ptyFd: Int,
        callback: RustEngineCallback
    ) : this(null, 0, 0, 0, 0, null, 0, callback) {
        mEnginePtr = enginePtr
    }

    companion object {
        const val TERMINAL_CURSOR_STYLE_BLOCK = 0
        const val TERMINAL_CURSOR_STYLE_BAR = 1
        const val TERMINAL_CURSOR_STYLE_UNDERLINE = 2
        const val MOUSE_LEFT_BUTTON = 0
        const val MOUSE_MIDDLE_BUTTON = 1
        const val MOUSE_RIGHT_BUTTON = 2
        const val MOUSE_LEFT_BUTTON_MOVED = 32
        const val MOUSE_WHEELUP_BUTTON = 64
        const val MOUSE_WHEELDOWN_BUTTON = 65
        const val UNICODE_REPLACEMENT_CHAR = 0xFFFD
        const val DEFAULT_TERMINAL_TRANSCRIPT_ROWS = 2000
        const val TERMINAL_TRANSCRIPT_ROWS_MIN = 100
        const val TERMINAL_TRANSCRIPT_ROWS_MAX = 200000
        const val DEFAULT_TERMINAL_CURSOR_STYLE = TERMINAL_CURSOR_STYLE_BLOCK
    }

    @Volatile
    private var mEnginePtr: Long = 0
    private var mActiveCallback: RustEngineCallback

    // --- 状态缓存 ---
    private var mCachedCursorCol = 0
    private var mCachedCursorRow = 0
    private var mCachedCursorStyle = 0
    private var mCachedCursorEnabled = true
    private var mCachedCursorVisible = true
    private var mCachedReverseVideo = false
    private var mCachedAlternateBuffer = false
    private var mCachedCursorKeysMode = false
    private var mCachedKeypadMode = false
    private var mCachedMouseTracking = false
    private var mCachedAutoScrollDisabled = false
    private var mCachedRows = 0
    private var mCachedCols = 0
    private var mCachedActiveTranscriptRows = 0
    private var mCachedScrollCounter = 0

    init {
        mActiveCallback = if (client is RustEngineCallback) {
            client
        } else {
            RustEngineCallback(client)
        }

        if (session is TerminalSession) mActiveCallback.setSession(session)
        // 避免从 Rust 异步回调路径（次构造函数）重复创建引擎
        if (columns != 0 || rows != 0) {
            mEnginePtr = RustTerminal.createEngine(
                columns, rows, cellWidthPixels, cellHeightPixels,
                transcriptRows ?: DEFAULT_TERMINAL_TRANSCRIPT_ROWS,
                mActiveCallback
            )
            if (mEnginePtr != 0L && ptyFd != -1) {
                RustTerminal.startIoThread(mEnginePtr, ptyFd)
            }
        }
    }

    // --- 数据输入 ---
    fun append(batch: ByteArray, length: Int) {
        if (mEnginePtr == 0L) return
        RustTerminal.processBatch(mEnginePtr, batch, length)
    }

    /**
     * 批量追加数据（零拷贝版本）
     * 必须传入 DirectByteBuffer 以获得最佳性能。
     */
    fun append(buffer: java.nio.ByteBuffer, length: Int) {
        if (mEnginePtr == 0L) return
        if (buffer.isDirect) {
            RustTerminal.processBatchDirect(mEnginePtr, buffer, buffer.position(), length)
            buffer.position(buffer.position() + length)
        } else {
            // 回退到普通拷贝路径
            val bytes = ByteArray(length)
            buffer.get(bytes)
            append(bytes, length)
        }
    }

    fun processCodePoint(codePoint: Int) {
        RustTerminal.processCodePoint(mEnginePtr, codePoint)
    }

    // --- 终端控制 ---
    fun resize(columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int) {
        RustTerminal.resize(mEnginePtr, columns, rows, cellWidthPixels, cellHeightPixels)
    }

    fun reset() = resetColors()

    fun destroy() {
        RustTerminal.destroyEngine(mEnginePtr)
        mEnginePtr = 0L
    }

    fun isAlive(): Boolean = mEnginePtr != 0L

    fun getNativePointer(): Long = mEnginePtr

    /**
     * 同步终端状态缓存。
     * 建议在每次 UI 刷新（如 onScreenUpdated）或高频轮询前调用一次。
     */
    fun syncState() {
        if (mEnginePtr == 0L) return
        val state = RustTerminal.getTerminalState(mEnginePtr) ?: return
        if (state.size >= 15) {
            mCachedCursorCol = state[0]
            mCachedCursorRow = state[1]
            mCachedCursorStyle = state[2]
            mCachedCursorEnabled = state[3] != 0
            mCachedCursorVisible = state[4] != 0
            mCachedReverseVideo = state[5] != 0
            mCachedAlternateBuffer = state[6] != 0
            mCachedCursorKeysMode = state[7] != 0
            mCachedKeypadMode = state[8] != 0
            mCachedMouseTracking = state[9] != 0
            mCachedAutoScrollDisabled = state[10] != 0
            mCachedRows = state[11]
            mCachedCols = state[12]
            mCachedActiveTranscriptRows = state[13]
            mCachedScrollCounter = state[14]
        }
    }

    // --- 光标 ---
    fun getCursorCol(): Int = mCachedCursorCol
    fun getCursorRow(): Int = mCachedCursorRow
    fun getCursorStyle(): Int = mCachedCursorStyle
    fun setCursorStyle(cursorStyle: Int) {
        RustTerminal.setCursorStyle(mEnginePtr, cursorStyle)
    }
    fun setCursorBlinkingEnabled(enabled: Boolean) {
        RustTerminal.setCursorBlinkingEnabled(mEnginePtr, enabled)
    }
    fun setCursorBlinkRate(rateMs: Int) {
        RustTerminal.setCursorBlinkRate(mEnginePtr, rateMs)
    }
    fun isCursorEnabled(): Boolean = mCachedCursorEnabled
    fun shouldCursorBeVisible(): Boolean = mCachedCursorVisible

    // --- 模式查询 ---
    fun isReverseVideo(): Boolean = mCachedReverseVideo
    fun isAlternateBufferActive(): Boolean = mCachedAlternateBuffer
    fun isCursorKeysApplicationMode(): Boolean = mCachedCursorKeysMode
    fun isKeypadApplicationMode(): Boolean = mCachedKeypadMode
    fun isMouseTrackingActive(): Boolean = mCachedMouseTracking
    fun isAutoScrollDisabled(): Boolean = mCachedAutoScrollDisabled
    fun doDecSetOrReset(setting: Boolean, mode: Int) {
        RustTerminal.doDecSetOrReset(mEnginePtr, setting, mode)
    }
    fun toggleAutoScrollDisabled() {
        RustTerminal.toggleAutoScrollDisabled(mEnginePtr)
    }

    // --- 尺寸 ---
    fun getRows(): Int = mCachedRows
    fun getCols(): Int = mCachedCols
    fun getActiveTranscriptRows(): Int = mCachedActiveTranscriptRows
    fun getTotalRows(): Int = mCachedActiveTranscriptRows + mCachedRows
    @Deprecated("Use getTotalRows() instead")
    fun getActiveRows(): Int = getTotalRows()

    // --- 滚动 ---
    fun getScrollCounter(): Int = mCachedScrollCounter
    fun clearScrollCounter() {
        RustTerminal.clearScrollCounter(mEnginePtr)
    }

    // --- 屏幕数据读取 ---
    fun readRow(row: Int, text: IntArray, styles: LongArray) {
        RustTerminal.readRow(mEnginePtr, row, text, styles)
    }
    fun getSelectedText(x1: Int, y1: Int, x2: Int, y2: Int): String =
        RustTerminal.getSelectedText(mEnginePtr, x1, y1, x2, y2)
    fun getWordAtLocation(x: Int, y: Int): String =
        RustTerminal.getWordAtLocation(mEnginePtr, x, y)
    fun getTranscriptText(): String = RustTerminal.getTranscriptText(mEnginePtr)
    fun getTitle(): String? = RustTerminal.getTitle(mEnginePtr)

    // --- 颜色 ---
    fun getCurrentColors(): IntArray = RustTerminal.getColors(mEnginePtr)
    fun resetColors() {
        RustTerminal.resetColors(mEnginePtr)
    }
    fun updateColorsFromProperties(props: java.util.Properties?) {
        if (props != null) RustTerminal.updateColors(mEnginePtr, props)
    }
    fun setCursorColorForBackground() {
        RustTerminal.setCursorColorForBackground(mEnginePtr)
    }

    // --- 输入事件 ---
    fun sendMouseEvent(button: Int, col: Int, row: Int, pressed: Boolean) {
        RustTerminal.sendMouseEvent(mEnginePtr, button, col, row, pressed)
    }
    fun sendKeyEvent(keyCode: Int, metaState: Int): String? =
        RustTerminal.sendKeyCode(mEnginePtr, keyCode, null, metaState)
    fun sendCharEvent(c: Char, metaState: Int) {
        RustTerminal.sendKeyCode(mEnginePtr, 0, c.toString(), metaState)
    }
    fun paste(text: String) {
        RustTerminal.pasteText(mEnginePtr, text)
    }

    // --- 客户端更新 ---
    fun updateTerminalSessionClient(client: TerminalSessionClient?) {
        mActiveCallback.updateClient(client)
    }

    // --- 调试 ---
    override fun toString(): String =
        if (mEnginePtr == 0L) "TerminalEmulator[destroyed]" else RustTerminal.getDebugInfo(mEnginePtr)
}
