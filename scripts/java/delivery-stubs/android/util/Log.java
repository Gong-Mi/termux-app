package android.util;
/** Test-only logging boundary. */
public class Log {
    public static int d(String tag, String message) { return 0; }
    public static int w(String tag, String message) { return 0; }
    public static int e(String tag, String message) { System.err.println(tag + ": " + message); return 0; }
}
