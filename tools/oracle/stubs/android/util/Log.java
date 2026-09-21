package android.util;

/**
 * Minimal stand-in for the Android framework class, used only to compile the reference
 * terminal implementation (upstream Termux `Logger.java`) on a plain JVM.
 *
 * The oracle harness must not depend on the Android SDK, otherwise the reference side of
 * the differential comparison could not run in ordinary CI (or on Termux itself, which has
 * javac but no Android SDK). Log output is discarded: log text is not part of the
 * comparison and must never be used as evidence.
 */
public final class Log {

    private Log() {}

    public static int v(String tag, String msg) { return 0; }

    public static int d(String tag, String msg) { return 0; }

    public static int i(String tag, String msg) { return 0; }

    public static int w(String tag, String msg) { return 0; }

    public static int e(String tag, String msg) { return 0; }

    public static int e(String tag, String msg, Throwable tr) { return 0; }

    public static boolean isLoggable(String tag, int level) { return false; }

    public static final int VERBOSE = 2;
    public static final int DEBUG = 3;
    public static final int INFO = 4;
    public static final int WARN = 5;
    public static final int ERROR = 6;
}