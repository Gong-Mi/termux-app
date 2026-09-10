package com.termux.app;

import android.content.Context;
import android.os.Process;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import com.termux.shared.logger.Logger;
import com.termux.shared.shell.command.ExecutionCommand;
import com.termux.shared.shell.command.runner.app.AppShell;
import com.termux.shared.termux.shell.command.environment.TermuxShellEnvironment;

import java.io.File;
import java.util.HashMap;

/**
 * Isolated runner for Termux bootstrap second stage execution.
 * Decouples second stage environment resolution, process execution, and diagnostics.
 */
public final class BootstrapSecondStageRunner {

    private static final String LOG_TAG = "BootstrapSecondStage";

    public static final String RELATIVE_SECOND_STAGE_PATH =
        "etc/termux/termux-bootstrap/second-stage/termux-bootstrap-second-stage.sh";
    public static final String RELATIVE_LEGACY_SECOND_STAGE_PATH =
        "etc/termux/bootstrap/termux-bootstrap-second-stage.sh";
    public static final String RELATIVE_LOCK_FILE_PATH =
        "etc/termux/termux-bootstrap/second-stage/termux-bootstrap-second-stage.sh.lock";

    public static final class Result {
        public final boolean success;
        public final boolean skipped;
        public final int exitCode;
        public final String stdout;
        public final String stderr;
        public final String detail;

        private Result(boolean success, boolean skipped, int exitCode, String stdout, String stderr, String detail) {
            this.success = success;
            this.skipped = skipped;
            this.exitCode = exitCode;
            this.stdout = stdout != null ? stdout : "";
            this.stderr = stderr != null ? stderr : "";
            this.detail = detail != null ? detail : "";
        }

        public static Result success(int exitCode, String stdout, String stderr) {
            return new Result(true, false, exitCode, stdout, stderr, "Second stage completed successfully");
        }

        public static Result skipped(String reason) {
            return new Result(true, true, 0, "", "", reason);
        }

        public static Result failure(int exitCode, String stdout, String stderr, String detail) {
            return new Result(false, false, exitCode, stdout, stderr, detail);
        }

        @NonNull
        @Override
        public String toString() {
            return "SecondStageResult{success=" + success + ", skipped=" + skipped +
                ", exitCode=" + exitCode + ", detail='" + detail + "'}";
        }
    }

    private BootstrapSecondStageRunner() {}

    /**
     * Resolves the second stage script path within the given prefix directory.
     */
    @Nullable
    public static File resolveSecondStageScript(@NonNull File prefixDir) {
        File primary = new File(prefixDir, RELATIVE_SECOND_STAGE_PATH);
        if (primary.isFile()) {
            return primary;
        }
        File legacy = new File(prefixDir, RELATIVE_LEGACY_SECOND_STAGE_PATH);
        if (legacy.isFile()) {
            return legacy;
        }
        return null;
    }

    /**
     * Resolves the best available libtermux-exec library for LD_PRELOAD.
     */
    @Nullable
    public static File resolvePreloadLibrary(@NonNull Context context, @NonNull File prefixDir) {
        File primaryExec = new File(prefixDir, "lib/libtermux-exec.so");
        if (primaryExec.exists()) {
            return primaryExec;
        }
        if (context.getApplicationInfo() != null && context.getApplicationInfo().nativeLibraryDir != null) {
            File nativeExec = new File(context.getApplicationInfo().nativeLibraryDir, "libtermux-exec.so");
            if (nativeExec.exists()) {
                return nativeExec;
            }
        }
        File ldPreload = new File(prefixDir, "lib/libtermux-exec-ld-preload.so");
        if (ldPreload.exists()) {
            return ldPreload;
        }
        File linkerPreload = new File(prefixDir, "lib/libtermux-exec-linker-ld-preload.so");
        if (linkerPreload.exists()) {
            return linkerPreload;
        }
        return null;
    }

    /**
     * Executes the bootstrap second stage script synchronously.
     *
     * @param context Application/Activity context
     * @param prefixDir The root of $PREFIX (e.g. /data/data/com.termux/files/usr)
     * @return Result containing status, exit code, stdout/stderr, and diagnostic details.
     */
    @NonNull
    public static Result run(@NonNull Context context, @NonNull File prefixDir) {
        File secondStage = resolveSecondStageScript(prefixDir);
        if (secondStage == null) {
            Logger.logWarn(LOG_TAG, "Bootstrap second stage script not found in " + prefixDir + ", skipping.");
            return Result.skipped("Bootstrap second stage script not found");
        }

        File bash = new File(prefixDir, "bin/bash");
        if (!bash.isFile()) {
            String err = "Required shell interpreter missing: " + bash.getAbsolutePath();
            Logger.logError(LOG_TAG, err);
            return Result.failure(-1, "", "", err);
        }

        String linker = "/system/bin/linker" + (Process.is64Bit() ? "64" : "");
        File linkerFile = new File(linker);
        if (!linkerFile.exists()) {
            Logger.logWarn(LOG_TAG, "System linker not found at " + linker + ", falling back to direct bash execution");
            linker = bash.getAbsolutePath();
        }

        Logger.logInfo(LOG_TAG, "Executing bootstrap second stage: " + secondStage.getAbsolutePath());

        String[] arguments;
        if (linker.equals(bash.getAbsolutePath())) {
            arguments = new String[]{secondStage.getAbsolutePath()};
        } else {
            arguments = new String[]{bash.getAbsolutePath(), secondStage.getAbsolutePath()};
        }

        ExecutionCommand command = new ExecutionCommand(
            -1,
            linker,
            arguments,
            null,
            prefixDir.getAbsolutePath(),
            ExecutionCommand.Runner.APP_SHELL.getName(),
            false
        );
        command.commandLabel = "Termux Bootstrap Second Stage Command";
        command.backgroundCustomLogLevel = Logger.LOG_LEVEL_NORMAL;

        HashMap<String, String> additionalEnv = new HashMap<>();
        File preloadLib = resolvePreloadLibrary(context, prefixDir);
        if (preloadLib != null) {
            Logger.logInfo(LOG_TAG, "Injecting LD_PRELOAD: " + preloadLib.getAbsolutePath());
            additionalEnv.put("LD_PRELOAD", preloadLib.getAbsolutePath());
        } else {
            Logger.logWarn(LOG_TAG, "No libtermux-exec library found for LD_PRELOAD injection");
        }

        AppShell shell = AppShell.execute(context, command, null, new TermuxShellEnvironment(), additionalEnv, true);
        if (shell == null) {
            String err = "Failed to launch AppShell process for second stage: " + command;
            Logger.logError(LOG_TAG, err);
            return Result.failure(-2, "", "", err);
        }

        String stdout = command.resultData != null && command.resultData.stdout != null ?
            command.resultData.stdout.toString() : "";
        String stderr = command.resultData != null && command.resultData.stderr != null ?
            command.resultData.stderr.toString() : "";
        int exitCode = command.resultData != null ? command.resultData.exitCode : -3;

        if (!command.isSuccessful() || exitCode != 0) {
            String err = "Bootstrap second stage command failed with exitCode=" + exitCode +
                "\nstdout:\n" + stdout + "\nstderr:\n" + stderr;
            Logger.logError(LOG_TAG, err);
            return Result.failure(exitCode, stdout, stderr, err);
        }

        File lockFile = new File(prefixDir, RELATIVE_LOCK_FILE_PATH);
        if (!lockFile.exists()) {
            Logger.logWarn(LOG_TAG, "Second stage returned 0 but lock file not found at " + lockFile.getAbsolutePath());
        } else {
            Logger.logInfo(LOG_TAG, "Lock file confirmed at: " + lockFile.getAbsolutePath());
        }

        Logger.logInfo(LOG_TAG, "Bootstrap second stage completed successfully with exit code 0");
        return Result.success(exitCode, stdout, stderr);
    }
}
