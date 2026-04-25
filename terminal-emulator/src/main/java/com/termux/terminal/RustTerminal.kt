package com.termux.terminal

/**
 * Rust Terminal JNI 桥接类
 *
 * 所有与 Rust 引擎的交互都通过这个类进行。
 * 每个方法都调用 JNI  native 方法并传递引擎指针。
 */
object RustTerminal {

    // --- 创建和销毁 ---

    @JvmStatic
    external fun createEngine(
        columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int,
        transcriptRows: Int, callback: RustEngineCallback
    ): Long

    @JvmStatic
    external fun destroyEngine(enginePtr: Long)

    @JvmStatic
    external fun startIoThread(enginePtr: Long, ptyFd: Int)

    // --- 数据处理 ---

    @JvmStatic
    external fun processBatch(enginePtr: Long, batch: ByteArray, length: Int)

    @JvmStatic
    external fun processCodePoint(enginePtr: Long, codePoint: Int)

    // --- 尺寸调整 ---

    @JvmStatic
    external fun resize(enginePtr: Long, columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int)

    // --- 光标 ---

    @JvmStatic
    external fun getCursorCol(enginePtr: Long): Int

    @JvmStatic
    external fun getCursorRow(enginePtr: Long): Int

    @JvmStatic
    external fun getCursorStyle(enginePtr: Long): Int

    @JvmStatic
    external fun setCursorStyle(enginePtr: Long, cursorStyle: Int)

    @JvmStatic
    external fun setCursorBlinkState(enginePtr: Long, state: Boolean)

    @JvmStatic
    external fun setCursorBlinkingEnabled(enginePtr: Long, enabled: Boolean)

    @JvmStatic
    external fun isCursorEnabled(enginePtr: Long): Boolean

    @JvmStatic
    external fun shouldCursorBeVisible(enginePtr: Long): Boolean

    // --- 模式查询 ---

    @JvmStatic
    external fun isReverseVideo(enginePtr: Long): Boolean

    @JvmStatic
    external fun isAlternateBufferActive(enginePtr: Long): Boolean

    @JvmStatic
    external fun isCursorKeysApplicationMode(enginePtr: Long): Boolean

    @JvmStatic
    external fun isKeypadApplicationMode(enginePtr: Long): Boolean

    @JvmStatic
    external fun isMouseTrackingActive(enginePtr: Long): Boolean

    @JvmStatic
    external fun isAutoScrollDisabled(enginePtr: Long): Boolean

    @JvmStatic
    external fun doDecSetOrReset(enginePtr: Long, setting: Boolean, mode: Int)

    @JvmStatic
    external fun toggleAutoScrollDisabled(enginePtr: Long)

    // --- 屏幕数据读取 ---

    @JvmStatic
    external fun getActiveTranscriptRows(enginePtr: Long): Int

    @JvmStatic
    external fun getRows(enginePtr: Long): Int

    @JvmStatic
    external fun getCols(enginePtr: Long): Int

    @JvmStatic
    external fun readRow(enginePtr: Long, row: Int, text: IntArray, styles: LongArray)

    @JvmStatic
    external fun getSelectedText(enginePtr: Long, x1: Int, y1: Int, x2: Int, y2: Int): String

    @JvmStatic
    external fun getWordAtLocation(enginePtr: Long, x: Int, y: Int): String

    @JvmStatic
    external fun getTranscriptText(enginePtr: Long): String

    @JvmStatic
    external fun getTitle(enginePtr: Long): String?

    // --- 颜色 ---

    @JvmStatic
    external fun getColors(enginePtr: Long): IntArray

    @JvmStatic
    external fun resetColors(enginePtr: Long)

    @JvmStatic
    external fun updateColors(enginePtr: Long, props: java.util.Properties)

    @JvmStatic
    external fun setCursorColorForBackground(enginePtr: Long)

    // --- 输入事件 ---

    @JvmStatic
    external fun sendMouseEvent(enginePtr: Long, button: Int, col: Int, row: Int, pressed: Boolean)

    @JvmStatic
    external fun sendKeyCode(enginePtr: Long, keyCode: Int, text: String?, metaState: Int): String?

    @JvmStatic
    external fun pasteText(enginePtr: Long, text: String)

    // --- 客户端更新 ---

    @JvmStatic
    external fun updateTerminalSessionClient(enginePtr: Long, client: TerminalSessionClient?)

    // --- 滚动 ---

    @JvmStatic
    external fun getScrollCounter(enginePtr: Long): Int

    @JvmStatic
    external fun clearScrollCounter(enginePtr: Long)

    // --- 本地 Socket 服务器 (Rust 实现) ---

    /** 启动 Rust 侧的本地 Socket 服务器 */
    @JvmStatic
    external fun startLocalSocketServer(socketPath: String)

    /** 
     * 供 Rust 引擎回调：执行 Activity Manager 命令 
     * 这是一个内部桥接方法，由 Rust 侧的 local_socket 模块调用
     */
    @JvmStatic
    @Keep
    fun runAmInternal(args: Array<String>): com.termux.shared.jni.models.JniResult {
        val context = TermuxApplication.getTermuxApplicationContext()
        val stdout = StringBuilder()
        val stderr = StringBuilder()
        
        val error = com.termux.shared.shell.am.AmSocketServer.runAmCommand(
            context, args, stdout, stderr, true
        )
        
        val result = com.termux.shared.jni.models.JniResult(
            if (error == null) 0 else 1,
            0,
            error?.minimalErrorString ?: ""
        )
        
        result.stdout = stdout.toString()
        result.stderr = stderr.toString()
        
        return result
    }

    // --- 调试 ---

    @JvmStatic
    external fun getDebugInfo(enginePtr: Long): String
}
