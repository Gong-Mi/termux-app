package com.termux.terminal

/** Real production Kotlin + independently built JNI, isolated JVM stdin. */
fun main() {
    check(JNI.sNativeLibrariesLoaded) { "Construction smoke requires real termux_rust JNI" }
    val callback = RustEngineCallback(null)
    val pointer = RustTerminal.createEngine(80, 24, 8, 16, 2000, callback)
    check(pointer != 0L)
    val adopted = TerminalEmulator(session = null, enginePtr = pointer, ptyFd = 0, callback = callback)
    try {
        check(adopted.getNativePointer() == pointer)
        check(adopted.getCols() == 80 && adopted.getRows() == 24)
        val bytes = "adopted-engine".toByteArray(Charsets.UTF_8)
        adopted.append(bytes, bytes.size)
        check(RustTerminal.getCursorCol(pointer) == bytes.size)
        check(adopted.getTranscriptText().contains("adopted-engine"))
        adopted.resize(100, 30, 8, 16)
        check(RustTerminal.getCols(pointer) == 100 && RustTerminal.getRows(pointer) == 30)
    } finally {
        adopted.destroy()
    }
    check(adopted.getNativePointer() == 0L && !adopted.isAlive())
    val created = TerminalEmulator(null, 60, 20, 8, 16, null, -1, callback)
    try {
        check(created.isAlive() && created.getCols() == 60 && created.getRows() == 20)
    } finally {
        created.destroy()
    }
    println("PASS: real JNI existing-engine identity, parse, resize, destroy; new no-IO engine")
}
