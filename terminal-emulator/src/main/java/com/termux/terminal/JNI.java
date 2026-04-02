package com.termux.terminal;

/**
 * Native methods for creating and managing pseudoterminal subprocesses.
 */
final class JNI {

    public static final boolean sNativeLibrariesLoaded;

    static {
        boolean loaded = false;
        try {
            String vendor = System.getProperty("java.vendor");
            if (vendor != null && vendor.contains("Android")) {
                System.loadLibrary("termux_rust");
                loaded = true;
            } else {
                // For unit tests on host, try to load from known locations
                String libName = System.mapLibraryName("termux_rust");
                // Try multiple possible paths for host-side testing
                String[] possiblePaths = {
                    "terminal-emulator/src/main/jniLibs/x86_64/" + libName,
                    "src/main/jniLibs/x86_64/" + libName,
                    "terminal-emulator/src/main/rust/target/release/" + libName,
                    "src/main/rust/target/release/" + libName,
                    "build/libs/" + libName
                };
                for (String path : possiblePaths) {
                    java.io.File libPath = new java.io.File(path);
                    if (libPath.exists()) {
                        System.load(libPath.getAbsolutePath());
                        loaded = true;
                        break;
                    }
                }
            }
        } catch (Throwable t) {
            // Silently fail for now, but in tests this should be checked
        }
        sNativeLibrariesLoaded = loaded;
    }

    public static native int createSubprocess(String cmd, String cwd, String[] args, String[] envVars, int[] processId, int rows, int columns, int cellWidth, int cellHeight);
    public static native void createSessionAsync(String cmd, String cwd, String[] args, String[] envVars, int rows, int columns, int cellWidth, int cellHeight, int transcriptRows, RustEngineCallback callback);
    public static native void setPtyWindowSize(int fd, int rows, int cols, int cellWidth, int cellHeight);
    public static native int waitFor(int processId);
    public static native void close(int fileDescriptor);
    
    // Session Coordinator methods
    public static native int registerSession();
    public static native void unregisterSession(int sessionId);
    public static native boolean tryAcquirePkgLock(int sessionId);
    public static native void releasePkgLock(int sessionId);
    public static native boolean isPkgLockHeld();
    public static native int getPkgLockOwner();
    public static native String getSessionState(int sessionId);
    public static native String getAllSessionStates();

    // KeyHandler methods (Rust implementation)
    public static native String getKeyCode(int keyCode, int keyMode, boolean cursorApp, boolean keypad);
    public static native String getKeyCodeFromTermcap(String termcap, boolean cursorApp, boolean keypad);
}
