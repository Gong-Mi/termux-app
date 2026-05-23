package com.termux.app;

import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Build;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Map;

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

    /**
     * Collect a detailed, normalized environment configuration report.
     * Ensures TermuxConstants are initialized to real paths before probing.
     */
    public static String collectEnvConfig(Context context) {
        ensureInitialized(context);

        StringBuilder sb = new StringBuilder();
        sb.append("===== Termux Environment Config Report =====\n");
        sb.append("Generated: ").append(java.text.DateFormat.getDateTimeInstance().format(new java.util.Date())).append("\n\n");

        // App & System Info
        sb.append("----- App & System -----\n");
        sb.append("App Version: ").append(getAppVersion(context)).append("\n");
        sb.append("Package: ").append(context.getPackageName()).append("\n");
        sb.append("Android Version: ").append(Build.VERSION.RELEASE).append(" (API ").append(Build.VERSION.SDK_INT).append(")\n");
        sb.append("Device: ").append(Build.MANUFACTURER).append(" ").append(Build.MODEL).append("\n");
        sb.append("ABI: ").append(Build.SUPPORTED_ABIS != null ? Arrays.toString(Build.SUPPORTED_ABIS) : "unknown").append("\n");
        sb.append("User: ").append(android.os.Process.myUid()).append("\n");
        sb.append("Native lib dir: ").append(context.getApplicationInfo().nativeLibraryDir).append("\n\n");

        // Initialization Status
        sb.append("----- Initialization Status -----\n");
        File prefixDir = new File(TermuxConstants.PREFIX_PATH);
        File lockFile = new File(TermuxConstants.PREFIX_PATH +
            "/etc/termux/termux-bootstrap/second-stage/termux-bootstrap-second-stage.sh.lock");
        sb.append("PREFIX exists: ").append(prefixDir.exists()).append("\n");
        sb.append("Bootstrap lock exists: ").append(lockFile.exists()).append("\n");
        sb.append("Data dir path: ").append(context.getFilesDir().getAbsolutePath()).append("\n");
        sb.append("FILES_PATH: ").append(TermuxConstants.FILES_PATH).append("\n");
        sb.append("PREFIX_PATH: ").append(TermuxConstants.PREFIX_PATH).append("\n");
        sb.append("HOME_PATH: ").append(TermuxConstants.HOME_PATH).append("\n");
        sb.append("APP_LIB_PATH: ").append(TermuxConstants.APP_LIB_PATH).append("\n\n");

        // Full Environment Dump
        sb.append("----- Full Environment -----\n");
        Map<String, String> env = System.getenv();
        List<String> envKeys = new ArrayList<>(env.keySet());
        Collections.sort(envKeys);
        for (String key : envKeys) {
            sb.append(key).append("=").append(env.get(key)).append("\n");
        }
        sb.append("\n");

        // Filesystem Checks (expanded)
        sb.append("----- Filesystem Checks -----\n");
        checkPath(sb, "Prefix dir", TermuxConstants.PREFIX_PATH);
        checkPath(sb, "Bin dir", TermuxConstants.PREFIX_PATH + "/bin");
        checkPath(sb, "Lib dir", TermuxConstants.PREFIX_PATH + "/lib");
        checkPath(sb, "Libexec dir", TermuxConstants.PREFIX_PATH + "/libexec");
        checkPath(sb, "Tmp dir", TermuxConstants.PREFIX_PATH + "/tmp");
        checkPath(sb, "Etc dir", TermuxConstants.PREFIX_PATH + "/etc");
        checkPath(sb, "sh", TermuxConstants.PREFIX_PATH + "/bin/sh");
        checkPath(sb, "bash", TermuxConstants.PREFIX_PATH + "/bin/bash");
        checkPath(sb, "env", TermuxConstants.PREFIX_PATH + "/bin/env");
        checkPath(sb, "ls", TermuxConstants.PREFIX_PATH + "/bin/ls");
        checkPath(sb, "cat", TermuxConstants.PREFIX_PATH + "/bin/cat");
        checkPath(sb, "git", TermuxConstants.PREFIX_PATH + "/bin/git");
        checkPath(sb, "python3", TermuxConstants.PREFIX_PATH + "/bin/python3");
        checkPath(sb, "linker64", "/system/bin/linker64");
        checkPath(sb, "linker", "/system/bin/linker");
        sb.append("\n");

        // LD_PRELOAD Candidates
        sb.append("----- LD_PRELOAD Candidates -----\n");
        String[] candidates = {
            TermuxConstants.PREFIX_PATH + "/lib/libtermux-exec-ld-preload.so",
            TermuxConstants.PREFIX_PATH + "/lib/libtermux-exec.so",
            TermuxConstants.PREFIX_PATH + "/lib/libtermux_exec.so",
            context.getApplicationInfo().nativeLibraryDir + "/libtermux_exec.so",
            context.getApplicationInfo().nativeLibraryDir + "/libtermux-exec.so",
        };
        String foundLdPreload = null;
        for (String candidate : candidates) {
            File f = new File(candidate);
            if (f.exists()) {
                sb.append("FOUND: ").append(candidate).append(" (size=").append(f.length()).append(")\n");
                if (foundLdPreload == null) {
                    foundLdPreload = candidate;
                }
            } else {
                sb.append("MISSING: ").append(candidate).append("\n");
            }
        }
        sb.append("Selected LD_PRELOAD: ").append(foundLdPreload != null ? foundLdPreload : "<none>").append("\n\n");

        // Native Library Status
        sb.append("----- Native Library Status -----\n");
        checkNativeLib(sb, context, "libtermux_exec.so");
        checkNativeLib(sb, context, "libtermux-exec.so");
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

        sb.append("===== End Report =====\n");
        return sb.toString();
    }

    /**
     * Collect command availability report by running the bundled termux_exec_device_probe binary.
     * The binary is extracted from APK assets and executed with a fully normalized environment.
     */
    public static String collectCommandAvailability(Context context) {
        ensureInitialized(context);

        StringBuilder sb = new StringBuilder();
        sb.append("===== Termux Command Availability Report =====\n");
        sb.append("Generated: ").append(java.text.DateFormat.getDateTimeInstance().format(new java.util.Date())).append("\n\n");

        File probeBinary = extractDeviceProbe(context);
        if (probeBinary == null) {
            sb.append("ERROR: Failed to extract termux_exec_device_probe from APK assets.\n");
            sb.append("Supported ABIs: ").append(Arrays.toString(Build.SUPPORTED_ABIS)).append("\n");
            sb.append("\n===== End Report =====\n");
            return sb.toString();
        }

        sb.append("Probe binary: ").append(probeBinary.getAbsolutePath()).append("\n");
        sb.append("Probe size: ").append(probeBinary.length()).append(" bytes\n");
        sb.append("Probe executable: ").append(probeBinary.canExecute()).append("\n\n");

        // Build normalized environment for the probe
        String ldPreload = findLdPreload(context);
        String nativeLibDir = context.getApplicationInfo().nativeLibraryDir;

        List<String> envList = new ArrayList<>();
        envList.add("PREFIX=" + TermuxConstants.PREFIX_PATH);
        envList.add("HOME=" + TermuxConstants.HOME_PATH);
        envList.add("TMPDIR=" + TermuxConstants.PREFIX_PATH + "/tmp");
        envList.add("PATH=" + TermuxConstants.PREFIX_PATH + "/bin:/system/bin");
        envList.add("LANG=C.UTF-8");
        envList.add("ANDROID_ROOT=/system");
        envList.add("ANDROID_DATA=/data");
        if (ldPreload != null) {
            envList.add("LD_PRELOAD=" + ldPreload);
        }
        // Ensure libc++_shared.so can be found by the probe binary
        envList.add("LD_LIBRARY_PATH=" + nativeLibDir);

        // Pass overrides for the probe's internal defaults
        envList.add("TERMUX_EXEC_PROBE_PREFIX=" + TermuxConstants.PREFIX_PATH);
        envList.add("TERMUX_EXEC_PROBE_HOME=" + TermuxConstants.HOME_PATH);
        envList.add("TERMUX_EXEC_PROBE_TMPDIR=" + TermuxConstants.PREFIX_PATH + "/tmp");
        if (ldPreload != null) {
            envList.add("TERMUX_EXEC_PROBE_LD_PRELOAD=" + ldPreload);
        }

        try {
            ProcessBuilder pb = new ProcessBuilder(probeBinary.getAbsolutePath());
            pb.redirectErrorStream(true);
            Map<String, String> pbEnv = pb.environment();
            pbEnv.clear();
            for (String entry : envList) {
                String[] parts = entry.split("=", 2);
                if (parts.length == 2) {
                    pbEnv.put(parts[0], parts[1]);
                }
            }
            Process p = pb.start();
            BufferedReader reader = new BufferedReader(new InputStreamReader(p.getInputStream()));
            StringBuilder out = new StringBuilder();
            String line;
            while ((line = reader.readLine()) != null) {
                out.append(line).append("\n");
            }
            int exitCode = p.waitFor();
            sb.append("----- Probe Output -----\n");
            sb.append(out.toString());
            sb.append("\n");
            sb.append("Probe exit code: ").append(exitCode).append("\n");
        } catch (Exception e) {
            sb.append("ERROR running probe: ").append(e.getMessage()).append("\n");
            TermuxLogger.e("LogCollector", "Failed to run device probe", e);
        }

        sb.append("===== End Report =====\n");
        return sb.toString();
    }

    private static void ensureInitialized(Context context) {
        TermuxConstants.init(context);
    }

    private static String findLdPreload(Context context) {
        String[] candidates = {
            TermuxConstants.PREFIX_PATH + "/lib/libtermux-exec-ld-preload.so",
            TermuxConstants.PREFIX_PATH + "/lib/libtermux-exec.so",
            TermuxConstants.PREFIX_PATH + "/lib/libtermux_exec.so",
            context.getApplicationInfo().nativeLibraryDir + "/libtermux_exec.so",
            context.getApplicationInfo().nativeLibraryDir + "/libtermux-exec.so",
        };
        for (String candidate : candidates) {
            if (new File(candidate).exists()) {
                return candidate;
            }
        }
        return null;
    }

    private static File extractDeviceProbe(Context context) {
        // Extract to $PREFIX/bin/ instead of getFilesDir() to avoid Android 10+ W^X restriction.
        // getFilesDir() is subject to noexec on targetSdk >= 29, but $PREFIX/bin is the
        // Termux-managed filesystem and allows execution.
        File probesDir = new File(TermuxConstants.PREFIX_PATH + "/bin");
        if (!probesDir.exists() && !probesDir.mkdirs()) {
            TermuxLogger.e("LogCollector", "Failed to create probes dir: " + probesDir.getAbsolutePath());
            return null;
        }

        File probeFile = new File(probesDir, "termux_exec_device_probe");

        // Find the best matching ABI from assets
        String[] supportedAbis = Build.SUPPORTED_ABIS;
        if (supportedAbis == null || supportedAbis.length == 0) {
            supportedAbis = new String[]{ Build.CPU_ABI };
        }

        String assetPath = null;
        for (String abi : supportedAbis) {
            String candidate = "termux-probes/" + abi + "/termux_exec_device_probe";
            try {
                InputStream test = context.getAssets().open(candidate);
                test.close();
                assetPath = candidate;
                break;
            } catch (IOException e) {
                // Not found for this ABI, try next
            }
        }

        if (assetPath == null) {
            TermuxLogger.e("LogCollector", "No termux_exec_device_probe found in assets for ABIs: " + Arrays.toString(supportedAbis));
            return null;
        }

        try (InputStream in = context.getAssets().open(assetPath);
             FileOutputStream out = new FileOutputStream(probeFile)) {
            byte[] buffer = new byte[8192];
            int read;
            while ((read = in.read(buffer)) != -1) {
                out.write(buffer, 0, read);
            }
            out.flush();
        } catch (IOException e) {
            TermuxLogger.e("LogCollector", "Failed to extract device probe", e);
            return null;
        }

        if (!probeFile.setExecutable(true, false)) {
            TermuxLogger.e("LogCollector", "Failed to set executable permission on device probe");
        }

        return probeFile;
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
