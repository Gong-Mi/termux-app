package com.termux.terminal

import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicReference

/** Real production Kotlin/JNI and PTY children. No Surface/ART/UI completion claim. */
private fun awaitCondition(label: String, predicate: () -> Boolean) {
    val deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(15)
    while (!predicate()) {
        check(System.nanoTime() < deadline) { "Timed out: $label" }
        Thread.sleep(10)
    }
}

private fun assertExited(sessionId: Int, handle: Long, code: Int, retained: String? = null) {
    var status: IntArray? = null
    awaitCondition("session $sessionId exit $code; status=${JNI.getSessionProcessStatus(sessionId)?.contentToString()}") {
        status = JNI.getSessionProcessStatus(sessionId)
        status?.get(0) in listOf(2, 3)
    }
    val expected = intArrayOf(2, -1, code)
    check(status!!.contentEquals(expected)) { "exit status: ${status!!.contentToString()}" }
    // A parsed marker proves the slave opened. Pending termination may happen
    // before first slave open, so process exit alone must not manufacture EOF.
    if (retained != null) {
        awaitCondition("IO EOF for session $sessionId") {
            RustTerminal.getCompletionStatus(handle)?.get(2) == 2
        }
        check(RustTerminal.getCompletionStatus(handle)!!.contentEquals(intArrayOf(2, code, 2, 0)))
    }
    val observed = RustTerminal.getCompletionStatus(handle)!!
    check(observed[0] == 2 && observed[1] == code)
    println("PASS: independent process/IO observation=${observed.contentToString()}; not UI completion")
    // Exit does not revoke the engine or clear text already parsed by the reader.
    // This is not a claim about EOF, output drain or UI callback delivery.
    repeat(3) {
        check(!JNI.terminateSession(sessionId)) { "terminal session retained signal authority" }
        check(JNI.getSessionProcessStatus(sessionId)!!.contentEquals(expected))
        check(RustTerminal.tryProcessInput(handle, byteArrayOf(65), 0, 1) == RustTerminal.INPUT_CLOSED)
        check(RustTerminal.getRows(handle) > 0) { "process exit disposed the engine" }
        if (retained != null) check(RustTerminal.getTranscriptText(handle).contains(retained))
    }
    println("PASS: process-owner session=$sessionId status=${status!!.contentToString()}; repeated terminate=false, input=CLOSED, engine retained")
}

private fun createPollingShell(sessionId: Int, command: String) {
    val shell = if (java.io.File("/system/bin/sh").canExecute()) "/system/bin/sh" else "/bin/sh"
    // Reflection bypasses only the Kotlin callback non-null annotation: null
    // selects the real production native polling handoff, never a mocked JNI.
    JNI::class.java.declaredMethods.single { it.name == "createSessionAsync" }.invoke(
        null, sessionId, shell, System.getProperty("user.dir"), arrayOf(shell, "-c", command),
        arrayOf("PATH=/system/bin:/usr/bin:/bin", "LD_PRELOAD="), 24, 80, 8, 16, 2000, null)
}

private fun pollHandle(sessionId: Int): LongArray {
    var data: LongArray? = null
    awaitCondition("async polling ownership for $sessionId") {
        data = JNI.pollEngineData(sessionId)
        data != null
    }
    check(data!!.size == 3 && data!![0] > 0 && data!![2] > 0)
    check(JNI.pollEngineData(sessionId) == null) { "poll ownership delivered twice" }
    return data!!
}

fun main() {
    check(JNI.sNativeLibrariesLoaded) { "Engine handle tests require the real JNI library" }
    val callback = RustEngineCallback(null)
    val handle = RustTerminal.createEngine(80, 24, 8, 16, 2000, callback)
    check(handle > 0)
    check(RustTerminal.getCompletionStatus(handle)!!.contentEquals(intArrayOf(0, 0, 0, 0)))
    val text = "lease-中文".toByteArray(Charsets.UTF_8)
    RustTerminal.processBatch(handle, text, text.size)
    check(RustTerminal.getTranscriptText(handle).contains("lease-中文"))
    RustTerminal.destroyEngine(handle)
    RustTerminal.destroyEngine(handle)

    // Cover every native RustTerminal method accepting an engine handle. Revoked
    // and arbitrary tokens must have the same neutral result as the existing 0
    // contract; no random address is ever dereferenced by the new implementation.
    val methods = RustTerminal::class.java.declaredMethods.filter {
        java.lang.reflect.Modifier.isNative(it.modifiers) &&
            it.parameterTypes.firstOrNull() == java.lang.Long.TYPE
    }.sortedBy { it.name }
    check(methods.isNotEmpty())
    fun arguments(types: Array<Class<*>>, token: Long): Array<Any?> =
        types.mapIndexed { index, type ->
            when {
                index == 0 -> token
                type == java.lang.Integer.TYPE -> 0
                type == java.lang.Boolean.TYPE -> false
                type == ByteArray::class.java -> byteArrayOf(65)
                type == IntArray::class.java -> intArrayOf(123)
                type == LongArray::class.java -> longArrayOf(456)
                type == String::class.java -> "x"
                type == java.util.Properties::class.java -> java.util.Properties()
                else -> null
            }
        }.toTypedArray()
    for (method in methods) {
        val expected = method.invoke(null, *arguments(method.parameterTypes, 0))
        for (token in listOf(handle, -1L, Long.MAX_VALUE)) {
            val args = arguments(method.parameterTypes, token)
            val actual = method.invoke(null, *args)
            check(actual == expected) { "${method.name}: stale token result $actual != zero $expected" }
            for (arg in args.drop(1)) {
                if (arg is IntArray) check(arg.contentEquals(intArrayOf(123)))
                if (arg is LongArray) check(arg.contentEquals(longArrayOf(456)))
            }
        }
    }

    // A callback without a session is not an ownership recipient.
    val rejected = RustTerminal.createEngine(20, 10, 8, 16, 100, callback)
    callback.onEngineInitialized(rejected, -1, -1)
    check(RustTerminal.getRows(rejected) == 0)

    val concurrent = RustTerminal.createEngine(80, 24, 8, 16, 2000, callback)
    check(concurrent > handle && concurrent != rejected)
    val ready = CountDownLatch(4)
    val start = CountDownLatch(1)
    val failure = AtomicReference<Throwable?>(null)
    val threads = (0 until 4).map { worker ->
        Thread {
            try {
                ready.countDown()
                start.await()
                repeat(400) {
                    if (worker % 2 == 0) {
                        RustTerminal.processBatch(concurrent, byteArrayOf(120), 1)
                    } else {
                        check(RustTerminal.getCols(concurrent) in listOf(0, 80))
                    }
                }
            } catch (t: Throwable) {
                failure.compareAndSet(null, t)
            }
        }.apply { start() }
    }
    ready.await()
    start.countDown()
    RustTerminal.destroyEngine(concurrent)
    threads.forEach { it.join() }
    failure.get()?.let { throw it }
    check(RustTerminal.getRows(concurrent) == 0)
    val independent = RustTerminal.createEngine(33, 12, 8, 16, 100, callback)
    RustTerminal.destroyEngine(concurrent)
    check(RustTerminal.getCols(independent) == 33)
    RustTerminal.destroyEngine(independent)
    // Exercise actual async creation, polling handoff, reader parsing and a
    // live processInput offset/count write. Null selects native polling mode;
    // reflection bypasses only Kotlin's non-null source annotation, not JNI.
    val sessionId = JNI.registerSession()
    var ptyHandle = 0L
    try {
        check(JNI.getSessionProcessStatus(sessionId)!!.contentEquals(intArrayOf(0, 0, 0)))
        val command = "IFS= read -r value; stty size; printf '\\nreply:%s\\n' \"${'$'}value\"; IFS= read -r finish; exit 37"
        createPollingShell(sessionId, command)
        val data = pollHandle(sessionId)
        ptyHandle = data[0]
        check(RustTerminal.getCols(ptyHandle) == 80)
        check(JNI.getSessionProcessStatus(sessionId)!!.contentEquals(intArrayOf(1, data[2].toInt(), 0)))
        val deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(15)
        RustTerminal.resize(ptyHandle, 81, 25, 8, 16)
        check(RustTerminal.tryProcessInput(ptyHandle, byteArrayOf(1), -1, 1) == RustTerminal.INPUT_INVALID)
        check(RustTerminal.tryProcessInput(ptyHandle, byteArrayOf(1), Int.MAX_VALUE, 1) == RustTerminal.INPUT_INVALID)
        check(RustTerminal.tryProcessInput(ptyHandle, ByteArray(1024 * 1024 + 1), 0, 1024 * 1024 + 1) == RustTerminal.INPUT_FULL)
        check(RustTerminal.tryProcessInput(ptyHandle, byteArrayOf(), 0, 0) == RustTerminal.INPUT_ACCEPTED)
        val input = "skipPAYLOAD\nignored".toByteArray(Charsets.UTF_8)
        check(RustTerminal.tryProcessInput(ptyHandle, input, 4, 3) == RustTerminal.INPUT_ACCEPTED)
        RustTerminal.processInput(ptyHandle, input, 7, 5)
        var transcript = ""
        while (!transcript.contains("reply:PAYLOAD") && System.nanoTime() < deadline) {
            transcript = RustTerminal.getTranscriptText(ptyHandle)
            if (!transcript.contains("reply:PAYLOAD")) Thread.sleep(10)
        }
        check(transcript.contains("reply:PAYLOAD")) { "PTY offset/count/reader failed: $transcript" }
        check(!transcript.contains("skip") && !transcript.contains("ignored"))
        check(transcript.contains("25 81")) { "handle-based PTY resize was not applied: $transcript" }
        // Hold the shell at a second read until the native reader has parsed the
        // marker. Retention assertions never assume that process exit drains IO.
        check(RustTerminal.tryProcessInput(ptyHandle, byteArrayOf(10), 0, 1) == RustTerminal.INPUT_ACCEPTED)
        assertExited(sessionId, ptyHandle, 37, "reply:PAYLOAD")
    } finally {
        JNI.terminateSession(sessionId)
        if (ptyHandle != 0L) RustTerminal.destroyEngine(ptyHandle)
        JNI.unregisterSession(sessionId)
    }
    check(JNI.getSessionProcessStatus(sessionId) == null)
    check(!JNI.terminateSession(sessionId))

    for (pending in listOf(true, false)) {
        val id = JNI.registerSession()
        var engine = 0L
        try {
            check(JNI.getSessionProcessStatus(id)!!.contentEquals(intArrayOf(0, 0, 0)))
            if (pending) {
                // Deterministically before create/bind, not a scheduler race.
                check(JNI.terminateSession(id))
                check(JNI.getSessionProcessStatus(id)!!.contentEquals(intArrayOf(0, 0, 0)))
            }
            createPollingShell(id, "printf 'owner-ready\\n'; IFS= read -r hold; exit 99")
            val data = pollHandle(id)
            engine = data[0]
            if (!pending) {
                awaitCondition("live shell output parsed") {
                    RustTerminal.getTranscriptText(engine).contains("owner-ready")
                }
                check(JNI.getSessionProcessStatus(id)!!.contentEquals(intArrayOf(1, data[2].toInt(), 0)))
                check(JNI.terminateSession(id)) { "live owner termination rejected" }
            }
            assertExited(id, engine, -9, if (pending) null else "owner-ready")
            println("PASS: ${if (pending) "pre-bind pending" else "live"} terminate applied to real async shell")
        } finally {
            JNI.terminateSession(id)
            if (engine != 0L) RustTerminal.destroyEngine(engine)
            JNI.unregisterSession(id)
        }
        check(JNI.getSessionProcessStatus(id) == null)
        check(!JNI.terminateSession(id))
    }
    check(JNI.getSessionProcessStatus(-1) == null)
    check(!JNI.terminateSession(-1))
    println("PASS: ${methods.size} native handle methods; invalid/revoked/double-destroy, unowned callback, concurrent JNI smoke, real async PTY input/reader/poll; process-owner normal exit, pending/live termination, cached terminal status/input rejection/parsed text retention")
}
