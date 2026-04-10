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
        try {
            val vendor = System.getProperty("java.vendor")
            if (vendor != null && vendor.contains("Android")) {
                System.loadLibrary("termux_rust")
                loaded = true
            } else {
                val libName = System.mapLibraryName("termux_rust")
                val possiblePaths = arrayOf(
                    "terminal-emulator/src/main/jniLibs/x86_64/$libName",
                    "src/main/jniLibs/x86_64/$libName",
                    "terminal-emulator/src/main/rust/target/release/$libName",
                    "src/main/rust/target/release/$libName",
                    "build/libs/$libName"
                )
                for (path in possiblePaths) {
                    val libFile = java.io.File(path)
                    if (libFile.exists()) {
                        System.load(libFile.absolutePath)
                        loaded = true
                        break
                    }
                }
            }
        } catch (_: Throwable) {
        }
        sNativeLibrariesLoaded = loaded
    }

    // --- PTY ---
    @JvmStatic external fun createSubprocess(
        cmd: String, cwd: String, args: Array<String?>?, envVars: Array<String?>?,
        processId: IntArray, rows: Int, columns: Int, cellWidth: Int, cellHeight: Int
    ): Int

    @JvmStatic external fun createSessionAsync(
        cmd: String, cwd: String, args: Array<String?>?, envVars: Array<String?>?,
        rows: Int, columns: Int, cellWidth: Int, cellHeight: Int,
        transcriptRows: Int, callback: RustEngineCallback
    )

    @JvmStatic external fun setPtyWindowSize(fd: Int, rows: Int, cols: Int, cellWidth: Int, cellHeight: Int)
    @JvmStatic external fun waitFor(processId: Int): Int
    @JvmStatic external fun close(fileDescriptor: Int)

    // --- Session Coordinator ---
    @JvmStatic external fun registerSession(): Int
    @JvmStatic external fun unregisterSession(sessionId: Int)
    @JvmStatic external fun tryAcquirePkgLock(sessionId: Int): Boolean
    @JvmStatic external fun releasePkgLock(sessionId: Int)
    @JvmStatic external fun isPkgLockHeld(): Boolean
    @JvmStatic external fun getPkgLockOwner(): Int
    @JvmStatic external fun getSessionState(sessionId: Int): String?
    @JvmStatic external fun getAllSessionStates(): String?

    // --- KeyHandler (Rust) ---
    @JvmStatic external fun getKeyCode(keyCode: Int, keyMode: Int, cursorApp: Boolean, keypad: Boolean): String?
    @JvmStatic external fun getKeyCodeFromTermcap(termcap: String, cursorApp: Boolean, keypad: Boolean): String?
}
