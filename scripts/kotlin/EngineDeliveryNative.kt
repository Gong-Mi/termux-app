package com.termux.terminal

/** Real production TerminalSession + JNI + child shell; queue shim is NOT ART. */
private fun waitFor(label: String, condition: () -> Boolean) {
    val deadline = System.nanoTime() + 15_000_000_000L
    while (!condition()) { check(System.nanoTime() < deadline) { label }; Thread.sleep(5) }
}
private fun id(session: TerminalSession): Int = TerminalSession::class.java.getDeclaredField("mNativeSessionId").apply { isAccessible = true }.getInt(session)
private fun callback(session: TerminalSession): RustEngineCallback = TerminalSession::class.java.getDeclaredField("mRustCallback").apply { isAccessible = true }.get(session) as RustEngineCallback
private class Client : TerminalSessionClient by RustEngineCallback(null) {
    var deliveries = 0
    var finished = 0
    var completionTranscript = ""
    var throwOnFinish = false
    var throwOnText = false
    var disposeOnFinish = false
    override fun onTextChanged(session: TerminalSession) { if (throwOnText) error("text callback failure") }
    override fun onSessionFinished(session: TerminalSession) {
        check(!session.isRunning)
        val emulator = checkNotNull(session.getEmulator())
        check(emulator.isAlive()) { "completion destroyed the display before result capture" }
        completionTranscript = RustTerminal.getTranscriptText(emulator.getNativePointer())
        finished++
        if (disposeOnFinish) session.dispose()
        if (throwOnFinish) error("completion client failure")
    }
    @Volatile var bells = 0
    var throwOnDelivery = false
    override fun setTerminalShellPid(session: TerminalSession, pid: Int) { check(pid > 0); deliveries++; if (throwOnDelivery) error("client notification failure") }
    override fun onBell(session: TerminalSession) { bells++ }
}
private fun session(client: Client): TerminalSession {
    val shell = if (java.io.File("/system/bin/sh").canExecute()) "/system/bin/sh" else "/bin/sh"
    return TerminalSession(shell, System.getProperty("user.dir"), arrayOf(shell, "-c", "printf 'delivery-ready\n'; IFS= read -r hold; printf '\\007'; IFS= read -r done"), arrayOf("PATH=/system/bin:/usr/bin:/bin", "LD_PRELOAD="), 2000, client)
}
fun main() {
    check(JNI.sNativeLibrariesLoaded)
    // Real native completion before any queued engine adoption is executed.
    val shell = if (java.io.File("/system/bin/sh").canExecute()) "/system/bin/sh" else "/bin/sh"
    val earlyClient = Client()
    val early = TerminalSession(shell, System.getProperty("user.dir"),
        arrayOf(shell, "-c", "printf 'completion-tail\n'; exit 37"),
        arrayOf("PATH=/system/bin:/usr/bin:/bin", "LD_PRELOAD="), 2000, earlyClient)
    early.initializeEmulator(80, 24, 8, 16)
    val earlyId = id(early)
    waitFor("real early completion bridge") { JNI.getCompletionDispatchStatus(earlyId) == 2 }
    check(early.getCompletionFacts() == TerminalSession.CompletionFacts(2, 37, 2, 0))
    check(!early.isEngineInitialized() && earlyClient.deliveries == 0)
    check(!callback(early).onSessionCompletion(earlyId, 2, 99, 2, 0))
    check(!callback(early).onSessionCompletion(earlyId + 1, 2, 99, 2, 0))
    early.mMainThreadHandler.drain()
    check(early.isEngineInitialized() && early.mEmulator!!.isAlive())
    check(earlyClient.finished == 1) { "early terminal facts were never delivered to the main-thread client" }
    check(!early.isRunning && early.getExitStatus() == 37)
    check(earlyClient.completionTranscript.contains("completion-tail"))
    check(earlyClient.completionTranscript.contains("[Process completed (code 37) - press Enter]"))
    check(early.getCompletionFacts() == TerminalSession.CompletionFacts(2, 37, 2, 0))
    early.dispose()
    check(!early.onNativeCompletion(earlyId, 2, 37, 2, 0))
    check(JNI.getCompletionDispatchStatus(earlyId) == -1)
    println("PASS: actual JNI early facts retained before adoption, duplicate/stale rejected, dispose does not resurrect")

    // Real JNI boolean rejection is observable, not a successful delivery.
    val unboundId = JNI.registerSession()
    val unbound = RustEngineCallback(null).apply { setNativeSessionId(unboundId) }
    JNI.createSessionAsync(unboundId, shell, requireNotNull(System.getProperty("user.dir")),
        arrayOf(shell, "-c", "exit 38"), arrayOf("PATH=/system/bin:/usr/bin:/bin", "LD_PRELOAD="),
        24, 80, 8, 16, 2000, unbound)
    waitFor("unbound bridge rejected") { JNI.getCompletionDispatchStatus(unboundId) == 3 }
    JNI.unregisterSession(unboundId)
    println("PASS: actual JNI receiver rejection records bridge failure")

    val client = Client()
    val s = session(client)
    s.initializeEmulator(80, 24, 8, 16)
    val originalId = id(s)
    s.initializeEmulator(80, 24, 8, 16)
    check(id(s) == originalId) { "duplicate initialization registered a second native owner" }
    waitFor("offer not posted") { s.mMainThreadHandler.queuedCount() > 0 }
    check(!s.isEngineInitialized() && s.mEmulator == null && client.deliveries == 0)
    check(JNI.getSessionProcessStatus(originalId)!![0] == 1)
    s.mMainThreadHandler.drain()
    check(s.isEngineInitialized() && client.deliveries == 1)
    val handle = s.mEmulator!!.getNativePointer()
    check(handle > 0 && RustTerminal.getRows(handle) == 24)
    check(JNI.claimEngineData(originalId, handle) == null)
    check(!JNI.ackEngineData(originalId, handle))
    check(!JNI.rejectEngineData(originalId, handle))
    check(RustTerminal.getRows(handle) == 24) // stale reject cannot destroy the acked owner
    callback(s).onEngineInitialized(handle, -1, s.getPid())
    s.mMainThreadHandler.drain()
    check(client.deliveries == 1 && s.mEmulator!!.getNativePointer() == handle)
    val replacement = Client()
    s.updateTerminalSessionClient(replacement)
    s.write(byteArrayOf(10), 0, 1)
    waitFor("replacement client did not receive real JNI bell") { replacement.bells == 1 }
    check(client.bells == 0 && replacement.bells == 1)
    s.dispose(); s.dispose()
    callback(s).onBell()
    check(replacement.bells == 1)
    check(JNI.getSessionProcessStatus(originalId) == null && RustTerminal.getRows(handle) == 0)
    s.initializeEmulator(90, 30, 8, 16); s.updateSize(90, 30, 8, 16)
    check(id(s) == -1 && !s.isEngineInitialized() && s.mEmulator == null)
    println("PASS: paused offer != READY; single claim/ack; stale reject/duplicate safe; replaceClient; adopted dispose/restart prevention")

    val competingClient = Client()
    val competing = session(competingClient)
    competing.initializeEmulator(80, 24, 8, 16)
    waitFor("competing offer") { competing.mMainThreadHandler.queuedCount() > 0 }
    val competingId = id(competing)
    val queuedOffer = competing.mMainThreadHandler.takeNext()!!
    // Read the real token captured by production Kotlin's Runnable; do not
    // manufacture native metadata or alter a native ownership flag.
    val tokenField = queuedOffer.javaClass.declaredFields.single { it.type == java.lang.Long.TYPE }
    tokenField.isAccessible = true
    val competingHandle = tokenField.getLong(queuedOffer)
    check(JNI.claimEngineData(competingId, competingHandle) != null)
    queuedOffer.run()
    check(JNI.ackEngineData(competingId, competingHandle)) { "failed duplicate claim revoked another claimant" }
    check(competingClient.deliveries == 0 && RustTerminal.getRows(competingHandle) == 24)
    RustTerminal.destroyEngine(competingHandle)
    competing.dispose()
    println("PASS: duplicate offer cannot revoke an already claimed token")

    val beforeDrainClient = Client()
    val beforeDrain = session(beforeDrainClient)
    beforeDrain.initializeEmulator(80, 24, 8, 16)
    val pendingId = id(beforeDrain)
    waitFor("pending offer") { beforeDrain.mMainThreadHandler.queuedCount() > 0 }
    check(!beforeDrain.isEngineInitialized() && beforeDrain.mEmulator == null)
    val pid = JNI.getSessionProcessStatus(pendingId)!![1]
    // Simulate a runnable dequeued by the UI just before concurrent disposal.
    val delayedOffer = beforeDrain.mMainThreadHandler.takeNext()!!
    beforeDrain.dispose(); beforeDrain.dispose()
    delayedOffer.run()
    beforeDrain.mMainThreadHandler.drain()
    check(beforeDrainClient.deliveries == 0 && !beforeDrain.isEngineInitialized())
    check(JNI.getSessionProcessStatus(pendingId) == null)
    if (java.io.File("/proc").exists()) waitFor("disposed pending child reaped") { !java.io.File("/proc/$pid").exists() }
    println("PASS: dispose-before-drain clears offers, unregisters native owner and reaps child; no resurrection")

    val rejectedClient = Client()
    val rejected = session(rejectedClient)
    rejected.mMainThreadHandler.setAccepting(false)
    rejected.initializeEmulator(80, 24, 8, 16)
    val rejectedId = id(rejected)
    waitFor("post rejection") { rejected.mMainThreadHandler.rejectedCount() > 0 }
    waitFor("rejected offer cleanup") { JNI.getSessionProcessStatus(rejectedId)?.get(0) in listOf(2, 3) }
    check(!rejected.isEngineInitialized() && rejectedClient.deliveries == 0)
    rejected.dispose()
    println("PASS: post(false) rejects native offer and terminates child, never delivers")

    val throwingClient = Client().apply { throwOnDelivery = true }
    val throwing = session(throwingClient)
    throwing.initializeEmulator(80, 24, 8, 16)
    waitFor("throwing client offer") { throwing.mMainThreadHandler.queuedCount() > 0 }
    check(runCatching { throwing.mMainThreadHandler.drain() }.exceptionOrNull()?.message == "client notification failure")
    check(throwing.isEngineInitialized() && throwingClient.deliveries == 1)
    val throwingHandle = throwing.mEmulator!!.getNativePointer()
    check(!JNI.rejectEngineData(id(throwing), throwingHandle))
    check(RustTerminal.getRows(throwingHandle) == 24)
    throwing.dispose()
    check(RustTerminal.getRows(throwingHandle) == 0)
    println("PASS: post-ack client exception cannot return ownership to native")
    val racing = session(Client())
    racing.initializeEmulator(80, 24, 8, 16)
    waitFor("racing offer") { racing.mMainThreadHandler.queuedCount() > 0 }
    racing.mMainThreadHandler.drain()
    val start = java.util.concurrent.CountDownLatch(1)
    val failure = java.util.concurrent.atomic.AtomicReference<Throwable?>()
    val reader = Thread {
        start.await()
        try {
            repeat(1000) {
                racing.getTitle()
                racing.write(byteArrayOf(), 0, 0)
                racing.mMainThreadHandler.handleMessage(android.os.Message().apply { what = 5 })
            }
        } catch (t: Throwable) { failure.set(t) }
    }
    reader.start()
    start.countDown()
    racing.dispose()
    reader.join()
    failure.get()?.let { throw it }
    check(racing.mEmulator == null && !racing.isEngineInitialized())
    println("PASS: concurrent read/input and dispose use stable wrapper snapshots")
    fun ready(client: Client): TerminalSession {
        val item = TerminalSession(shell, System.getProperty("user.dir"),
            arrayOf(shell, "-c", "IFS= read -r hold; printf 'ui-tail'; exit 0"),
            arrayOf("PATH=/system/bin:/usr/bin:/bin", "LD_PRELOAD="), 2000, client)
        item.initializeEmulator(80, 24, 8, 16)
        waitFor("ui offer") { item.mMainThreadHandler.queuedCount() > 0 }
        item.mMainThreadHandler.drain()
        check(item.isEngineInitialized() && item.getCompletionFacts() == null)
        return item
    }
    fun exit(item: TerminalSession) {
        item.write(byteArrayOf(10), 0, 1)
        waitFor("ui raw receipt") { JNI.getCompletionDispatchStatus(id(item)) == 2 }
    }
    val oldUi = Client()
    val switched = ready(oldUi)
    exit(switched)
    check(switched.getCompletionDeliveryState() == "POSTED" && oldUi.finished == 0)
    val newUi = Client()
    switched.updateTerminalSessionClient(newUi)
    switched.mMainThreadHandler.drain()
    check(oldUi.finished == 0 && newUi.finished == 1)
    check(switched.getCompletionDeliveryState() == "DELIVERED")
    check(switched.getProcessExitStatus() == 0 && switched.getCompletionError() == null)
    check(newUi.completionTranscript.indexOf("ui-tail") < newUi.completionTranscript.indexOf("[Process completed"))
    switched.mMainThreadHandler.drain()
    check(newUi.finished == 1)
    switched.dispose()
    println("PASS: completion posted != delivered; current client; tail before banner; retained display; once")

    val killedClient = Client()
    val killed = ready(killedClient)
    killed.finishIfRunning()
    waitFor("terminated session raw receipt") { JNI.getCompletionDispatchStatus(id(killed)) == 2 }
    killed.mMainThreadHandler.drain()
    check(killedClient.finished == 1 && killed.getProcessExitStatus() == -9)
    check(killedClient.completionTranscript.contains("signal 9"))
    check(killed.getEmulator()!!.isAlive())
    killed.dispose()
    println("PASS: real ProcessOwner kill reaches one completion with signal status and retained display")

    val noPostClient = Client()
    val noPost = ready(noPostClient)
    noPost.mMainThreadHandler.setAccepting(false)
    exit(noPost)
    check(noPost.getCompletionDeliveryState() == "FAILED" && noPostClient.finished == 0)
    check(noPost.mEmulator!!.isAlive())
    noPost.dispose()
    println("PASS: completion post(false) is FAILED, not delivered and not display disposal")

    val cancelledClient = Client()
    val cancelled = ready(cancelledClient)
    exit(cancelled)
    val queued = mutableListOf<Runnable>()
    while (true) { queued.add(cancelled.mMainThreadHandler.takeNext() ?: break) }
    cancelled.dispose()
    queued.forEach { it.run() }
    check(cancelledClient.finished == 0 && cancelled.getCompletionDeliveryState() == "CANCELLED")
    println("PASS: dequeued completion after dispose cannot notify or resurrect")

    for (throwText in listOf(false, true)) {
        val brokenClient = Client()
        val broken = ready(brokenClient)
        exit(broken)
        // Remove screen messages without executing foreign callbacks. The completion
        // runnable itself isolates both text and finished callbacks.
        brokenClient.throwOnFinish = !throwText
        // Text exceptions in ordinary screen messages are inherited; only directly
        // invoke completion's queued runnable after identifying its captured sessionId.
        val all = mutableListOf<Runnable>()
        while (true) { all.add(broken.mMainThreadHandler.takeNext() ?: break) }
        all.filter { runnable -> runnable.javaClass.declaredFields.any { it.type == Integer.TYPE } }
            .also { check(it.size == 1) }.forEach { runnable ->
                brokenClient.throwOnText = throwText
                runnable.run()
            }
        check(brokenClient.finished == 1 && broken.getCompletionDeliveryState() == "FAILED")
        check(broken.mEmulator!!.isAlive())
        broken.dispose()
    }
    println("PASS: throwing text/finished clients record failure without duplicate or premature destroy")

    val resultClient = Client().apply { disposeOnFinish = true }
    val result = ready(resultClient)
    exit(result)
    result.mMainThreadHandler.drain()
    check(resultClient.finished == 1 && resultClient.completionTranscript.contains("ui-tail"))
    check(result.getEmulator() == null && result.getCompletionDeliveryState() == "DELIVERED")
    println("PASS: synchronous result capture precedes callback-driven dispose")

    // Inject only raw failure facts at the production receiver boundary. These are
    // projection tests, not claims of a real kernel IO error or external reaper.
    for ((pk, pc, ik, ic) in listOf(listOf(2, 0, 3, 0), listOf(2, 0, 4, 5),
        listOf(2, 0, 5, 0), listOf(2, 0, 6, 0), listOf(3, 10, 2, 0))) {
        val errorClient = Client()
        val item = ready(errorClient)
        check(item.onNativeCompletion(id(item), pk, pc, ik, ic))
        item.mMainThreadHandler.drain()
        check(errorClient.finished == 1 && item.getCompletionError() != null)
        check(item.getProcessExitStatus() == if (pk == 2) pc else null)
        check(item.getEmulator()!!.isAlive())
        item.dispose()
    }
    println("PASS: injected cancelled/error/overflow/panic/lost preserve error and nullable actual process status")
    println("BOUNDARY: actual Kotlin/JNI/shell completion and queue shim; error projection injected; not ART/Service/GPU present")
}
