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
    println("BOUNDARY: actual production Kotlin + actual JNI + real child shells; scheduling shim only, not ART or D2 completion")
}
