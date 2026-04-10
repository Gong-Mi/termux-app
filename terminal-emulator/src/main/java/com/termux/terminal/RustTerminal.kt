package com.termux.terminal

/**
 * Kotlin 包装层 — 集中管理所有 Rust JNI 调用
 *
 * 标记说明:
 *   ✅ 已迁移到 Rust
 *   ⚠️  部分迁移 / 有已知问题
 *   ❌ 未迁移 (仍用 Java 实现或空实现)
 */
object RustTerminal {

    init {
        // 确保 native 库已加载
        if (JNI.sNativeLibrariesLoaded.not()) {
            kotlin.runCatching { System.loadLibrary("termux_rust") }
        }
    }

    // =========================================================================
    // TerminalEmulator JNI 方法 (enginePtr 绑定)
    // =========================================================================

    /** ✅ 创建 Rust 引擎实例 */
    fun createEngine(
        cols: Int, rows: Int, cellWidth: Int, cellHeight: Int,
        totalRows: Int, callback: JNI.RustEngineCallback
    ): Long = TerminalEmulator.createEngineRustWithCallback(
        cols, rows, cellWidth, cellHeight, totalRows, callback
    )

    /** ✅ 销毁引擎 */
    fun destroyEngine(enginePtr: Long) {
        if (enginePtr != 0L) TerminalEmulator.destroyEngineRust(enginePtr)
    }

    /** ✅ 启动 IO 线程 */
    fun startIoThread(enginePtr: Long, fd: Int) {
        if (enginePtr != 0L) TerminalEmulator.nativeStartIoThread(enginePtr, fd)
    }

    /** ✅ 批量处理字节 */
    fun processBatch(enginePtr: Long, batch: ByteArray, length: Int) {
        if (enginePtr != 0L) TerminalEmulator.processBatchRust(enginePtr, batch, length)
    }

    /** ✅ 处理单个码点 */
    fun processCodePoint(enginePtr: Long, codePoint: Int) {
        if (enginePtr != 0L) TerminalEmulator.processCodePointRust(enginePtr, codePoint)
    }

    /** ✅ resize */
    fun resize(enginePtr: Long, cols: Int, rows: Int, cw: Int, ch: Int) {
        if (enginePtr != 0L) TerminalEmulator.resizeEngineRustFull(enginePtr, cols, rows, cw, ch)
    }

    // --- 光标 ---

    /** ✅ 获取光标列 */
    fun getCursorCol(enginePtr: Long): Int =
        if (enginePtr != 0L) TerminalEmulator.getCursorColFromRust(enginePtr) else 0

    /** ✅ 获取光标行 */
    fun getCursorRow(enginePtr: Long): Int =
        if (enginePtr != 0L) TerminalEmulator.getCursorRowFromRust(enginePtr) else 0

    /** ✅ 获取光标样式 */
    fun getCursorStyle(enginePtr: Long): Int =
        if (enginePtr != 0L) TerminalEmulator.getCursorStyleFromRust(enginePtr)
        else TerminalEmulator.TERMINAL_CURSOR_STYLE_BLOCK

    /** ✅ 设置光标样式 */
    fun setCursorStyle(enginePtr: Long, style: Int) {
        if (enginePtr != 0L) TerminalEmulator.setCursorStyleFromRust(enginePtr, style)
    }

    /** ✅ 光标是否启用 */
    fun isCursorEnabled(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.isCursorEnabledFromRust(enginePtr)

    /** ✅ 光标是否应该可见 */
    fun shouldCursorBeVisible(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.shouldCursorBeVisibleFromRust(enginePtr)

    /** ✅ 设置光标闪烁状态 */
    fun setCursorBlinkState(enginePtr: Long, state: Boolean) {
        if (enginePtr != 0L) TerminalEmulator.setCursorBlinkStateInRust(enginePtr, state)
    }

    /** ✅ 设置光标闪烁开关 */
    fun setCursorBlinkingEnabled(enginePtr: Long, enabled: Boolean) {
        if (enginePtr != 0L) TerminalEmulator.setCursorBlinkingEnabledInRust(enginePtr, enabled)
    }

    // --- 屏幕查询 ---

    /** ✅ 行数 */
    fun getRows(enginePtr: Long): Int =
        if (enginePtr != 0L) TerminalEmulator.getRowsFromRust(enginePtr) else 0

    /** ✅ 列数 */
    fun getCols(enginePtr: Long): Int =
        if (enginePtr != 0L) TerminalEmulator.getColsFromRust(enginePtr) else 0

    /** ✅ 活跃历史行数 */
    fun getActiveTranscriptRows(enginePtr: Long): Int =
        if (enginePtr != 0L) TerminalEmulator.getActiveTranscriptRowsFromRust(enginePtr) else 0

    /** ✅ 读取行数据 */
    fun readRow(enginePtr: Long, row: Int, textBuf: IntArray, styleBuf: LongArray) {
        if (enginePtr != 0L) TerminalEmulator.readRowFromRust(enginePtr, row, textBuf, styleBuf)
    }

    /** ✅ 获取选中文本 */
    fun getSelectedText(enginePtr: Long, x1: Int, y1: Int, x2: Int, y2: Int): String =
        if (enginePtr != 0L) TerminalEmulator.getSelectedTextFromRust(enginePtr, x1, y1, x2, y2)
        else ""

    /** ✅ 获取光标处单词 */
    fun getWordAtLocation(enginePtr: Long, x: Int, y: Int): String =
        if (enginePtr != 0L) TerminalEmulator.getWordAtLocationFromRust(enginePtr, x, y) else ""

    /** ✅ 获取全部历史文本 */
    fun getTranscriptText(enginePtr: Long): String =
        if (enginePtr != 0L) TerminalEmulator.getTranscriptTextFromRust(enginePtr) else ""

    /** ✅ 获取标题 */
    fun getTitle(enginePtr: Long): String =
        if (enginePtr != 0L) TerminalEmulator.getTitleFromRust(enginePtr) else ""

    // --- 模式查询 ---

    /** ✅ 反色模式 */
    fun isReverseVideo(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.isReverseVideoFromRust(enginePtr)

    /** ✅ 备用缓冲区 */
    fun isAlternateBufferActive(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.isAlternateBufferActiveFromRust(enginePtr)

    /** ✅ 应用光标键 */
    fun isCursorKeysApplicationMode(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.isCursorKeysApplicationModeFromRust(enginePtr)

    /** ✅ 应用小键盘 */
    fun isKeypadApplicationMode(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.isKeypadApplicationModeFromRust(enginePtr)

    /** ✅ 鼠标追踪 */
    fun isMouseTrackingActive(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.isMouseTrackingActiveFromRust(enginePtr)

    /** ✅ DECSET/DECRST */
    fun doDecSetOrReset(enginePtr: Long, setting: Boolean, mode: Int) {
        if (enginePtr != 0L) TerminalEmulator.doDecSetOrResetFromRust(enginePtr, setting, mode)
    }

    // --- 输入 ---

    /** ✅ 鼠标事件 */
    fun sendMouseEvent(enginePtr: Long, button: Int, col: Int, row: Int, pressed: Boolean) {
        if (enginePtr != 0L) TerminalEmulator.sendMouseEventFromRust(enginePtr, button, col, row, pressed)
    }

    /** ✅ 按键事件 → 返回转义序列 */
    fun sendKeyCode(enginePtr: Long, keyCode: Int, charStr: String?, metaState: Int): String? =
        if (enginePtr != 0L) TerminalEmulator.sendKeyCodeFromRust(enginePtr, keyCode, charStr, metaState)
        else null

    /** ✅ 粘贴文本 */
    fun pasteText(enginePtr: Long, text: String) {
        if (enginePtr != 0L) TerminalEmulator.pasteTextFromRust(enginePtr, text)
    }

    // --- 颜色 ---

    /** ✅ 获取调色板 */
    fun getColors(enginePtr: Long): IntArray =
        if (enginePtr != 0L) TerminalEmulator.getColorsFromRust(enginePtr) else IntArray(259)

    /** ✅ 重置颜色 */
    fun resetColors(enginePtr: Long) {
        if (enginePtr != 0L) TerminalEmulator.resetColorsFromRust(enginePtr)
    }

    /** ✅ 从 Properties 更新颜色 */
    fun updateColors(enginePtr: Long, properties: java.util.Properties) {
        if (enginePtr != 0L) TerminalEmulator.updateColorsFromProperties(enginePtr, properties)
    }

    /** ✅ 设置光标颜色 */
    fun setCursorColorForBackground(enginePtr: Long) {
        if (enginePtr != 0L) TerminalEmulator.setCursorColorForBackgroundFromRust(enginePtr)
    }

    // --- 其他 ---

    /** ✅ 滚动计数器 */
    fun getScrollCounter(enginePtr: Long): Int =
        if (enginePtr != 0L) TerminalEmulator.getScrollCounterFromRust(enginePtr) else 0

    /** ✅ 清除滚动计数器 */
    fun clearScrollCounter(enginePtr: Long) {
        if (enginePtr != 0L) TerminalEmulator.clearScrollCounterFromRust(enginePtr)
    }

    /** ✅ 自动滚动禁用 */
    fun isAutoScrollDisabled(enginePtr: Long): Boolean =
        enginePtr != 0L && TerminalEmulator.isAutoScrollDisabledFromRust(enginePtr)

    /** ✅ 切换自动滚动 */
    fun toggleAutoScrollDisabled(enginePtr: Long) {
        if (enginePtr != 0L) TerminalEmulator.toggleAutoScrollDisabledFromRust(enginePtr)
    }

    /** ✅ 更新客户端回调 */
    fun updateTerminalSessionClient(enginePtr: Long, client: TerminalSessionClient?) {
        if (enginePtr != 0L) TerminalEmulator.updateTerminalSessionClientFromRust(enginePtr, client)
    }

    /** ✅ 调试信息 */
    fun getDebugInfo(enginePtr: Long): String =
        if (enginePtr != 0L) TerminalEmulator.getDebugInfoFromRust(enginePtr) else "null"


    // =========================================================================
    // JNI 全局方法 (不绑定 enginePtr)
    // =========================================================================

    // --- PTY ---
    /** ✅ 创建子进程 */
    fun createSubprocess(
        cmd: String, cwd: String, args: Array<String>?, envVars: Array<String>?,
        processId: IntArray, rows: Int, columns: Int, cellWidth: Int, cellHeight: Int
    ): Int = JNI.createSubprocess(cmd, cwd, args, envVars, processId, rows, columns, cellWidth, cellHeight)

    /** ✅ 异步创建会话 */
    fun createSessionAsync(
        cmd: String, cwd: String, args: Array<String>?, envVars: Array<String>?,
        rows: Int, columns: Int, cellWidth: Int, cellHeight: Int,
        transcriptRows: Int, callback: JNI.RustEngineCallback
    ) = JNI.createSessionAsync(cmd, cwd, args, envVars, rows, columns, cellWidth, cellHeight, transcriptRows, callback)

    /** ✅ 设置 PTY 窗口大小 */
    fun setPtyWindowSize(fd: Int, rows: Int, cols: Int, cellWidth: Int, cellHeight: Int) =
        JNI.setPtyWindowSize(fd, rows, cols, cellWidth, cellHeight)

    /** ✅ 等待进程 */
    fun waitFor(pid: Int): Int = JNI.waitFor(pid)

    /** ✅ 关闭 FD */
    fun close(fd: Int) = JNI.close(fd)

    // --- Session 协调器 ---
    /** ✅ 注册会话 */
    fun registerSession(): Int = JNI.registerSession()

    /** ✅ 注销会话 */
    fun unregisterSession(sessionId: Int) = JNI.unregisterSession(sessionId)

    /** ✅ 获取包锁 */
    fun tryAcquirePkgLock(sessionId: Int): Boolean = JNI.tryAcquirePkgLock(sessionId)

    /** ✅ 释放包锁 */
    fun releasePkgLock(sessionId: Int) = JNI.releasePkgLock(sessionId)

    /** ✅ 是否持有包锁 */
    fun isPkgLockHeld(): Boolean = JNI.isPkgLockHeld()

    /** ✅ 获取锁持有者 */
    fun getPkgLockOwner(): Int = JNI.getPkgLockOwner()

    /** ✅ 获取会话状态 */
    fun getSessionState(sessionId: Int): String? = JNI.getSessionState(sessionId)

    /** ✅ 获取所有会话状态 */
    fun getAllSessionStates(): String? = JNI.getAllSessionStates()

    // --- 键盘 ---
    /** ✅ 获取按键转义序列 */
    fun getKeyCode(keyCode: Int, keyMode: Int, cursorApp: Boolean, keypad: Boolean): String? =
        JNI.getKeyCode(keyCode, keyMode, cursorApp, keypad)

    /** ✅ 从 termcap 获取转义序列 */
    fun getKeyCodeFromTermcap(termcap: String, cursorApp: Boolean, keypad: Boolean): String? =
        JNI.getKeyCodeFromTermcap(termcap, cursorApp, keypad)

    // --- 字符宽度 ---
    /** ✅ 获取 Unicode 字符宽度 (Rust 实现) */
    fun wcWidth(ucs: Int): Int = com.termux.terminal.WcWidth.widthRust(ucs)


    // =========================================================================
    // TerminalView JNI 方法 (渲染)
    // =========================================================================

    /** ✅ Surface 设置 */
    fun setSurface(surface: android.view.Surface) =
        com.termux.view.TerminalView.nativeSetSurface(surface)

    /** ✅ 尺寸变化 */
    fun onSizeChanged(width: Int, height: Int) =
        com.termux.view.TerminalView.nativeOnSizeChanged(width, height)

    /** ✅ 引擎指针 */
    fun setEnginePointer(enginePtr: Long) =
        com.termux.view.TerminalView.nativeSetEnginePointer(enginePtr)

    /** ✅ 字体大小 */
    fun setFontSize(fontSize: Float) =
        com.termux.view.TerminalView.nativeSetFontSize(fontSize)

    /** ✅ 字体指标 */
    fun getFontMetrics(buffer: FloatArray) =
        com.termux.view.TerminalView.nativeGetFontMetrics(buffer)

    /** ✅ 渲染参数更新 — 包含选择坐标 */
    fun updateRenderParams(
        scale: Float, scrollOffset: Float, topRow: Int,
        selX1: Int, selY1: Int, selX2: Int, selY2: Int, selActive: Boolean
    ) = com.termux.view.TerminalView.nativeUpdateRenderParams(
        scale, scrollOffset, topRow, selX1, selY1, selX2, selY2, selActive
    )

    /** ✅ 渲染 (已弃用路径) */
    fun render(
        enginePtr: Long, scale: Float, scrollOffset: Float,
        selX1: Int, selY1: Int, selX2: Int, selY2: Int, selActive: Boolean
    ) = com.termux.view.TerminalView.nativeRender(
        enginePtr, scale, scrollOffset, selX1, selY1, selX2, selY2, selActive
    )
}
