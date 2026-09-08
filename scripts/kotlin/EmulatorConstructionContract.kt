package com.termux.terminal

/** Recording-boundary contract; not a native resource-lifetime test. */
fun main() {
    val callback = RustEngineCallback(null)
    // Include zero: adoption is a mode, not a nonzero-pointer heuristic.
    for (pointer in listOf(0L, 0x123456789ABCDEFL, -7L)) {
        RustTerminal.clearRecording()
        val adopted = TerminalEmulator(null, pointer, 0, callback)
        check(RustTerminal.createCalls == 0) { "adoption must not create an engine; calls=${RustTerminal.createCalls}" }
        check(RustTerminal.ioCalls.isEmpty()) { "adoption must not start IO: ${RustTerminal.ioCalls}" }
        check(adopted.getNativePointer() == pointer) { "adoption changed pointer" }
    }
    for (fd in listOf(-1, 0, 37)) {
        for (pointer in listOf(0L, 0x123456789ABCDEFL)) {
            for (transcript in listOf(null, 123)) {
                RustTerminal.clearRecording()
                RustTerminal.nextPtr = pointer
                // Named arguments preserve the existing Kotlin source constructor API.
                val created = TerminalEmulator(session = null, columns = 80, rows = 24,
                    cellWidthPixels = 8, cellHeightPixels = 16, transcriptRows = transcript,
                    ptyFd = fd, client = callback)
                check(RustTerminal.createCalls == 1)
                check(RustTerminal.createArgs == listOf(80, 24, 8, 16, transcript ?: 2000))
                check(created.getNativePointer() == pointer)
                val expected = if (fd != -1 && pointer != 0L) listOf(pointer to fd) else emptyList()
                check(RustTerminal.ioCalls == expected) { "creation IO ${RustTerminal.ioCalls}, expected $expected" }
            }
        }
    }
    // Both public JVM constructor descriptors remain present for Java callers.
    TerminalEmulator::class.java.getConstructor(TerminalOutput::class.java,
        Int::class.javaPrimitiveType, Int::class.javaPrimitiveType, Int::class.javaPrimitiveType,
        Int::class.javaPrimitiveType, Integer::class.java, Int::class.javaPrimitiveType,
        TerminalSessionClient::class.java)
    TerminalEmulator::class.java.getConstructor(TerminalSession::class.java,
        Long::class.javaPrimitiveType, Int::class.javaPrimitiveType, RustEngineCallback::class.java)
    println("PASS: 3 adoption cases, 12 creation cases, both public JVM constructor descriptors")
}
