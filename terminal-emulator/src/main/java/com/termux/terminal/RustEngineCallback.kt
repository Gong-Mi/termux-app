package com.termux.terminal

import androidx.annotation.NonNull
import androidx.annotation.Nullable

/**
 * 回调接口：由 Rust 引擎直接通过 JNI 调用
 * 注意：此类必须是顶层公共类，以便 JNI 反射能够轻松找到方法
 *
 * 实现 TerminalSessionClient 接口，以便可以直接传给 Rust JNI
 */
class RustEngineCallback(private val mClient: TerminalSessionClient?) : TerminalSessionClient {

    private var mSession: TerminalSession? = null

    fun setSession(session: TerminalSession) {
        mSession = session
    }

    fun onScreenUpdate() {
        // 屏幕更新通知 - 目前不需要特殊处理
    }

    fun onScreenUpdated() {
        if (mSession != null) {
            mSession!!.onNativeScreenUpdated()
        } else if (mClient != null) {
            mClient.onTextChanged(null)
        }
    }

    /**
     * Called when the Rust engine and PTY are initialized asynchronously.
     */
    fun onEngineInitialized(enginePtr: Long, ptyFd: Int, pid: Int) {
        mSession?.onEngineInitialized(enginePtr, ptyFd, pid)
    }

    fun reportTitleChange(title: String?) {
        mClient?.reportTitleChange(title)
    }

    fun onColorsChanged() {
        mClient?.onColorsChanged()
    }

    fun reportCursorVisibility(visible: Boolean) {
        mClient?.onTerminalCursorStateChange(visible)
    }

    fun onBell() {
        mClient?.onBell()
    }

    fun onCopyTextToClipboard(text: String) {
        mClient?.onCopyTextToClipboard(null, text)
    }

    fun onPasteTextFromClipboard() {
        mClient?.onPasteTextFromClipboard(null)
    }

    fun onWriteToSession(data: String) {
        // 将终端响应（DSR、光标位置、颜色查询等）写回 PTY
        // 否则嵌套 shell 会在等待响应时无限期挂起
        if (!data.isNullOrEmpty() && mSession != null) {
            mSession!!.write(data.toByteArray(Charsets.UTF_8))
        } else if (mClient != null) {
            mClient.logVerbose("RustEngineCallback", "Write to session: $data")
        }
    }

    fun onWriteToSessionBytes(data: ByteArray) {
        // 二进制数据写入 PTY
        if (!data.isNullOrEmpty() && mSession != null) {
            mSession!!.write(data, 0, data.size)
        } else if (mClient != null) {
            mClient.logVerbose("RustEngineCallback", "Write ${data.size} bytes to session")
        }
    }

    fun write(data: String) = onWriteToSession(data)
    fun writeBytes(data: ByteArray) = onWriteToSessionBytes(data)

    fun reportColorResponse(colorSpec: String) = write(colorSpec)
    fun reportTerminalResponse(response: String) = write(response)

    /**
     * Sixel 图像回调 - 由 Rust 引擎通过 JNI 调用
     */
    fun onSixelImage(rgbaData: ByteArray?, width: Int, height: Int, startX: Int, startY: Int) {
        if (mClient != null) {
            mClient.logDebug("SixelImage", String.format(
                "Received Sixel image: %dx%d at (%d,%d), data size: %d",
                width, height, startX, startY, rgbaData?.size ?: 0
            ))
            mClient.onSixelImage(rgbaData, width, height, startX, startY)
        }
    }

    /**
     * 清屏回调 - 由 Rust 引擎通过 JNI 调用
     */
    fun onClearScreen() {
        if (mClient != null) {
            mClient.logDebug("SixelImage", "Clear screen event received")
            mClient.onClearScreen()
        }
    }

    // --- TerminalSessionClient 接口实现 - 委托给 mClient ---

    override fun onTextChanged(@NonNull changedSession: TerminalSession) {
        mClient?.onTextChanged(changedSession)
    }

    override fun onTitleChanged(@NonNull changedSession: TerminalSession) {
        mClient?.onTitleChanged(changedSession)
    }

    override fun onSessionFinished(@NonNull finishedSession: TerminalSession) {
        mClient?.onSessionFinished(finishedSession)
    }

    override fun onCopyTextToClipboard(@NonNull session: TerminalSession, text: String) {
        mClient?.onCopyTextToClipboard(session, text)
    }

    override fun onPasteTextFromClipboard(@Nullable session: TerminalSession?) {
        mClient?.onPasteTextFromClipboard(session)
    }

    override fun onBell(@NonNull session: TerminalSession) {
        mClient?.onBell(session)
    }

    override fun onColorsChanged(@NonNull session: TerminalSession) {
        mClient?.onColorsChanged(session)
    }

    override fun onTerminalCursorStateChange(state: Boolean) {
        mClient?.onTerminalCursorStateChange(state)
    }

    override fun setTerminalShellPid(@NonNull session: TerminalSession, pid: Int) {
        mClient?.setTerminalShellPid(session, pid)
    }

    @Nullable
    override fun getTerminalCursorStyle(): Int? = mClient?.getTerminalCursorStyle()

    override fun logError(tag: String, message: String) { mClient?.logError(tag, message) }
    override fun logWarn(tag: String, message: String) { mClient?.logWarn(tag, message) }
    override fun logInfo(tag: String, message: String) { mClient?.logInfo(tag, message) }
    override fun logDebug(tag: String, message: String) { mClient?.logDebug(tag, message) }
    override fun logVerbose(tag: String, message: String) { mClient?.logVerbose(tag, message) }
    override fun logStackTraceWithMessage(tag: String, message: String, e: Exception?) {
        mClient?.logStackTraceWithMessage(tag, message, e)
    }
    override fun logStackTrace(tag: String, e: Exception?) {
        mClient?.logStackTrace(tag, e)
    }

    companion object {
        @JvmStatic fun create(client: TerminalSessionClient?): RustEngineCallback =
            RustEngineCallback(client)
    }
}
