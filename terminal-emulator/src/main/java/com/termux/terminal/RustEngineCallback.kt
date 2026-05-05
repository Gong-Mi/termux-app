package com.termux.terminal

import androidx.annotation.NonNull
import androidx.annotation.Nullable
import androidx.annotation.Keep

/**
 * 回调接口：由 Rust 引擎直接通过 JNI 调用
 * 注意：此类必须是顶层公共类，以便 JNI 反射能够轻松找到方法
 *
 * 实现 TerminalSessionClient 接口，以便可以直接传给 Rust JNI
 */
@Keep
class RustEngineCallback(private var mClient: TerminalSessionClient?) : TerminalSessionClient {

    private var mSession: TerminalSession? = null

    fun setSession(session: TerminalSession) {
        mSession = session
    }

    fun updateClient(client: TerminalSessionClient?) {
        mClient = client
    }

    fun onScreenUpdate() {
        android.util.Log.v("TermuxTrace", "[JNI_CALLBACK] onScreenUpdate (singular) called")
        onScreenUpdated()
    }

    override fun onScreenUpdated() {
        // Trace log removed to reduce logcat noise and CPU overhead
        val session = mSession
        val client = mClient
        if (session != null) {
            session.onNativeScreenUpdated()
        } else if (client != null) {
            client.logVerbose("RustEngineCallback", "Screen updated but no session available")
        }
    }

    /**
     * Called when the Rust engine and PTY are initialized asynchronously.
     */
    fun onEngineInitialized(enginePtr: Long, ptyFd: Int, pid: Int) {
        android.util.Log.d("TermuxTrace", "[JNI_CALLBACK] onEngineInitialized: enginePtr=$enginePtr, ptyFd=$ptyFd, pid=$pid")
        mSession?.onEngineInitialized(enginePtr, ptyFd, pid)
    }

    /**
     * Called when the Rust engine initialization fails.
     */
    override fun onEngineInitializationFailed(error: String) {
        android.util.Log.e("TermuxTrace", "[JNI_CALLBACK] onEngineInitializationFailed: $error")
        mSession?.onEngineInitializationFailed(error)
    }

    /**
     * Called when the subprocess exits (called from native waiter thread).
     */
    @Keep
    fun onProcessExited(exitCode: Int) {
        android.util.Log.i("TermuxTrace", "[JNI_CALLBACK] onProcessExited: code=$exitCode")
        mSession?.onProcessExited(exitCode)
    }

    override fun reportTitleChange(title: String?) {
        mClient?.reportTitleChange(title)
    }

    /** Convenience method (no session) - delegates to interface method with session if available. */
    fun onColorsChanged() {
        val session = mSession
        val client = mClient
        if (session != null) {
            client?.onColorsChanged(session)
        } else {
            client?.logVerbose("RustEngineCallback", "Colors changed but no session available")
        }
    }

    fun reportCursorVisibility(visible: Boolean) {
        mClient?.onTerminalCursorStateChange(visible)
    }

    /** Convenience method (no session). */
    fun onBell() {
        val session = mSession
        val client = mClient
        if (session != null) {
            client?.onBell(session)
        } else {
            client?.logVerbose("RustEngineCallback", "Bell but no session available")
        }
    }

    fun onCopyTextToClipboard(text: String) {
        val session = mSession
        if (session != null) {
            mClient?.onCopyTextToClipboard(session, text)
        }
    }

    fun onPasteTextFromClipboard() {
        val session = mSession
        if (session != null) {
            mClient?.onPasteTextFromClipboard(session)
        }
    }

    fun onWriteToSession(data: String) {
        // 将终端响应（DSR、光标位置、颜色查询等）写回 PTY
        // 否则嵌套 shell 会在等待响应时无限期挂起
        val session = mSession
        if (data.isNotEmpty() && session != null) {
            session.write(data.toByteArray(Charsets.UTF_8))
        } else {
            mClient?.logVerbose("RustEngineCallback", "Write to session: $data")
        }
    }

    fun onWriteToSessionBytes(data: ByteArray) {
        // 二进制数据写入 PTY
        val session = mSession
        if (data.isNotEmpty() && session != null) {
            session.write(data, 0, data.size)
        } else {
            mClient?.logVerbose("RustEngineCallback", "Write ${data.size} bytes to session")
        }
    }

    fun write(data: String) = onWriteToSession(data)
    fun writeBytes(data: ByteArray) = onWriteToSessionBytes(data)

    fun reportColorResponse(colorSpec: String) = write(colorSpec)
    fun reportTerminalResponse(response: String) = write(response)

    /**
     * Sixel 图像回调 - 由 Rust 引擎通过 JNI 调用
     */
    override fun onSixelImage(rgbaData: ByteArray?, width: Int, height: Int, start_x: Int, start_y: Int) {
        val client = mClient
        if (client != null) {
            client.logDebug("SixelImage", String.format(
                "Received Sixel image: %dx%d at (%d,%d), data size: %d",
                width, height, start_x, start_y, rgbaData?.size ?: 0
            ))
            client.onSixelImage(rgbaData, width, height, start_x, start_y)
        }
    }

    /**
     * 清屏回调 - 由 Rust 引擎通过 JNI 调用
     */
    override fun onClearScreen() {
        val client = mClient
        if (client != null) {
            client.logDebug("TerminalScreen", "Clear screen event received")
            client.onClearScreen()
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
