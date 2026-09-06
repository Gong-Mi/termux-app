package com.termux.app;

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.ServiceConnection;
import android.os.Bundle;
import android.os.IBinder;
import android.os.Looper;
import android.os.SystemClock;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import com.termux.shared.shell.command.ExecutionCommand;
import com.termux.shared.shell.command.environment.IShellEnvironment;
import com.termux.shared.termux.shell.command.runner.terminal.TermuxSession;
import com.termux.shared.termux.terminal.TermuxTerminalSessionClientBase;
import com.termux.terminal.TerminalSession;
import com.termux.terminal.RustTerminal;
import java.util.HashMap;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.BooleanSupplier;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.*;

/** Actual target APK, ART, Handler/Looper, JNI, PTY and Service. No queue/dependency shims. */
@RunWith(AndroidJUnit4.class)
public class SessionCompletionArtTest {
    private static Context context() { return InstrumentationRegistry.getInstrumentation().getTargetContext(); }
    private static void main(Runnable runnable) { InstrumentationRegistry.getInstrumentation().runOnMainSync(runnable); }
    private static void until(String label, BooleanSupplier ready) {
        long deadline = SystemClock.uptimeMillis() + 20000;
        while (!ready.getAsBoolean()) {
            if (SystemClock.uptimeMillis() >= deadline) fail(label);
            SystemClock.sleep(20);
        }
    }
    private static void await(CountDownLatch done) throws Exception { assertTrue("completion timeout", done.await(20, TimeUnit.SECONDS)); }
    private static void rethrow(AtomicReference<Throwable> error) { if (error.get() != null) throw new AssertionError(error.get()); }
    private static final class Probe extends TermuxTerminalSessionClientBase {
        final CountDownLatch done = new CountDownLatch(1);
        final AtomicReference<Throwable> error = new AtomicReference<>();
        final AtomicInteger calls = new AtomicInteger();
        volatile String transcript;
        Runnable result;
        @Override public void onSessionFinished(TerminalSession session) {
            try {
                assertSame(Looper.getMainLooper(), Looper.myLooper());
                assertFalse(session.isRunning());
                assertNotNull(session.getEmulator());
                assertTrue(session.getEmulator().isAlive());
                transcript = RustTerminal.getTranscriptText(session.getEmulator().getNativePointer());
                calls.incrementAndGet();
                if (result != null) result.run();
            } catch (Throwable failure) { error.set(failure); }
            finally { done.countDown(); }
        }
    }
    private static TerminalSession terminal(String script, Probe probe) {
        return new TerminalSession("/system/bin/sh", context().getFilesDir().getAbsolutePath(),
            new String[]{"sh", "-c", script}, new String[]{"PATH=/system/bin", "LD_PRELOAD="}, 2000, probe);
    }

    @Test public void earlyExitDeliversOnRealMainLooperAndRetainsTranscript() throws Exception {
        Probe probe = new Probe();
        AtomicReference<TerminalSession> holder = new AtomicReference<>();
        main(() -> {
            TerminalSession session = terminal("/system/bin/toybox printf 'art-tail\\n' || exit 91; exit 37", probe);
            holder.set(session);
            session.initializeEmulator(80, 24, 8, 16);
            // Keep this real main-thread task active while native producers finish;
            // the queued adoption cannot run until we return to the actual Looper.
            until("raw facts before main-thread adoption", () -> session.getCompletionFacts() != null);
            assertFalse(session.isEngineInitialized());
        });
        TerminalSession session = holder.get();
        try {
            await(probe.done); rethrow(probe.error);
            until("delivery state", () -> "DELIVERED".equals(session.getCompletionDeliveryState()));
            assertEquals(Integer.valueOf(37), session.getProcessExitStatus());
            assertNull(session.getCompletionError());
            assertEquals(1, probe.calls.get());
            assertTrue("actual transcript=" + probe.transcript, probe.transcript.contains("art-tail"));
            assertTrue(probe.transcript.contains("[Process completed (code 37) - press Enter]"));
            assertTrue(session.getEmulator().isAlive());
            main(() -> session.updateSize(90, 30, 8, 16));
            assertTrue(RustTerminal.getTranscriptText(session.getEmulator().getNativePointer()).contains("art-tail"));
        } finally { main(session::dispose); }
        assertNull(session.getEmulator());
    }

    private static final IShellEnvironment SYSTEM = new IShellEnvironment() {
        public String getDefaultWorkingDirectoryPath() { return context().getFilesDir().getAbsolutePath(); }
        public String getDefaultBinPath() { return "/system/bin"; }
        public String[] setupShellCommandArguments(String file, String[] args) {
            String[] result = new String[args.length + 1]; result[0] = file;
            System.arraycopy(args, 0, result, 1, args.length); return result;
        }
        public HashMap<String, String> setupShellCommandEnvironment(Context c, ExecutionCommand command) {
            HashMap<String, String> env = new HashMap<>(); env.put("PATH", "/system/bin"); env.put("LD_PRELOAD", ""); return env;
        }
    };

    @Test public void actualExecutionCommandCapturesBeforeResultCallbackDisposes() throws Exception {
        Probe probe = new Probe();
        AtomicReference<TermuxSession> holder = new AtomicReference<>();
        ExecutionCommand command = new ExecutionCommand(901, "/system/bin/sh",
            new String[]{"-c", "/system/bin/toybox printf 'result-tail\\n' || exit 91; exit 0"}, null,
            context().getFilesDir().getAbsolutePath(), "terminal-session", true);
        AtomicInteger results = new AtomicInteger();
        main(() -> {
            TermuxSession wrapper = TermuxSession.execute(context(), command, probe, completed -> {
                assertSame(Looper.getMainLooper(), Looper.myLooper());
                assertEquals(Integer.valueOf(0), command.resultData.exitCode);
                assertFalse(command.isStateFailed());
                assertTrue("actual stdout=" + command.resultData.stdout, command.resultData.stdout.toString().contains("result-tail"));
                assertTrue(completed.getTerminalSession().getEmulator().isAlive());
                results.incrementAndGet();
                completed.getTerminalSession().dispose();
            }, SYSTEM, null, true);
            assertNotNull(wrapper); holder.set(wrapper); probe.result = wrapper::finish;
            wrapper.getTerminalSession().initializeEmulator(80, 24, 8, 16);
        });
        TerminalSession session = holder.get().getTerminalSession();
        try {
            await(probe.done); rethrow(probe.error);
            until("result completion state", () -> "DELIVERED".equals(session.getCompletionDeliveryState()));
            assertEquals(1, results.get()); assertEquals(1, probe.calls.get());
            assertNull(session.getEmulator());
        } finally { main(session::dispose); }
    }

    @Test public void serviceOnlyPluginDeliversPendingIntentThenRemovesSession() throws Exception {
        Context context = context();
        AtomicReference<TermuxService> service = new AtomicReference<>();
        CountDownLatch bound = new CountDownLatch(1), received = new CountDownLatch(1);
        AtomicReference<Intent> result = new AtomicReference<>();
        ServiceConnection connection = new ServiceConnection() {
            public void onServiceConnected(ComponentName name, IBinder binder) {
                service.set(((TermuxService.LocalBinder) binder).service); bound.countDown();
            }
            public void onServiceDisconnected(ComponentName name) { }
        };
        String action = context.getPackageName() + ".ART_COMPLETION_RESULT";
        BroadcastReceiver receiver = new BroadcastReceiver() {
            @Override public void onReceive(Context c, Intent intent) { result.set(intent); received.countDown(); }
        };
        context.registerReceiver(receiver, new IntentFilter(action), Context.RECEIVER_NOT_EXPORTED);
        PendingIntent pending = PendingIntent.getBroadcast(context, 902,
            new Intent(action).setPackage(context.getPackageName()), PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
        AtomicReference<TermuxSession> holder = new AtomicReference<>();
        boolean didBind = false;
        try {
            didBind = context.bindService(new Intent(context, TermuxService.class), connection, Context.BIND_AUTO_CREATE);
            assertTrue(didBind); await(bound);
            ExecutionCommand command = new ExecutionCommand(902, "/system/bin/sh",
                new String[]{"-c", "/system/bin/toybox printf 'plugin-art-tail\\n' || exit 91; exit 0"}, null,
                context.getFilesDir().getAbsolutePath(), "terminal-session", true);
            command.isPluginExecutionCommand = true;
            command.resultConfig.resultPendingIntent = pending;
            main(() -> {
                // Instrumentation starts with no Activity attached; select the real Service client.
                service.get().unsetTermuxTerminalSessionClient();
                TermuxSession wrapper = service.get().createTermuxSession(command);
                assertNotNull(wrapper); holder.set(wrapper);
                wrapper.getTerminalSession().initializeEmulator(80, 24, 8, 16);
            });
            await(received);
            Bundle bundle = result.get().getBundleExtra(command.resultConfig.resultBundleKey);
            assertNotNull(bundle);
            assertTrue("actual plugin stdout=" + bundle.getString(command.resultConfig.resultStdoutKey),
                bundle.getString(command.resultConfig.resultStdoutKey).contains("plugin-art-tail"));
            assertEquals(0, bundle.getInt(command.resultConfig.resultExitCodeKey));
            until("service removed completed plugin", () -> holder.get().getTerminalSession().getEmulator() == null);
            main(() -> assertEquals(-1, service.get().getIndexOfSession(holder.get().getTerminalSession())));
            assertFalse(command.isStateFailed());
        } finally {
            if (holder.get() != null) main(() -> holder.get().getTerminalSession().dispose());
            if (didBind) context.unbindService(connection);
            context.unregisterReceiver(receiver); pending.cancel();
        }
    }
    @Test public void callerEnvironmentRemainsIntactInArt() throws Exception {
        HashMap<String, String> generated = new com.termux.shared.termux.shell.command.environment.TermuxShellEnvironment()
            .getEnvironment(context(), true);
        assertEquals(context().getApplicationInfo().dataDir, generated.get("TERMUX_APP__DATA_DIR"));
        assertEquals("/data/data/" + context().getPackageName(), generated.get("TERMUX_APP__LEGACY_DATA_DIR"));
        assertEquals(generated.get("PREFIX"), generated.get("TERMUX__PREFIX"));
        Probe probe = new Probe();
        AtomicReference<TerminalSession> holder = new AtomicReference<>();
        main(() -> {
            TerminalSession session = terminal("[ \"$PATH\" = /system/bin ] || exit 71; " +
                "[ \"${LD_LIBRARY_PATH+x}\" != x ] || exit 72; " +
                "/system/bin/toybox printf 'environment-intact\\n' || exit 91", probe);
            holder.set(session); session.initializeEmulator(80, 24, 8, 16);
        });
        TerminalSession session = holder.get();
        try {
            await(probe.done); rethrow(probe.error);
            assertEquals("transcript=" + probe.transcript, Integer.valueOf(0), session.getProcessExitStatus());
            assertTrue(probe.transcript.contains("environment-intact"));
        } finally { main(session::dispose); }
    }

    @Test public void privateCommandsAndNestedShellsExecuteInArt() throws Exception {
        String prefix = context().getFilesDir().getAbsolutePath() + "/usr";
        // Fail, do not skip: the bootstrap is part of this app's execution contract.
        for (String path : new String[]{"lib/libtermux-exec.so", "bin/printf", "bin/env", "bin/bash", "bin/apt", "bin/dpkg"})
            assertTrue("bootstrap dependency missing: " + path, new java.io.File(prefix, path).isFile());
        Probe probe = new Probe();
        AtomicReference<TerminalSession> holder = new AtomicReference<>();
        String script = "[ -n \"$LD_PRELOAD\" ] || exit 81; " +
            "echo \"exec-preload=$LD_PRELOAD\"; " +
            "/system/bin/toybox grep termux-exec /proc/$$/maps || exit 88; " +
            "\"$PREFIX/bin/printf\" 'ecosystem-direct\\n' || exit 82; " +
            "/system/bin/sh -c '\"$PREFIX/bin/printf\" \"ecosystem-nested\\n\"' || exit 83; " +
            "\"$PREFIX/bin/env\" /system/bin/sh -c '\"$PREFIX/bin/printf\" \"ecosystem-env\\n\"' || exit 84; " +
            "\"$PREFIX/bin/bash\" --noprofile --norc -c '\"$PREFIX/bin/printf\" \"ecosystem-bash\\n\"' || exit 85; " +
            "\"$PREFIX/bin/apt\" --version >/dev/null || exit 86; " +
            "\"$PREFIX/bin/dpkg\" --version >/dev/null || exit 87; " +
            "/system/bin/toybox printf 'ecosystem-tools-ok\\n'";
        main(() -> {
            TerminalSession session = new TerminalSession("/system/bin/sh", context().getFilesDir().getAbsolutePath(),
                new String[]{"sh", "-c", script}, new String[]{"PREFIX=" + prefix,
                "PATH=" + prefix + "/bin:/system/bin", "LD_PRELOAD="}, 2000, probe);
            holder.set(session); session.initializeEmulator(100, 30, 8, 16);
        });
        TerminalSession session = holder.get();
        try {
            await(probe.done); rethrow(probe.error);
            assertEquals("transcript=" + probe.transcript, Integer.valueOf(0), session.getProcessExitStatus());
            for (String marker : new String[]{"ecosystem-direct", "ecosystem-nested", "ecosystem-env", "ecosystem-bash", "ecosystem-tools-ok"})
                assertTrue("missing " + marker + "; transcript=" + probe.transcript, probe.transcript.contains(marker));
        } finally { main(session::dispose); }
    }

}
