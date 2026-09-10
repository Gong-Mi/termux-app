#!/usr/bin/env python3
"""Verify BootstrapState and BootstrapSecondStageRunner logic with Java compiler."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

fixture = r'''
package com.termux.app;

import java.io.File;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.concurrent.atomic.AtomicInteger;

public class BootstrapStateMachineProbe {
    static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }

    public static void main(String[] args) throws Exception {
        Path tempDir = Paths.get(args[0]);

        // 1. Test Initial State
        check(BootstrapState.getStage() == BootstrapState.Stage.IDLE, "Initial stage must be IDLE");
        check(!BootstrapState.isRunning(), "Must not be running initially");
        check(!BootstrapState.isReady(), "Must not be ready initially");

        // 2. Test Stage Transitions
        BootstrapState.setStage(BootstrapState.Stage.EXTRACT_ZIP);
        check(BootstrapState.isRunning(), "Must be running during EXTRACT_ZIP");
        check(BootstrapState.getStage() == BootstrapState.Stage.EXTRACT_ZIP, "Stage must be EXTRACT_ZIP");

        // 3. Test Callback Queueing and Dispatch
        final AtomicInteger callbackCount = new AtomicInteger(0);
        BootstrapState.addCallback(new Runnable() {
            public void run() { callbackCount.incrementAndGet(); }
        });
        BootstrapState.addCallback(new Runnable() {
            public void run() { callbackCount.incrementAndGet(); }
        });
        check(callbackCount.get() == 0, "Callbacks must not run before dispatch");

        BootstrapState.dispatchSuccess();
        check(BootstrapState.isReady(), "Stage must be READY after dispatchSuccess");
        check(!BootstrapState.isRunning(), "Must not be running when READY");
        check(callbackCount.get() == 2, "Both callbacks must have executed on dispatch");

        // 4. Test Immediate Callback on Ready
        final AtomicInteger immediateCount = new AtomicInteger(0);
        BootstrapState.addCallback(new Runnable() {
            public void run() { immediateCount.incrementAndGet(); }
        });
        check(immediateCount.get() == 1, "Callback added while READY must execute immediately");

        // 5. Test Reset and Failure
        BootstrapState.reset();
        check(BootstrapState.getStage() == BootstrapState.Stage.IDLE, "Stage must be IDLE after reset");

        BootstrapState.dispatchFailure(BootstrapState.Stage.SECOND_STAGE, "Mock exit code 127");
        check(BootstrapState.isFailed(), "Must be in FAILED state");
        check(BootstrapState.getFailedStage() == BootstrapState.Stage.SECOND_STAGE, "Failed stage must match");
        check("Mock exit code 127".equals(BootstrapState.getFailureReason()), "Failure reason must match");

        // 6. Test Script Path Resolution in BootstrapSecondStageRunner
        File fakePrefix = tempDir.resolve("fake_prefix").toFile();
        fakePrefix.mkdirs();
        check(BootstrapSecondStageRunner.resolveSecondStageScript(fakePrefix) == null, "Should return null when script missing");

        File primaryScript = new File(fakePrefix, BootstrapSecondStageRunner.RELATIVE_SECOND_STAGE_PATH);
        primaryScript.getParentFile().mkdirs();
        primaryScript.createNewFile();
        File resolved = BootstrapSecondStageRunner.resolveSecondStageScript(fakePrefix);
        check(resolved != null && resolved.equals(primaryScript), "Must resolve primary second stage path");

        BootstrapState.reset();
        System.out.println("PASS Bootstrap state machine and path resolution contracts");
    }
}
'''

stubs = {
    'android/content/Context.java': '''package android.content;
public class Context {
    public android.content.pm.ApplicationInfo getApplicationInfo() { return new android.content.pm.ApplicationInfo(); }
}''',
    'android/content/pm/ApplicationInfo.java': '''package android.content.pm;
public class ApplicationInfo { public String nativeLibraryDir = "/fake/lib"; }''',
    'android/os/Process.java': '''package android.os;
public class Process { public static boolean is64Bit() { return true; } }''',
    'com/termux/shared/logger/Logger.java': '''package com.termux.shared.logger;
public class Logger {
    public static int LOG_LEVEL_NORMAL = 0;
    public static void logInfo(String t, String m) {}
    public static void logWarn(String t, String m) {}
    public static void logError(String t, String m) {}
}''',
    'com/termux/shared/shell/command/ExecutionCommand.java': '''package com.termux.shared.shell.command;
public class ExecutionCommand {
    public static class Runner { public static Runner APP_SHELL = new Runner(); public String getName() { return "app"; } }
    public String commandLabel;
    public int backgroundCustomLogLevel;
    public ResultData resultData = new ResultData();
    public static class ResultData { public int exitCode = 0; public CharSequence stdout = ""; public CharSequence stderr = ""; }
    public ExecutionCommand(Integer id, String executable, String[] arguments, String stdin, String workingDirectory, String runner, boolean isFailsafe) {}
    public boolean isSuccessful() { return resultData.exitCode == 0; }
}''',
    'com/termux/shared/shell/command/runner/app/AppShell.java': '''package com.termux.shared.shell.command.runner.app;
import android.content.Context;
import com.termux.shared.shell.command.ExecutionCommand;
import java.util.HashMap;
public class AppShell {
    public static AppShell execute(Context c, ExecutionCommand cmd, Object cl, Object env, HashMap<String, String> add, boolean sync) { return new AppShell(); }
}''',
    'com/termux/shared/termux/shell/command/environment/TermuxShellEnvironment.java': '''package com.termux.shared.termux.shell.command.environment;
public class TermuxShellEnvironment {}''',
    'androidx/annotation/NonNull.java': '''package androidx.annotation;
import java.lang.annotation.*;
@Retention(RetentionPolicy.CLASS) public @interface NonNull {}''',
    'androidx/annotation/Nullable.java': '''package androidx.annotation;
import java.lang.annotation.*;
@Retention(RetentionPolicy.CLASS) public @interface Nullable {}'''
}

with tempfile.TemporaryDirectory(prefix='bootstrap-state-') as directory:
    root = Path(directory)
    test_file = root / 'BootstrapStateMachineProbe.java'
    test_file.write_text(fixture)

    stub_files = []
    for rel_path, content in stubs.items():
        file_path = root / rel_path
        file_path.parent.mkdir(parents=True, exist_ok=True)
        file_path.write_text(content)
        stub_files.append(str(file_path))

    state_file = ROOT / 'app/src/main/java/com/termux/app/BootstrapState.java'
    runner_file = ROOT / 'app/src/main/java/com/termux/app/BootstrapSecondStageRunner.java'

    cmd = ['javac', '-d', directory] + stub_files + [str(state_file), str(runner_file), str(test_file)]
    subprocess.run(cmd, check=True)
    subprocess.run(['java', '-cp', directory, 'com.termux.app.BootstrapStateMachineProbe', directory], check=True)
