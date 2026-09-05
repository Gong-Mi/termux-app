package com.termux.terminal

import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicReference

/** Real production Kotlin/JNI. No Surface/ART/PTY acceptance is claimed here. */
fun main() {
    check(JNI.sNativeLibrariesLoaded) { "Engine handle tests require the real JNI library" }
    val callback = RustEngineCallback(null)
    val handle = RustTerminal.createEngine(80, 24, 8, 16, 2000, callback)
    check(handle > 0)
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
        val shell = if (java.io.File("/system/bin/sh").canExecute()) "/system/bin/sh" else "/bin/sh"
        val command = "IFS= read -r value; printf '\\nreply:%s\\n' \"${'$'}value\""
        val create = JNI::class.java.declaredMethods.single { it.name == "createSessionAsync" }
        create.invoke(null, sessionId, shell, System.getProperty("user.dir"),
            arrayOf(shell, "-c", command), arrayOf("PATH=/system/bin:/usr/bin:/bin", "LD_PRELOAD="),
            24, 80, 8, 16, 2000, null)
        val deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(15)
        var data: LongArray? = null
        while (data == null && System.nanoTime() < deadline) {
            data = JNI.pollEngineData(sessionId)
            if (data == null) Thread.sleep(10)
        }
        check(data != null) { "async polling ownership was not delivered" }
        ptyHandle = data!![0]
        check(ptyHandle > 0 && RustTerminal.getCols(ptyHandle) == 80)
        check(JNI.pollEngineData(sessionId) == null) { "poll ownership delivered twice" }
        val input = "skipPAYLOAD\nignored".toByteArray(Charsets.UTF_8)
        RustTerminal.processInput(ptyHandle, input, 4, 8)
        var transcript = ""
        while (!transcript.contains("reply:PAYLOAD") && System.nanoTime() < deadline) {
            transcript = RustTerminal.getTranscriptText(ptyHandle)
            if (!transcript.contains("reply:PAYLOAD")) Thread.sleep(10)
        }
        check(transcript.contains("reply:PAYLOAD")) { "PTY offset/count/reader failed: $transcript" }
        check(!transcript.contains("skip") && !transcript.contains("ignored"))
    } finally {
        if (ptyHandle != 0L) RustTerminal.destroyEngine(ptyHandle)
        JNI.unregisterSession(sessionId)
    }
    println("PASS: ${methods.size} native handle methods; invalid/revoked/double-destroy, unowned callback, concurrent JNI smoke, real async PTY input/reader/poll")
}
