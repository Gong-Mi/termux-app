package com.termux.terminal

import android.annotation.SuppressLint
import android.os.Handler
import android.os.Looper
import android.os.Message
import android.system.ErrnoException
import java.io.File
import java.io.FileDescriptor
import java.io.FileOutputStream
import java.io.IOException
import java.lang.reflect.Field
import java.nio.charset.StandardCharsets
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

/**
 * A terminal session, consisting of a process coupled to a terminal interface.
 *
 * The subprocess will be executed by the constructor, and when the size is made known by a call to
 * [updateSize] terminal emulation will begin and threads will be spawned to handle the subprocess I/O.
 * All terminal emulation and callback methods will be performed on the main thread.
 *
 * The child process may be exited forcefully by using the [finishIfRunning] method.
 *
 * NOTE: The terminal session may outlive the EmulatorView, so be careful with callbacks!
 */
class TerminalSession(
    val shellPath: String,
    private val cwd: String?,
    val args: Array<String?>?,
    val env: Array<String?>?,
    val transcriptRows: Int?,
    client: TerminalSessionClient
) : TerminalOutput() {

    companion object {
        private const val MSG_NEW_INPUT = 1
        private const val MSG_SCREEN_UPDATED = 5
        private const val LOG_TAG = "TerminalSession"
    }

    @JvmField
    val mHandle: String = UUID.randomUUID().toString()
    @JvmField
    @Volatile var mEmulator: TerminalEmulator? = null
    @JvmField
    var mSessionName: String? = null

    /** A queue written to from a separate thread when the process outputs, and read by main thread. */
    internal val mProcessToTerminalIOQueue = ByteQueue(64 * 1024)

    /** A queue written to from the main thread due to user interaction, read by another thread. */
    internal val mTerminalToProcessIOQueue = ByteQueue(4096)

    /** Buffer to write translate code points into utf8 before writing to mTerminalToProcessIOQueue */
    private val mUtf8InputBuffer = ByteArray(5)

    @Volatile var mClient: TerminalSessionClient = client
        private set

    /** The pid of the shell process. 0 if not started and -1 if finished running. */
    @Volatile var mShellPid: Int = 0
        private set

    /** The exit status of the shell process. Only valid if mShellPid is -1. */
    @Volatile var mShellExitStatus: Int = 0
        private set

    /** The file descriptor referencing the master half of a pseudo-terminal pair. */
    private var mTerminalFileDescriptor: Int = -1
    @Volatile private var mNativeSessionId: Int = -1

    private val mRustCallback: RustEngineCallback = RustEngineCallback(client).also { it.setSession(this) }

    private val mLifecycleLock = Any()
    private enum class SessionState { IDLE, INITIALIZING, READY, DISPOSED }
    @Volatile private var mSessionState = SessionState.IDLE
    private val mScreenUpdatePending = AtomicBoolean(false)

    /** Immutable raw outcomes. Receipt does not mean Handler/client delivery. */
    data class CompletionFacts(val processKind: Int, val processCode: Int,
                               val ioKind: Int, val ioCode: Int)
    @Volatile private var mCompletionFacts: CompletionFacts? = null
    fun getCompletionFacts(): CompletionFacts? = mCompletionFacts
    private enum class CompletionDelivery { NONE, PENDING, POSTED, RUNNING, ATTEMPTED, DELIVERED, FAILED, CANCELLED }
    @Volatile private var mCompletionDelivery = CompletionDelivery.NONE
    fun getCompletionDeliveryState(): String = mCompletionDelivery.name
    /** Null means the process exit status was lost; never export a fabricated process code. */
    fun getProcessExitStatus(): Int? = mCompletionFacts?.let { if (it.processKind == 2) it.processCode else null }
    fun getCompletionError(): String? = mCompletionFacts?.let { facts ->
        val errors = mutableListOf<String>()
        if (facts.processKind == 3) errors.add("Process exit status unavailable (errno=${facts.processCode})")
        val ioError = when (facts.ioKind) {
            3 -> "PTY output cancelled; output may be incomplete"
            4 -> "PTY IO failed (errno=${facts.ioCode}); output may be incomplete"
            5 -> "PTY response queue overflow; output may be incomplete"
            6 -> "PTY worker panicked; output may be incomplete"
            else -> null
        }
        ioError?.let { errors.add(it) }
        errors.takeIf { it.isNotEmpty() }?.joinToString("; ")
    }

    /** Native callback may precede adoption; queue completion only once READY. */
    fun onNativeCompletion(sessionId: Int, processKind: Int, processCode: Int,
                           ioKind: Int, ioCode: Int): Boolean = synchronized(mLifecycleLock) {
        if (sessionId != mNativeSessionId || mSessionState == SessionState.DISPOSED ||
            mSessionState == SessionState.IDLE || mCompletionFacts != null) return@synchronized false
        if (processKind !in 2..3 || ioKind !in 2..6) return@synchronized false
        mCompletionFacts = CompletionFacts(processKind, processCode, ioKind, ioCode)
        mCompletionDelivery = CompletionDelivery.PENDING
        postCompletionLocked()
        true
    }

    val mMainThreadHandler = MainThreadHandler()

    /** Update the client for this session. */
    fun updateTerminalSessionClient(client: TerminalSessionClient) {
        synchronized(mLifecycleLock) {
            if (mSessionState == SessionState.DISPOSED) return
            mClient = client
            mRustCallback.updateClient(client)
            // Native already holds this stable bridge; only its client reference changes.
        }
    }

    /** Inform the attached pty of the new size and reflow or initialize the emulator. */
    fun updateSize(columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int) {
        if (mEmulator == null && mSessionState == SessionState.IDLE) {
            initializeEmulator(columns, rows, cellWidthPixels, cellHeightPixels)
        } else if (mSessionState == SessionState.READY && mEmulator != null) {
            // Native resize routes the ioctl to the live IO owner. Never use a
            // cached raw descriptor after a concurrent worker close/reuse.
            mEmulator?.takeIf { it.isAlive() }?.resize(columns, rows, cellWidthPixels, cellHeightPixels)
        }
    }

    /** The terminal title as set through escape sequences or null if none set. */
    fun getTitle(): String? {
        if (mSessionState != SessionState.READY) return null
        val emulator = mEmulator ?: return null
        return if (emulator.isAlive()) emulator.getTitle() else null
    }

    /** Begin at most once; init, offer adoption and disposal share one lifecycle lock. */
    fun initializeEmulator(columns: Int, rows: Int, cellWidthPixels: Int, cellHeightPixels: Int) {
        val failure = synchronized(mLifecycleLock) {
            if (mSessionState != SessionState.IDLE) return
            mSessionState = SessionState.INITIALIZING
            if (!JNI.sNativeLibrariesLoaded) {
                mSessionState = SessionState.IDLE
                "Native libraries unavailable"
            } else {
                val sessionId = JNI.registerSession()
                if (sessionId < 0) {
                    mSessionState = SessionState.IDLE
                    "Native session ID allocation failed"
                } else {
                    mNativeSessionId = sessionId
                    mRustCallback.setNativeSessionId(sessionId)
                    try {
                        JNI.createSessionAsync(sessionId, shellPath, cwd ?: "", args, env,
                            rows, columns, cellWidthPixels, cellHeightPixels,
                            transcriptRows ?: TerminalEmulator.DEFAULT_TERMINAL_TRANSCRIPT_ROWS, mRustCallback)
                        null
                    } catch (t: Exception) {
                        JNI.terminateSession(sessionId)
                        JNI.unregisterSession(sessionId)
                        mNativeSessionId = -1
                        // A creator may already have escaped; never reuse this callback generation.
                        "Native session creation failed: ${t.message}"
                    }
                }
            }
        }
        failure?.let { mClient.logError(LOG_TAG, it) }
    }

    /** Native offers remain coordinator-owned until the queued adoption acknowledges them. */
    fun onEngineInitialized(enginePtr: Long, ptyFd: Int, pid: Int) {
        synchronized(mLifecycleLock) {
            val sessionId = mNativeSessionId
            if (mSessionState != SessionState.INITIALIZING) {
                JNI.rejectEngineData(sessionId, enginePtr)
                return
            }
            val posted = mMainThreadHandler.post {
                val adopted = synchronized(mLifecycleLock) {
                    if (mSessionState != SessionState.INITIALIZING || mNativeSessionId != sessionId) {
                        JNI.rejectEngineData(sessionId, enginePtr)
                        false
                    } else {
                        val data = JNI.claimEngineData(sessionId, enginePtr)
                        if (data == null) {
                            // No successful claim means no authority to reject.
                            // A duplicate offer may race a different claimant.
                            false
                        } else {
                            // Until ack, the wrapper only borrows the native owner's token.
                            // Constructor failures must reject, never create a replacement engine.
                            val emulator = try {
                                require(data.size == 3 && data[0] == enginePtr && data[1] == ptyFd.toLong() && data[2] == pid.toLong())
                                TerminalEmulator(this, enginePtr, ptyFd, mRustCallback)
                            } catch (t: Throwable) {
                                JNI.rejectEngineData(sessionId, enginePtr)
                                null
                            }
                            if (emulator == null) false
                            else if (!JNI.ackEngineData(sessionId, enginePtr)) {
                                JNI.rejectEngineData(sessionId, enginePtr)
                                false
                            } else {
                                mEmulator = emulator
                                mTerminalFileDescriptor = ptyFd
                                mShellPid = pid
                                mSessionState = SessionState.READY
                                postCompletionLocked()
                                true
                            }
                        }
                    }
                }
                // Exceptions from client code cannot roll ownership back after ack.
                // Never retain a pre-adoption client snapshot or call clients under the lock.
                if (adopted && mSessionState == SessionState.READY) {
                    mClient.setTerminalShellPid(this, pid)
                    mClient.onSessionStateChanged(this)
                    mClient.onTextChanged(this)
                    notifyScreenUpdate()
                }
            }
            if (!posted) JNI.rejectEngineData(sessionId, enginePtr)
        }
    }

    /** Explicit removal, not process exit. Revokes pending offers before releasing adopted state. */
    fun dispose() {
        synchronized(mLifecycleLock) {
            if (mSessionState == SessionState.DISPOSED) return
            mSessionState = SessionState.DISPOSED
            if (mCompletionDelivery == CompletionDelivery.PENDING || mCompletionDelivery == CompletionDelivery.POSTED) {
                mCompletionDelivery = CompletionDelivery.CANCELLED
            }
            val sessionId = mNativeSessionId
            mNativeSessionId = -1
            if (JNI.sNativeLibrariesLoaded && sessionId >= 0) {
                runCatching { JNI.terminateSession(sessionId) }
                runCatching { JNI.unregisterSession(sessionId) }
            }
            runCatching { mEmulator?.destroy() }
            mEmulator = null
            mTerminalFileDescriptor = -1
            mShellPid = -1
            mTerminalToProcessIOQueue.close()
            mProcessToTerminalIOQueue.close()
            mMainThreadHandler.removeCallbacksAndMessages(null)
            mScreenUpdatePending.set(false)
            mRustCallback.clear()
            mClient = RustEngineCallback(null)
        }
    }

    fun isEngineInitialized(): Boolean = mSessionState == SessionState.READY

    /** Write data to the shell process. */
    override fun write(data: ByteArray, offset: Int, count: Int) {
        if (mSessionState != SessionState.READY) return
        val emulator = mEmulator ?: return
        val ptr = emulator.getNativePointer()
        if (ptr != 0L) {
            val status = RustTerminal.tryProcessInput(ptr, data, offset, count)
            if (status != RustTerminal.INPUT_ACCEPTED) {
                mClient.logWarn(LOG_TAG, "PTY input rejected (status=$status, bytes=$count); input was not queued")
                mClient.onBell(this)
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

    /** Request termination through the native child owner, including pending bind.
     * mShellPid remains a presentation value, never native process identity. */
    fun finishIfRunning() {
        val sessionId = mNativeSessionId
        if (JNI.sNativeLibrariesLoaded && sessionId >= 0) {
            runCatching { JNI.terminateSession(sessionId) }
                .onSuccess { accepted ->
                    if (!accepted) mClient.logVerbose(LOG_TAG, "Process already exited or session unavailable")
                }
                .onFailure { e -> mClient.logWarn(LOG_TAG, "Failed requesting process termination: ${e.message}") }
        }
    }

    /** Called with mLifecycleLock held. post success is only queue admission. */
    private fun postCompletionLocked() {
        if (mSessionState != SessionState.READY || mCompletionDelivery != CompletionDelivery.PENDING) return
        val sessionId = mNativeSessionId
        mCompletionDelivery = CompletionDelivery.POSTED
        try {
            if (!mMainThreadHandler.post { deliverCompletion(sessionId) }) {
                mCompletionDelivery = CompletionDelivery.FAILED
            }
        } catch (_: RuntimeException) {
            mCompletionDelivery = CompletionDelivery.FAILED
        }
    }

    private fun deliverCompletion(sessionId: Int) {
        val client = try {
            synchronized(mLifecycleLock) {
                if (mSessionState != SessionState.READY || sessionId != mNativeSessionId ||
                    mCompletionDelivery != CompletionDelivery.POSTED) return
                val facts = mCompletionFacts ?: return
                val emulator = mEmulator?.takeIf { it.isAlive() } ?: run {
                    mCompletionDelivery = CompletionDelivery.FAILED
                    return
                }
                mCompletionDelivery = CompletionDelivery.RUNNING
                // Compatibility presentation code only; shared results use nullable
                // getProcessExitStatus(), so Lost is never exported as process exit 1.
                mShellExitStatus = if (facts.processKind == 2) facts.processCode else 1
                mShellPid = -1
                var description = "\r\n[Process completed"
                if (facts.processKind == 2) description += when {
                    facts.processCode > 0 -> " (code ${facts.processCode})"
                    facts.processCode < 0 -> " (signal ${-facts.processCode})"
                    else -> ""
                }
                getCompletionError()?.let { description += " - $it" }
                description += " - press Enter]"
                val bytes = description.toByteArray(StandardCharsets.UTF_8)
                emulator.append(bytes, bytes.size)
                mClient
            }
        } catch (failure: Throwable) {
            mCompletionDelivery = CompletionDelivery.FAILED
            runCatching { mClient.logError(LOG_TAG, "Completion preparation failed: ${failure.message}") }
            return
        }
        // Client code is outside lifecycle/native locks. A throwing render notification
        // must not suppress the separate result callback; neither is retried.
        var failed = false
        try { client.onTextChanged(this) } catch (_: Throwable) { failed = true }
        if (mSessionState == SessionState.DISPOSED) {
            mCompletionDelivery = CompletionDelivery.CANCELLED
            return
        }
        mCompletionDelivery = CompletionDelivery.ATTEMPTED
        try { client.onSessionFinished(this) } catch (_: Throwable) { failed = true }
        mCompletionDelivery = if (failed) CompletionDelivery.FAILED else CompletionDelivery.DELIVERED
        // The client may synchronously read results/remove/dispose. Never destroy here.
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
    inner class MainThreadHandler : Handler() {
        val mReceiveBuffer = ByteArray(64 * 1024)

        override fun handleMessage(msg: Message) {
            val emulator = mEmulator
            if (emulator == null || !emulator.isAlive()) return

            if (msg.what == MSG_SCREEN_UPDATED) {
                notifyScreenUpdate()
                return
            }

            // --- 这里的旧代码（读取 mProcessToTerminalIOQueue）已被移除 ---
            // 所有 IO 逻辑现在完全在 Rust 后台线程中处理
        }
    }
}

private fun wrapFileDescriptor(fileDescriptor: Int, client: TerminalSessionClient): FileDescriptor {
    val result = FileDescriptor()
    try {
        val descriptorField = runCatching { FileDescriptor::class.java.getDeclaredField("descriptor") }
            .recoverCatching { FileDescriptor::class.java.getDeclaredField("fd") }
            .getOrElse { throw it }
        descriptorField.isAccessible = true
        descriptorField.set(result, fileDescriptor)
    } catch (e: Exception) {
        client.logStackTraceWithMessage("TerminalSession", "Error accessing FileDescriptor#descriptor private field", e)
        System.exit(1)
    }
    return result
}
