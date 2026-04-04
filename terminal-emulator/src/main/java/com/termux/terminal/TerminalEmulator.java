package com.termux.terminal;

/**
 * Terminal Emulator - Rust 实现的 Java 包装类
 * 所有实际逻辑都在 Rust 中实现，此类仅包含 JNI 调用
 */
public final class TerminalEmulator {

    static { System.loadLibrary("termux_rust"); }

    // --- 常量定义 ---
    public static final int TERMINAL_CURSOR_STYLE_BLOCK = 0;
    public static final int TERMINAL_CURSOR_STYLE_BAR = 1;
    public static final int TERMINAL_CURSOR_STYLE_UNDERLINE = 2;
    public static final int MOUSE_LEFT_BUTTON = 0;
    public static final int MOUSE_MIDDLE_BUTTON = 1;
    public static final int MOUSE_RIGHT_BUTTON = 2;
    public static final int MOUSE_LEFT_BUTTON_MOVED = 32;
    public static final int MOUSE_WHEELUP_BUTTON = 64;
    public static final int MOUSE_WHEELDOWN_BUTTON = 65;
    public static final int UNICODE_REPLACEMENT_CHAR = 0xFFFD;
    public static final int DEFAULT_TERMINAL_TRANSCRIPT_ROWS = 2000;
    public static final int TERMINAL_TRANSCRIPT_ROWS_MIN = 100;
    public static final int TERMINAL_TRANSCRIPT_ROWS_MAX = 50000;
    public static final int DEFAULT_TERMINAL_CURSOR_STYLE = TERMINAL_CURSOR_STYLE_BLOCK;

    // --- 状态 ---
    private long mEnginePtr;
    private final RustEngineCallback mRustCallback;

    // --- 构造器 ---
    public TerminalEmulator(TerminalOutput session, int columns, int rows,
                           int cellWidthPixels, int cellHeightPixels,
                           Integer transcriptRows, int ptyFd, TerminalSessionClient client) {
        this.mRustCallback = new RustEngineCallback(client);
        if (session instanceof TerminalSession) this.mRustCallback.setSession((TerminalSession) session);
        mEnginePtr = createEngineRustWithCallback(
            columns, rows, cellWidthPixels, cellHeightPixels,
            transcriptRows != null ? transcriptRows : DEFAULT_TERMINAL_TRANSCRIPT_ROWS,
            mRustCallback
        );
        if (mEnginePtr != 0 && ptyFd != -1) nativeStartIoThread(mEnginePtr, ptyFd);
    }

    public TerminalEmulator(TerminalOutput session, long enginePtr, int ptyFd, RustEngineCallback callback) {
        this.mRustCallback = callback;
        if (session instanceof TerminalSession) this.mRustCallback.setSession((TerminalSession) session);
        this.mEnginePtr = enginePtr;
    }

    // --- 数据输入 ---
    public void append(byte[] batch, int length) {
        if (mEnginePtr != 0) processBatchRust(mEnginePtr, batch, length);
    }

    public void processCodePoint(int codePoint) {
        if (mEnginePtr != 0) processCodePointRust(mEnginePtr, codePoint);
    }

    // --- 终端控制 ---
    public void resize(int columns, int rows, int cellWidthPixels, int cellHeightPixels) {
        if (mEnginePtr != 0) resizeEngineRustFull(mEnginePtr, columns, rows, cellWidthPixels, cellHeightPixels);
    }

    public void reset() { resetColors(); }

    public void destroy() {
        if (mEnginePtr != 0) { destroyEngineRust(mEnginePtr); mEnginePtr = 0; }
    }

    public boolean isAlive() { return mEnginePtr != 0; }

    /** 获取 Rust 引擎的原始指针，用于渲染 */
    public long getNativePointer() { return mEnginePtr; }

    // --- 光标 ---
    public int getCursorCol() { return mEnginePtr != 0 ? getCursorColFromRust(mEnginePtr) : 0; }
    public int getCursorRow() { return mEnginePtr != 0 ? getCursorRowFromRust(mEnginePtr) : 0; }
    public int getCursorStyle() { return mEnginePtr != 0 ? getCursorStyleFromRust(mEnginePtr) : TERMINAL_CURSOR_STYLE_BLOCK; }
    public void setCursorStyle(int cursorStyle) { if (mEnginePtr != 0) setCursorStyleFromRust(mEnginePtr, cursorStyle); }
    public void setCursorBlinkState(boolean state) { if (mEnginePtr != 0) setCursorBlinkStateInRust(mEnginePtr, state); }
    public void setCursorBlinkingEnabled(boolean enabled) { if (mEnginePtr != 0) setCursorBlinkingEnabledInRust(mEnginePtr, enabled); }
    public boolean isCursorEnabled() { return mEnginePtr != 0 && isCursorEnabledFromRust(mEnginePtr); }
    public boolean shouldCursorBeVisible() { return mEnginePtr != 0 && shouldCursorBeVisibleFromRust(mEnginePtr); }

    // --- 模式查询 ---
    public boolean isReverseVideo() { return mEnginePtr != 0 && isReverseVideoFromRust(mEnginePtr); }
    public boolean isAlternateBufferActive() { return mEnginePtr != 0 && isAlternateBufferActiveFromRust(mEnginePtr); }
    public boolean isCursorKeysApplicationMode() { return mEnginePtr != 0 && isCursorKeysApplicationModeFromRust(mEnginePtr); }
    public boolean isKeypadApplicationMode() { return mEnginePtr != 0 && isKeypadApplicationModeFromRust(mEnginePtr); }
    public boolean isMouseTrackingActive() { return mEnginePtr != 0 && isMouseTrackingActiveFromRust(mEnginePtr); }
    public boolean isAutoScrollDisabled() { return mEnginePtr != 0 && isAutoScrollDisabledFromRust(mEnginePtr); }
    public void doDecSetOrReset(boolean setting, int mode) { if (mEnginePtr != 0) doDecSetOrResetFromRust(mEnginePtr, setting, mode); }
    public void toggleAutoScrollDisabled() { if (mEnginePtr != 0) toggleAutoScrollDisabledFromRust(mEnginePtr); }

    // --- 尺寸 ---
    public int getRows() { return mEnginePtr != 0 ? getRowsFromRust(mEnginePtr) : 0; }
    public int getCols() { return mEnginePtr != 0 ? getColsFromRust(mEnginePtr) : 0; }
    public int getActiveTranscriptRows() { return mEnginePtr != 0 ? getActiveTranscriptRowsFromRust(mEnginePtr) : 0; }
    public int getTotalRows() { return getActiveTranscriptRows() + getRows(); }
    /** @deprecated 使用 getActiveTranscriptRows() + getRows() 替代 */
    @Deprecated public int getActiveRows() { return getTotalRows(); }
    /** @deprecated 使用 readRow() 直接访问屏幕数据 */
    @Deprecated public TerminalBufferCompat getScreen() { return new TerminalBufferCompat(this, getCols(), getRows(), getTotalRows()); }

    // --- 滚动 ---
    public int getScrollCounter() { return mEnginePtr != 0 ? getScrollCounterFromRust(mEnginePtr) : 0; }
    public void clearScrollCounter() { if (mEnginePtr != 0) clearScrollCounterFromRust(mEnginePtr); }

    // --- 屏幕数据读取 (用于渲染) ---
    public void readRow(int row, int[] text, long[] styles) {
        if (mEnginePtr != 0) readRowFromRust(mEnginePtr, row, text, styles);
    }
    public String getSelectedText(int x1, int y1, int x2, int y2) {
        return mEnginePtr != 0 ? getSelectedTextFromRust(mEnginePtr, x1, y1, x2, y2) : "";
    }
    public String getWordAtLocation(int x, int y) {
        return mEnginePtr != 0 ? getWordAtLocationFromRust(mEnginePtr, x, y) : "";
    }
    public String getTranscriptText() {
        return mEnginePtr != 0 ? getTranscriptTextFromRust(mEnginePtr) : "";
    }
    public String getTitle() {
        return mEnginePtr != 0 ? getTitleFromRust(mEnginePtr) : null;
    }

    // --- 颜色 ---
    public int[] getCurrentColors() {
        return mEnginePtr != 0 ? getColorsFromRust(mEnginePtr) : new int[259];
    }
    public void resetColors() { if (mEnginePtr != 0) resetColorsFromRust(mEnginePtr); }
    public void updateColorsFromProperties(java.util.Properties props) {
        if (mEnginePtr != 0 && props != null) updateColorsFromProperties(mEnginePtr, props);
    }
    public void setCursorColorForBackground() {
        if (mEnginePtr != 0) setCursorColorForBackgroundFromRust(mEnginePtr);
    }

    // --- 输入事件 ---
    public void sendMouseEvent(int button, int col, int row, boolean pressed) {
        if (mEnginePtr != 0) sendMouseEventFromRust(mEnginePtr, button, col, row, pressed);
    }
    public String sendKeyEvent(int keyCode, int metaState) {
        if (mEnginePtr != 0) return sendKeyCodeFromRust(mEnginePtr, keyCode, null, metaState);
        return null;
    }
    public void sendCharEvent(char c, int metaState) {
        if (mEnginePtr != 0) sendKeyCodeFromRust(mEnginePtr, 0, String.valueOf(c), metaState);
    }
    public void paste(String text) {
        if (mEnginePtr != 0) pasteTextFromRust(mEnginePtr, text);
    }

    // --- 客户端更新 ---
    public void updateTerminalSessionClient(TerminalSessionClient client) {
        if (mEnginePtr != 0) updateTerminalSessionClientFromRust(mEnginePtr, mRustCallback);
    }

    // --- 调试 ---
    @Override
    public String toString() {
        return mEnginePtr == 0 ? "TerminalEmulator[destroyed]" : getDebugInfoFromRust(mEnginePtr);
    }

    // --- Native 接口 ---
    private static native long createEngineRustWithCallback(int cols, int rows, int cw, int ch, int totalRows, RustEngineCallback callback);
    private static native void destroyEngineRust(long enginePtr);
    private static native void nativeStartIoThread(long enginePtr, int fd);
    private static native void processBatchRust(long enginePtr, byte[] batch, int length);
    private static native void processCodePointRust(long enginePtr, int codePoint);
    private static native void resizeEngineRustFull(long enginePtr, int cols, int rows, int cw, int ch);
    private static native String getDebugInfoFromRust(long enginePtr);
    private static native int getCursorColFromRust(long enginePtr);
    private static native int getCursorRowFromRust(long enginePtr);
    private static native int getCursorStyleFromRust(long enginePtr);
    private static native void setCursorStyleFromRust(long enginePtr, int cursorStyle);
    private static native void doDecSetOrResetFromRust(long enginePtr, boolean setting, int mode);
    private static native boolean isCursorEnabledFromRust(long enginePtr);
    private static native boolean shouldCursorBeVisibleFromRust(long enginePtr);
    private static native boolean isReverseVideoFromRust(long enginePtr);
    private static native boolean isAlternateBufferActiveFromRust(long enginePtr);
    private static native boolean isCursorKeysApplicationModeFromRust(long enginePtr);
    private static native boolean isKeypadApplicationModeFromRust(long enginePtr);
    private static native boolean isMouseTrackingActiveFromRust(long enginePtr);
    private static native int getScrollCounterFromRust(long enginePtr);
    private static native void clearScrollCounterFromRust(long enginePtr);
    private static native int getRowsFromRust(long enginePtr);
    private static native int getColsFromRust(long enginePtr);
    private static native int getActiveTranscriptRowsFromRust(long enginePtr);
    private static native boolean isAutoScrollDisabledFromRust(long enginePtr);
    private static native void toggleAutoScrollDisabledFromRust(long enginePtr);
    private static native void readRowFromRust(long enginePtr, int row, int[] text, long[] styles);
    private static native String getSelectedTextFromRust(long enginePtr, int x1, int y1, int x2, int y2);
    private static native String getWordAtLocationFromRust(long enginePtr, int x, int y);
    private static native String getTranscriptTextFromRust(long enginePtr);
    private static native String getTitleFromRust(long enginePtr);
    private static native void sendMouseEventFromRust(long enginePtr, int button, int col, int row, boolean pressed);
    private static native String sendKeyCodeFromRust(long enginePtr, int keyCode, String charStr, int metaState);
    private static native void pasteTextFromRust(long enginePtr, String text);
    private static native int[] getColorsFromRust(long enginePtr);
    private static native void resetColorsFromRust(long enginePtr);
    private static native void updateColorsFromProperties(long enginePtr, java.util.Properties properties);
    private static native void setCursorColorForBackgroundFromRust(long enginePtr);
    private static native void updateTerminalSessionClientFromRust(long enginePtr, TerminalSessionClient client);
    private static native void setCursorBlinkStateInRust(long enginePtr, boolean state);
    private static native void setCursorBlinkingEnabledInRust(long enginePtr, boolean enabled);
}
