package com.termux.terminal

/**
 * Native methods for creating and managing pseudoterminal subprocesses.
 *
 * All native implementations are in Rust (jni_bindings.rs).
 */
internal object JNI {

    @JvmField
    val sNativeLibrariesLoaded: Boolean

    init {
        var loaded = false
        android.util.Log.d("TermuxTrace", "[JNI] Start loading termux_rust library")
        try {
            val vendor = System.getProperty("java.vendor")
            android.util.Log.d("TermuxTrace", "[JNI] java.vendor: $vendor")
            if (vendor != null && vendor.contains("Android")) {
                android.util.Log.d("TermuxTrace", "[JNI] Attempting System.loadLibrary(\"termux_rust\")")
                System.loadLibrary("termux_rust")
                loaded = true
                android.util.Log.d("TermuxTrace", "[JNI] System.loadLibrary(\"termux_rust\") succeeded")
            } else {
                val libName = System.mapLibraryName("termux_rust")
                android.util.Log.d("TermuxTrace", "[JNI] Non-Android environment, libName: $libName")
                val possiblePaths = arrayOf(
                    "terminal-emulator/src/main/jniLibs/x86_64/$libName",
                    "src/main/jniLibs/x86_64/$libName",
                    "terminal-emulator/src/main/rust/target/release/$libName",
                    "src/main/rust/target/release/$libName",
                    "build/libs/$libName"
                )
                for (path in possiblePaths) {
                    val libFile = java.io.File(path)
                    android.util.Log.d("TermuxTrace", "[JNI] Checking path: ${libFile.absolutePath}")
                    if (libFile.exists()) {
                        android.util.Log.d("TermuxTrace", "[JNI] Found library at: ${libFile.absolutePath}, attempting System.load")
                        System.load(libFile.absolutePath)
                        loaded = true
                        android.util.Log.d("TermuxTrace", "[JNI] System.load succeeded")
                        break
                    }
                }
            }
        } catch (t: Throwable) {
            android.util.Log.e("TermuxTrace", "[JNI] Failed to load termux_rust library", t)
        }
        sNativeLibrariesLoaded = loaded
        android.util.Log.d("TermuxTrace", "[JNI] sNativeLibrariesLoaded: $sNativeLibrariesLoaded")
    }

    // --- PTY ---
    @JvmStatic external fun createSubprocess(
        cmd: String, cwd: String, args: Array<String?>?, envVars: Array<String?>?,
        processId: IntArray, rows: Int, columns: Int, cellWidth: Int, cellHeight: Int
    ): Int

    @JvmStatic external fun createSessionAsync(
        cmd: String, cwd: String, args: Array<String?>?, envVars: Array<String?>?,
        rows: Int, columns: Int, cellWidth: Int, cellHeight: Int,
        transcriptRows: Int, callback: RustEngineCallback,
        isFailSafe: Boolean
    )

    @JvmStatic external fun setPtyWindowSize(fd: Int, rows: Int, cols: Int, cellWidth: Int, cellHeight: Int)
    @JvmStatic external fun waitFor(processId: Int): Int
    @JvmStatic external fun close(fileDescriptor: Int)
    @JvmStatic external fun nativeWrite(fd: Int, data: ByteArray, offset: Int, count: Int): Int
    @JvmStatic external fun nativeWriteDirect(fd: Int, buffer: java.nio.ByteBuffer, offset: Int, count: Int): Int

    // --- Session Coordinator ---
    @JvmStatic external fun registerSession(): Int
    @JvmStatic external fun unregisterSession(sessionId: Int)
    @JvmStatic external fun tryAcquirePkgLock(sessionId: Int): Boolean
    @JvmStatic external fun releasePkgLock(sessionId: Int)
    @JvmStatic external fun isPkgLockHeld(): Boolean
    @JvmStatic external fun getPkgLockOwner(): Int
    @JvmStatic external fun getSessionState(sessionId: Int): String?
    @JvmStatic external fun getAllSessionStates(): String?

    // --- Session 状态查询（Rust 为唯一真相源） ---
    @JvmStatic external fun sessionGetPid(enginePtr: Long): Int
    @JvmStatic external fun sessionGetPtyFd(enginePtr: Long): Int
    @JvmStatic external fun sessionIsRunning(enginePtr: Long): Boolean

    // --- Termux 元数据 ---
    @JvmStatic external fun setTermuxVersion(version: String)
    @JvmStatic external fun setTermuxPrefix(prefix: String)
    @JvmStatic external fun setExtendedEnvironment(keys: Array<String>, values: Array<String>)

    // --- KeyHandler (Rust) ---
    @JvmStatic external fun getKeyCode(keyCode: Int, keyMode: Int, cursorApp: Boolean, keypad: Boolean): String?
    @JvmStatic external fun getKeyCodeFromTermcap(termcap: String, cursorApp: Boolean, keypad: Boolean): String?
}
