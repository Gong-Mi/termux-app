package com.termux.terminal

import android.annotation.SuppressLint
import android.os.Handler
import android.os.Message
import android.system.Os
import android.system.OsConstants
import java.io.File
import java.nio.charset.StandardCharsets
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

/**
 * A terminal session, consisting of a process coupled to a terminal interface.
 *
 * All process I/O and lifecycle management are handled natively in Rust.
 * This Kotlin class acts as a state synchronization layer for the UI.
 */
class TerminalSession(
    val shellPath: String?,
    private val cwd: String?,
    val args: Array<String?>?,
    val env: Array<String?>?,
    val transcriptRows: Int?,
    val isFailSafe: Boolean = false,
    client: TerminalSessionClient
) : TerminalOutput() {

    companion object {
        private const val MSG_PROCESS_EXITED = 4
        private const val MSG_SCREEN_UPDATED = 5
        private const val LOG_TAG = "TerminalSession"
    }

    @JvmField
    val mHandle: String = UUID.randomUUID().toString()
    @JvmField
    var mEmulator: TerminalEmulator? = null
    @JvmField
    var mSessionName: String? = null

    /** Buffer to translate code points into utf8 before writing natively */
    private val mUtf8InputBuffer = ByteArray(5)

    var mClient: TerminalSessionClient = client
        private set

    /** The pid of the shell process. 0 if not started and -1 if finished running. */
    var mShellPid: Int = 0
        private set

    /** The exit status of the shell process. Only valid if mShellPid is -1. */
    var mShellExitStatus: Int = 0
        private set

    /** The file descriptor referencing the master half of a pseudo-terminal pair. */
    private var mTerminalFileDescriptor: Int = -1

    private val mRustCallback: RustEngineCallback = RustEngineCallback(client).also { it.setSession(this) }

    private enum class SessionState { IDLE, INITIALIZING, READY }
    private var mSessionState = SessionState.IDLE
    private val mScreenUpdatePending = AtomicBoolean(false)

    val mMainThreadHandler = MainThreadHandler()

    /** Update the client for this session. */
    fun updateTerminalSessionClient(client: TerminalSessionClient) {
        mClient = client
        mRustCallback.updateClient(client)
        mEmulator?.takeIf { it.isAlive() }?.updateTerminalSessionClient(client)
    }

    /** Inform the attached pty of the new size and reflow or initialize the emulator. */
    fun updateSize(columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int) {
        if (mEmulator == null && mSessionState == SessionState.IDLE) {
            initializeEmulator(columns, rows, cellWidthPixels, cellHeightPixels)
        } else if (mSessionState == SessionState.READY && mEmulator != null) {
            if (JNI.sNativeLibrariesLoaded && mTerminalFileDescriptor != -1) {
                runCatching { JNI.setPtyWindowSize(mTerminalFileDescriptor, rows, columns, cellWidthPixels, cellHeightPixels) }
            }
            mEmulator?.takeIf { it.isAlive() }?.resize(columns, rows, cellWidthPixels, cellHeightPixels)
        }
    }

    /** The terminal title as set through escape sequences or null if none set. */
    fun getTitle(): String? {
        if (mSessionState != SessionState.READY || mEmulator == null || !mEmulator!!.isAlive()) return null
        return mEmulator!!.getTitle()
    }

    /** Set the terminal emulator's window size and start terminal emulation asynchronously. */
    fun initializeEmulator(columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int) {
        android.util.Log.d("TermuxTrace", "[TRACE_SESSION] 4. initializeEmulator called (${columns}x${rows})")
        mSessionState = SessionState.INITIALIZING
        
        // Add timeout check
        mMainThreadHandler.postDelayed({
            if (mSessionState == SessionState.INITIALIZING) {
                android.util.Log.e("TermuxTrace", "[TRACE_SESSION] TIMEOUT: onEngineInitialized was not called within 5 seconds")
                onEngineInitializationFailed("Timeout waiting for terminal engine initialization")
            }
        }, 5000)

        if (JNI.sNativeLibrariesLoaded) {
            android.util.Log.d("TermuxTrace", "[TRACE_SESSION] 5. Calling JNI.createSessionAsync with callback: $mRustCallback")
            JNI.createSessionAsync(
                shellPath ?: "", cwd ?: "", args, env, rows, columns, cellWidthPixels, cellHeightPixels,
                transcriptRows ?: TerminalEmulator.DEFAULT_TERMINAL_TRANSCRIPT_ROWS, mRustCallback,
                isFailSafe
            )
        } else {
            android.util.Log.w("TermuxTrace", "[TRACE_SESSION] JNI libraries not loaded, using mock")
            mShellPid = 99999
            mTerminalFileDescriptor = -1
            mEmulator = TerminalEmulator(this, columns, rows, cellWidthPixels, cellHeightPixels, transcriptRows, mTerminalFileDescriptor, mClient)
            mSessionState = SessionState.READY
            mClient.setTerminalShellPid(this, mShellPid)
        }
    }

    /** Callback from Rust when async initialization is complete. */
    fun onEngineInitialized(enginePtr: Long, ptyFd: Int, pid: Int) {
        android.util.Log.d("TermuxTrace", "[TRACE_SESSION] 6. onEngineInitialized callback received (pid=$pid)")
        mMainThreadHandler.post {
            android.util.Log.d("TermuxTrace", "[TRACE_SESSION] 7. Running onEngineInitialized logic on MainThread")
            mSessionState = SessionState.READY
            mTerminalFileDescriptor = ptyFd
            mShellPid = pid

            mEmulator = TerminalEmulator(this, enginePtr, ptyFd, mRustCallback)
            mClient.setTerminalShellPid(this, mShellPid)
            android.util.Log.d("TermuxTrace", "[TRACE_SESSION] 8. Emulator instance created")

            // 启动 Rust 端的数据循环以开始接管 PTY 读取
            RustTerminal.startIoThread(enginePtr, ptyFd)

            mClient.onSessionStateChanged(this)
            mClient.onTextChanged(this)

            notifyScreenUpdate()
        }
    }

    /** Callback from Rust when async initialization fails. */
    fun onEngineInitializationFailed(error: String) {
        android.util.Log.e("TermuxTrace", "[TRACE_SESSION] Engine initialization FAILED: $error")
        mMainThreadHandler.post {
            mSessionState = SessionState.IDLE
            mClient.onEngineInitializationFailed(error)
            mClient.logError(LOG_TAG, "Terminal engine initialization failed: $error")
        }
    }

    /** Called from native waiter thread via RustEngineCallback. */
    fun onProcessExited(exitCode: Int) {
        mMainThreadHandler.sendMessage(mMainThreadHandler.obtainMessage(MSG_PROCESS_EXITED, exitCode))
    }

    fun isEngineInitialized(): Boolean = mSessionState == SessionState.READY

    /** Write data to the shell process natively. */
    override fun write(data: ByteArray, offset: Int, count: Int) {
        if (mSessionState != SessionState.READY) return
        if (mShellPid > 0 && mTerminalFileDescriptor != -1) {
            JNI.nativeWrite(mTerminalFileDescriptor, data, offset, count)
        }
    }

    /** Write data to the shell process natively (Zero-Copy version). */
    override fun write(buffer: java.nio.ByteBuffer, count: Int) {
        if (mSessionState != SessionState.READY) return
        if (mShellPid > 0 && mTerminalFileDescriptor != -1) {
            if (buffer.isDirect) {
                JNI.nativeWriteDirect(mTerminalFileDescriptor, buffer, buffer.position(), count)
                buffer.position(buffer.position() + count)
            } else {
                super.write(buffer, count)
            }
        }
    }

    /** Write the Unicode code point to the terminal encoded in UTF-8. */
    fun writeCodePoint(prependEscape: Boolean, codePoint: Int) {
        if (codePoint > 1114111 || codePoint in 0xD800..0xDFFF) {
            throw IllegalArgumentException("Invalid code point: $codePoint")
        }

        var bufferPosition = 0
        if (prependEscape) mUtf8InputBuffer[bufferPosition++] = 27.toByte()

        when {
            codePoint <= 0b1111111 -> {
                mUtf8InputBuffer[bufferPosition++] = codePoint.toByte()
            }
            codePoint <= 0b11111111111 -> {
                mUtf8InputBuffer[bufferPosition++] = (0b11000000 or (codePoint shr 6)).toByte()
                mUtf8InputBuffer[bufferPosition++] = (0b10000000 or (codePoint and 0b111111)).toByte()
            }
            codePoint <= 0b1111111111111111 -> {
                mUtf8InputBuffer[bufferPosition++] = (0b11100000 or (codePoint shr 12)).toByte()
                mUtf8InputBuffer[bufferPosition++] = (0b10000000 or ((codePoint shr 6) and 0b111111)).toByte()
                mUtf8InputBuffer[bufferPosition++] = (0b10000000 or (codePoint and 0b111111)).toByte()
            }
            else -> {
                mUtf8InputBuffer[bufferPosition++] = (0b11110000 or (codePoint shr 18)).toByte()
                mUtf8InputBuffer[bufferPosition++] = (0b10000000 or ((codePoint shr 12) and 0b111111)).toByte()
                mUtf8InputBuffer[bufferPosition++] = (0b10000000 or ((codePoint shr 6) and 0b111111)).toByte()
                mUtf8InputBuffer[bufferPosition++] = (0b10000000 or (codePoint and 0b111111)).toByte()
            }
        }
        write(mUtf8InputBuffer, 0, bufferPosition)
    }

    fun getEmulator(): TerminalEmulator? = mEmulator

    /** Notify the client that the screen has changed. */
    private fun notifyScreenUpdate() {
        mScreenUpdatePending.set(false)
        mClient.onTextChanged(this)
    }

    /** Called by Rust IO thread when screen needs updating */
    fun onNativeScreenUpdated() {
        if (mSessionState == SessionState.READY && mScreenUpdatePending.compareAndSet(false, true)) {
            mMainThreadHandler.sendEmptyMessage(MSG_SCREEN_UPDATED)
        }
    }

    /** Reset state for terminal emulator state. */
    fun reset() {
        mEmulator?.takeIf { it.isAlive() }?.apply {
            reset()
            notifyScreenUpdate()
        }
    }

    /** Finish this terminal session by sending SIGKILL to the shell. */
    fun finishIfRunning() {
        if (isRunning) {
            runCatching { Os.kill(mShellPid, OsConstants.SIGKILL) }
                .onFailure { e -> mClient.logWarn(LOG_TAG, "Failed sending SIGKILL: ${e.message}") }
        }
    }

    /** Cleanup resources when the process exits. */
    private fun cleanupResources(exitStatus: Int) {
        android.util.Log.i("TermuxTrace", "[TRACE_SESSION] cleanupResources called with status: $exitStatus")
        mShellPid = -1
        mShellExitStatus = exitStatus
        mEmulator?.destroy()
        mEmulator = null
    }

    val isRunning: Boolean
        @Synchronized get() = mShellPid != -1

    /** Only valid if not [isRunning]. */
    @Synchronized
    fun getExitStatus(): Int = mShellExitStatus

    fun getPid(): Int = mShellPid

    /** Returns the shell's working directory or null if it was unavailable. */
    fun getCwd(): String? {
        if (mShellPid < 1) return null
        return runCatching {
            val cwdSymlink = "/proc/$mShellPid/cwd/"
            val outputPath = File(cwdSymlink).canonicalPath
            val outputPathWithSlash = if (!outputPath.endsWith("/")) "$outputPath/" else outputPath
            if (cwdSymlink != outputPathWithSlash) outputPath else null
        }.onFailure { ex -> mClient.logStackTraceWithMessage(LOG_TAG, "Error getting current directory", ex as? Exception) }.getOrNull()
    }

    // --- TerminalOutput overrides (delegate to mClient) ---
    override fun titleChanged(oldTitle: String?, newTitle: String?) { mClient.onTitleChanged(this) }
    override fun onCopyTextToClipboard(text: String?) { text?.let { mClient.onCopyTextToClipboard(this, it) } }
    override fun onPasteTextFromClipboard() { mClient.onPasteTextFromClipboard(this) }
    override fun onBell() { mClient.onBell(this) }
    override fun onColorsChanged() { mClient.onColorsChanged(this) }
    override fun onTerminalCursorStateChange(visible: Boolean) { mClient.onTerminalCursorStateChange(visible) }
    override fun onSixelImage(rgbaData: ByteArray?, width: Int, height: Int, startX: Int, startY: Int) {
        mClient.onSixelImage(rgbaData, width, height, startX, startY)
    }

    @SuppressLint("HandlerLeak")
    inner class MainThreadHandler : Handler(android.os.Looper.getMainLooper()) {
        override fun handleMessage(msg: Message) {
            if (msg.what != MSG_PROCESS_EXITED && (mEmulator == null || !mEmulator!!.isAlive())) return

            if (msg.what == MSG_SCREEN_UPDATED) {
                notifyScreenUpdate()
                return
            }

            if (msg.what == MSG_PROCESS_EXITED) {
                val exitCode = msg.obj as? Int ?: 0
                var exitDescription = "\r\n[Process completed"
                exitDescription += when {
                    exitCode > 0 -> " (code $exitCode)"
                    exitCode < 0 -> " (signal ${-exitCode})"
                    else -> ""
                }
                exitDescription += " - press Enter]\r\n"

                val bytesToWrite = exitDescription.toByteArray(StandardCharsets.UTF_8)
                if (mEmulator?.isAlive() == true) {
                    mEmulator!!.append(bytesToWrite, bytesToWrite.size)
                    notifyScreenUpdate()
                }
                cleanupResources(exitCode)
                mClient.onSessionFinished(this@TerminalSession)
            }
        }
    }
}
