package com.termux.app;

import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Build;

import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;
import java.util.Arrays;
import java.util.List;

/**
 * Collects diagnostic information about Termux initialization state and environment.
 * Used by the "Export debug logs" feature.
 */
public final class TermuxLogCollector {

    private TermuxLogCollector() {
    }

    public static String collect(Context context) {
        StringBuilder sb = new StringBuilder();
        sb.append("===== Termux Debug Report =====\n");
        sb.append("Generated: ").append(java.text.DateFormat.getDateTimeInstance().format(new java.util.Date())).append("\n\n");

        // App & System Info
        sb.append("----- App & System -----\n");
        sb.append("App Version: ").append(getAppVersion(context)).append("\n");
        sb.append("Package: ").append(context.getPackageName()).append("\n");
        sb.append("Android Version: ").append(Build.VERSION.RELEASE).append(" (API ").append(Build.VERSION.SDK_INT).append(")\n");
        sb.append("Device: ").append(Build.MANUFACTURER).append(" ").append(Build.MODEL).append("\n");
        sb.append("ABI: ").append(Build.SUPPORTED_ABIS != null ? Arrays.toString(Build.SUPPORTED_ABIS) : "unknown").append("\n");
        sb.append("User: ").append(android.os.Process.myUid()).append("\n\n");

        // Termux Paths
        sb.append("----- Termux Paths -----\n");
        sb.append("FILES_PATH: ").append(TermuxConstants.FILES_PATH).append("\n");
        sb.append("PREFIX_PATH: ").append(TermuxConstants.PREFIX_PATH).append("\n");
        sb.append("HOME_PATH: ").append(TermuxConstants.HOME_PATH).append("\n");
        sb.append("StagingPrefix: ").append(TermuxConstants.FILES_PATH).append("/usr-staging\n");
        sb.append("Data dir exists: ").append(context.getFilesDir().exists()).append("\n");
        sb.append("Data dir path: ").append(context.getFilesDir().getAbsolutePath()).append("\n\n");

        // Environment
        sb.append("----- Environment -----\n");
        sb.append("LD_PRELOAD: ").append(getEnv("LD_PRELOAD")).append("\n");
        sb.append("PREFIX: ").append(getEnv("PREFIX")).append("\n");
        sb.append("HOME: ").append(getEnv("HOME")).append("\n");
        sb.append("PATH: ").append(getEnv("PATH")).append("\n");
        sb.append("TMPDIR: ").append(getEnv("TMPDIR")).append("\n");
        sb.append("SHELL: ").append(getEnv("SHELL")).append("\n\n");

        // Filesystem Checks
        sb.append("----- Filesystem Checks -----\n");
        checkPath(sb, "Prefix dir", TermuxConstants.PREFIX_PATH);
        checkPath(sb, "Bin dir", TermuxConstants.PREFIX_PATH + "/bin");
        checkPath(sb, "sh", TermuxConstants.PREFIX_PATH + "/bin/sh");
        checkPath(sb, "bash", TermuxConstants.PREFIX_PATH + "/bin/bash");
        checkPath(sb, "env", TermuxConstants.PREFIX_PATH + "/bin/env");
        checkPath(sb, "ld-preload", "/data/data/com.termux/files/usr/lib/libtermux-exec.so");
        checkPath(sb, "ld-preload-alt", "/data/data/com.termux/files/usr/lib/libtermux-exec-ld-preload.so");
        sb.append("\n");

        // libtermux_exec status
        sb.append("----- libtermux_exec Status -----\n");
        checkNativeLib(sb, context, "libtermux_exec.so");
        sb.append("\n");

        // Process Info
        sb.append("----- Process Info -----\n");
        sb.append("PID: ").append(android.os.Process.myPid()).append("\n");
        sb.append("UID: ").append(android.os.Process.myUid()).append("\n");
        try {
            File selfExe = new File("/proc/self/exe");
            sb.append("/proc/self/exe: ").append(selfExe.getCanonicalPath()).append("\n");
        } catch (Exception e) {
            sb.append("/proc/self/exe: error (").append(e.getMessage()).append(")\n");
        }
        sb.append("\n");

        // SECCOMP / bootstrap probe
        sb.append("----- Runtime Probe -----\n");
        sb.append(runCommand("ls", "-la", TermuxConstants.PREFIX_PATH + "/bin/sh"));
        sb.append(runCommand("file", TermuxConstants.PREFIX_PATH + "/bin/sh"));
        sb.append(runCommand("/system/bin/getprop", "ro.build.version.sdk"));
        sb.append("\n");

        sb.append("===== End Report =====\n");
        return sb.toString();
    }

    private static String getAppVersion(Context context) {
        try {
            PackageInfo pi = context.getPackageManager().getPackageInfo(context.getPackageName(), 0);
            return pi.versionName + " (" + pi.versionCode + ")";
        } catch (PackageManager.NameNotFoundException e) {
            return "unknown";
        }
    }

    private static String getEnv(String key) {
        String val = System.getenv(key);
        return val != null ? val : "<not set>";
    }

    private static void checkPath(StringBuilder sb, String label, String path) {
        File f = new File(path);
        sb.append(label).append(": ").append(path);
        if (f.exists()) {
            sb.append(" [exists, ").append(f.isDirectory() ? "dir" : "file");
            sb.append(", size=").append(f.length()).append("]");
            if (!f.isDirectory()) {
                sb.append(" [executable=").append(f.canExecute()).append("]");
            }
        } else {
            sb.append(" [MISSING]");
        }
        sb.append("\n");
    }

    private static void checkNativeLib(StringBuilder sb, Context context, String libName) {
        File nativeLibDir = new File(context.getApplicationInfo().nativeLibraryDir);
        File lib = new File(nativeLibDir, libName);
        sb.append("Native lib dir: ").append(nativeLibDir.getAbsolutePath()).append("\n");
        sb.append(libName).append(": ");
        if (lib.exists()) {
            sb.append("exists (size=").append(lib.length()).append(")\n");
        } else {
            sb.append("MISSING\n");
            // Try to list available libs
            String[] libs = nativeLibDir.list();
            if (libs != null) {
                sb.append("  Available libs: ").append(Arrays.toString(libs)).append("\n");
            }
        }
    }

    private static String runCommand(String... cmd) {
        try {
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.redirectErrorStream(true);
            Process p = pb.start();
            BufferedReader reader = new BufferedReader(new InputStreamReader(p.getInputStream()));
            StringBuilder out = new StringBuilder();
            String line;
            while ((line = reader.readLine()) != null) {
                out.append(line).append("\n");
            }
            p.waitFor();
            return "$ " + String.join(" ", cmd) + "\n" + out.toString() + "\n";
        } catch (Exception e) {
            return "$ " + String.join(" ", cmd) + "\n[error: " + e.getMessage() + "]\n\n";
        }
    }
}
