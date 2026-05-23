package com.termux.app;

import android.util.Log;
import com.termux.BuildConfig;

/**
 * Unified logging utility for the Termux app.
 *
 * All log tags are prefixed with "Termux." so they can be easily filtered:
 *   adb logcat -s "Termux.*"
 *
 * Debug and verbose logs are automatically stripped in release builds
 * (controlled by BuildConfig.DEBUG).
 */
public final class TermuxLogger {

    private static final String PREFIX = "Termux";

    private TermuxLogger() {
        // utility class
    }

    private static String tag(String module) {
        return PREFIX + "." + module;
    }

    public static void v(String module, String msg) {
        if (BuildConfig.DEBUG) {
            Log.v(tag(module), msg);
        }
    }

    public static void v(String module, String msg, Throwable tr) {
        if (BuildConfig.DEBUG) {
            Log.v(tag(module), msg, tr);
        }
    }

    public static void d(String module, String msg) {
        if (BuildConfig.DEBUG) {
            Log.d(tag(module), msg);
        }
    }

    public static void d(String module, String msg, Throwable tr) {
        if (BuildConfig.DEBUG) {
            Log.d(tag(module), msg, tr);
        }
    }

    public static void i(String module, String msg) {
        Log.i(tag(module), msg);
    }

    public static void i(String module, String msg, Throwable tr) {
        Log.i(tag(module), msg, tr);
    }

    public static void w(String module, String msg) {
        Log.w(tag(module), msg);
    }

    public static void w(String module, String msg, Throwable tr) {
        Log.w(tag(module), msg, tr);
    }

    public static void e(String module, String msg) {
        Log.e(tag(module), msg);
    }

    public static void e(String module, String msg, Throwable tr) {
        Log.e(tag(module), msg, tr);
    }

    /**
     * Log a stack trace with a custom message.
     */
    public static void wtf(String module, String msg, Throwable tr) {
        Log.wtf(tag(module), msg, tr);
    }
}
