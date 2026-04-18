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
    ) : this(null, 0, 0, 0, 0, null, 0, object : TerminalSessionClient {
        override fun onTextChanged(changedSession: TerminalSession) {}
        override fun onTitleChanged(changedSession: TerminalSession) {}
        override fun onSessionFinished(finishedSession: TerminalSession) {}
        override fun onCopyTextToClipboard(session: TerminalSession, text: String) {}
        override fun onPasteTextFromClipboard(session: TerminalSession?) {}
        override fun onBell(session: TerminalSession) {}
        override fun onColorsChanged(session: TerminalSession) {}
        override fun onTerminalCursorStateChange(state: Boolean) {}
        override fun setTerminalShellPid(session: TerminalSession, pid: Int) {}
        override fun getTerminalCursorStyle(): Int? = null
        override fun logError(tag: String, message: String) {}
        override fun logWarn(tag: String, message: String) {}
        override fun logInfo(tag: String, message: String) {}
        override fun logDebug(tag: String, message: String) {}
        override fun logVerbose(tag: String, message: String) {}
        override fun logStackTraceWithMessage(tag: String, message: String, e: Exception?) {}
        override fun logStackTrace(tag: String, e: Exception?) {}
    }) {
        if (session != null) callback.setSession(session)
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
        const val TERMINAL_TRANSCRIPT_ROWS_MAX = 50000
        const val DEFAULT_TERMINAL_CURSOR_STYLE = TERMINAL_CURSOR_STYLE_BLOCK
    }

    @Volatile
    private var mEnginePtr: Long = 0
    private val mRustCallback: RustEngineCallback = RustEngineCallback(client)

    init {
        if (session is TerminalSession) mRustCallback.setSession(session)
        mEnginePtr = RustTerminal.createEngine(
            columns, rows, cellWidthPixels, cellHeightPixels,
            transcriptRows ?: DEFAULT_TERMINAL_TRANSCRIPT_ROWS,
            mRustCallback
        )
        if (mEnginePtr != 0L && ptyFd != -1) {
            RustTerminal.startIoThread(mEnginePtr, ptyFd)
        }
    }

    // --- 数据输入 ---
    fun append(batch: ByteArray, length: Int) {
        RustTerminal.processBatch(mEnginePtr, batch, length)
    }

    fun processCodePoint(codePoint: Int) {
        RustTerminal.processCodePoint(mEnginePtr, codePoint)
    }

    // --- 终端控制 ---
    fun resize(columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int) {
        RustTerminal.resize(mEnginePtr, columns, rows, cellWidthPixels, cellHeightPixels)
    }

    fun setTranscriptRows(rows: Int) {
        RustTerminal.setTranscriptRows(mEnginePtr, rows)
    }

    fun reset() = resetColors()

    fun destroy() {
        RustTerminal.destroyEngine(mEnginePtr)
        mEnginePtr = 0L
    }

    fun isAlive(): Boolean = mEnginePtr != 0L

    fun getNativePointer(): Long = mEnginePtr

    // --- 光标 ---
    fun getCursorCol(): Int = RustTerminal.getCursorCol(mEnginePtr)
    fun getCursorRow(): Int = RustTerminal.getCursorRow(mEnginePtr)
    fun getCursorStyle(): Int = RustTerminal.getCursorStyle(mEnginePtr)
    fun setCursorStyle(cursorStyle: Int) {
        RustTerminal.setCursorStyle(mEnginePtr, cursorStyle)
    }
    fun setCursorBlinkState(state: Boolean) {
        RustTerminal.setCursorBlinkState(mEnginePtr, state)
    }
    fun setCursorBlinkingEnabled(enabled: Boolean) {
        RustTerminal.setCursorBlinkingEnabled(mEnginePtr, enabled)
    }
    fun isCursorEnabled(): Boolean = RustTerminal.isCursorEnabled(mEnginePtr)
    fun shouldCursorBeVisible(): Boolean = RustTerminal.shouldCursorBeVisible(mEnginePtr)

    // --- 模式查询 ---
    fun isReverseVideo(): Boolean = RustTerminal.isReverseVideo(mEnginePtr)
    fun isAlternateBufferActive(): Boolean = RustTerminal.isAlternateBufferActive(mEnginePtr)
    fun isCursorKeysApplicationMode(): Boolean = RustTerminal.isCursorKeysApplicationMode(mEnginePtr)
    fun isKeypadApplicationMode(): Boolean = RustTerminal.isKeypadApplicationMode(mEnginePtr)
    fun isMouseTrackingActive(): Boolean = RustTerminal.isMouseTrackingActive(mEnginePtr)
    fun isAutoScrollDisabled(): Boolean = RustTerminal.isAutoScrollDisabled(mEnginePtr)
    fun doDecSetOrReset(setting: Boolean, mode: Int) {
        RustTerminal.doDecSetOrReset(mEnginePtr, setting, mode)
    }
    fun toggleAutoScrollDisabled() {
        RustTerminal.toggleAutoScrollDisabled(mEnginePtr)
    }

    // --- 尺寸 ---
    fun getRows(): Int = RustTerminal.getRows(mEnginePtr)
    fun getCols(): Int = RustTerminal.getCols(mEnginePtr)
    fun getActiveTranscriptRows(): Int = RustTerminal.getActiveTranscriptRows(mEnginePtr)
    fun getTotalRows(): Int = getActiveTranscriptRows() + getRows()
    @Deprecated("Use getTotalRows() instead")
    fun getActiveRows(): Int = getTotalRows()

    // --- 滚动 ---
    fun getScrollCounter(): Int = RustTerminal.getScrollCounter(mEnginePtr)
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
        RustTerminal.updateTerminalSessionClient(mEnginePtr, client)
    }

    // --- 调试 ---
    override fun toString(): String =
        if (mEnginePtr == 0L) "TerminalEmulator[destroyed]" else RustTerminal.getDebugInfo(mEnginePtr)
}
