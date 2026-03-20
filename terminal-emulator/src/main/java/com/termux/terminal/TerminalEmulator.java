package com.termux.terminal;

import android.view.KeyEvent;

/**
 * Terminal Emulator - Rust 实现的 Java 包装类
 * 所有实际逻辑都在 Rust 中实现，此类仅包含 JNI 调用
 */
public final class TerminalEmulator {

    // 原生引擎指针
    private long mEnginePtr = 0;
    
    // 终端会话
    private final TerminalOutput mSession;
    
    // 客户端
    private TerminalSessionClient mClient;

    // 光标样式常量
    public static final int TERMINAL_CURSOR_STYLE_BLOCK = 0;
    public static final int TERMINAL_CURSOR_STYLE_UNDERLINE = 1;
    public static final int TERMINAL_CURSOR_STYLE_BAR = 2;
    public static final int DEFAULT_TERMINAL_CURSOR_STYLE = TERMINAL_CURSOR_STYLE_BLOCK;
    public static final Integer[] TERMINAL_CURSOR_STYLES_LIST = {TERMINAL_CURSOR_STYLE_BLOCK, TERMINAL_CURSOR_STYLE_UNDERLINE, TERMINAL_CURSOR_STYLE_BAR};
    
    // 滚动历史行数常量
    public static final int TERMINAL_TRANSCRIPT_ROWS_MIN = 100;
    public static final int TERMINAL_TRANSCRIPT_ROWS_MAX = 50000;
    public static final int DEFAULT_TERMINAL_TRANSCRIPT_ROWS = 2000;
    
    // 鼠标按钮常量
    public static final int MOUSE_LEFT_BUTTON = 0;
    public static final int MOUSE_MIDDLE_BUTTON = 1;
    public static final int MOUSE_RIGHT_BUTTON = 2;
    public static final int MOUSE_LEFT_BUTTON_MOVED = 32;
    public static final int MOUSE_RIGHT_BUTTON_MOVED = 34;
    public static final int MOUSE_MIDDLE_BUTTON_MOVED = 33;
    public static final int MOUSE_WHEELUP_BUTTON = 64;
    public static final int MOUSE_WHEELDOWN_BUTTON = 65;
    
    // Unicode 替换字符
    public static final int UNICODE_REPLACEMENT_CHAR = 0xFFFD;

    static {
        // 加载 Rust 库
        try {
            System.loadLibrary("termux_rust");
        } catch (UnsatisfiedLinkError e) {
            // 库可能尚未加载，将在使用时处理
        }
    }

    public TerminalEmulator(TerminalOutput session, int columns, int rows, 
                           int cellWidthPixels, int cellHeightPixels, 
                           Integer transcriptRows, TerminalSessionClient client) {
        mSession = session;
        mClient = client;
        
        android.util.Log.d("Termux-JNI", "Calling createEngineRustWithCallback: cols=" + columns + ", rows=" + rows);
        // 创建 Rust 引擎
        mEnginePtr = createEngineRustWithCallback(
            columns, rows, cellWidthPixels, cellHeightPixels, 
            transcriptRows != null ? transcriptRows : 2000,
            client
        );
        android.util.Log.d("Termux-JNI", "Engine creation complete, mEnginePtr=" + mEnginePtr);
    }

    public void updateTerminalSessionClient(TerminalSessionClient client) {
        mClient = client;
        if (mEnginePtr != 0) {
            updateTerminalSessionClientFromRust(mEnginePtr, client);
        }
    }

    public synchronized void resize(int columns, int rows, int cellWidthPixels, int cellHeightPixels) {
        if (mEnginePtr != 0) {
            resizeEngineRustFull(mEnginePtr, columns, rows, cellWidthPixels, cellHeightPixels);
        }
    }

    public synchronized void append(byte[] batch, int length) {
        if (mEnginePtr != 0) {
            try {
                processBatchRust(mEnginePtr, batch, length);
            } catch (Exception e) {
                android.util.Log.e("Termux-JNI", "Error in processBatchRust", e);
            }
        } else {
            android.util.Log.w("Termux-JNI", "append called but mEnginePtr is 0");
        }
    }

    public String getTitle() {
        if (mEnginePtr != 0) {
            return getTitleFromRust(mEnginePtr);
        }
        return null;
    }

    public int getCursorRow() {
        if (mEnginePtr != 0) {
            return getCursorRowFromRust(mEnginePtr);
        }
        return 0;
    }

    public int getCursorCol() {
        if (mEnginePtr != 0) {
            return getCursorColFromRust(mEnginePtr);
        }
        return 0;
    }

    public int getCursorStyle() {
        if (mEnginePtr != 0) {
            return getCursorStyleFromRust(mEnginePtr);
        }
        return TERMINAL_CURSOR_STYLE_BLOCK;
    }

    public void setCursorStyle() {
        // 由客户端处理
    }

    public void setCursorBlinkingEnabled(boolean enabled) {
        if (mEnginePtr != 0) {
            setCursorBlinkingEnabledInRust(mEnginePtr, enabled);
        }
    }

    public void setCursorBlinkState(boolean state) {
        if (mEnginePtr != 0) {
            setCursorBlinkStateInRust(mEnginePtr, state);
        }
    }

    public boolean shouldCursorBeVisible() {
        if (mEnginePtr != 0) {
            return shouldCursorBeVisibleFromRust(mEnginePtr);
        }
        return true;
    }

    public boolean isCursorEnabled() {
        if (mEnginePtr != 0) {
            return isCursorEnabledFromRust(mEnginePtr);
        }
        return true;
    }

    public boolean isReverseVideo() {
        if (mEnginePtr != 0) {
            return isReverseVideoFromRust(mEnginePtr);
        }
        return false;
    }

    public boolean isAlternateBufferActive() {
        if (mEnginePtr != 0) {
            return isAlternateBufferActiveFromRust(mEnginePtr);
        }
        return false;
    }

    public boolean isCursorKeysApplicationMode() {
        if (mEnginePtr != 0) {
            return isCursorKeysApplicationModeFromRust(mEnginePtr);
        }
        return false;
    }

    public boolean isKeypadApplicationMode() {
        if (mEnginePtr != 0) {
            return isKeypadApplicationModeFromRust(mEnginePtr);
        }
        return false;
    }

    public boolean isMouseTrackingActive() {
        if (mEnginePtr != 0) {
            return isMouseTrackingActiveFromRust(mEnginePtr);
        }
        return false;
    }

    public boolean isInsertModeActive() {
        if (mEnginePtr != 0) {
            return isInsertModeActiveFromRust(mEnginePtr);
        }
        return false;
    }

    public int getScrollCounter() {
        if (mEnginePtr != 0) {
            return getScrollCounterFromRust(mEnginePtr);
        }
        return 0;
    }

    public void clearScrollCounter() {
        if (mEnginePtr != 0) {
            clearScrollCounterFromRust(mEnginePtr);
        }
    }

    public void readRow(int row, char[] text, long[] styles) {
        if (mEnginePtr != 0) {
            readRowFromRust(mEnginePtr, row, text, styles);
        }
    }

    public boolean isAutoScrollDisabled() {
        if (mEnginePtr != 0) {
            return isAutoScrollDisabledFromRust(mEnginePtr);
        }
        return false;
    }

    public void toggleAutoScrollDisabled() {
        if (mEnginePtr != 0) {
            toggleAutoScrollDisabledFromRust(mEnginePtr);
        }
    }

    public void reset() {
        // Rust 引擎在 resize 时自动重置
    }

    public void sendMouseEvent(int mouseButton, int column, int row, boolean pressed) {
        if (mEnginePtr != 0) {
            sendMouseEventFromRust(mEnginePtr, mouseButton, column, row, pressed);
        }
    }

    public void sendKeyCode(int keyCode, char[] chars, int metaState) {
        if (mEnginePtr != 0) {
            String charStr = (chars != null && chars.length > 0) ? new String(chars) : "";
            sendKeyCodeFromRust(mEnginePtr, keyCode, charStr, metaState);
        }
    }

    public void paste(String text) {
        if (mEnginePtr != 0) {
            pasteTextFromRust(mEnginePtr, text);
        }
    }

    public int getTerminalTranscriptRows(Integer transcriptRows) {
        return transcriptRows != null ? transcriptRows : 2000;
    }

    // ========================================================================
    // 公共字段访问器（供 TerminalView 使用）
    // ========================================================================
    
    /** @deprecated 使用 getRows() 代替 */
    @Deprecated
    public int getmRows() {
        return getRows();
    }
    
    /** @deprecated 使用 getCols() 代替 */
    @Deprecated
    public int getmColumns() {
        return getCols();
    }
    
    /** 获取终端行数 */
    public int getRows() {
        // 通过 JNI 获取 Rust 侧的行数
        if (mEnginePtr != 0) {
            return getRowsFromRust(mEnginePtr);
        }
        return 24;
    }
    
    /** 获取终端列数 */
    public int getCols() {
        // 通过 JNI 获取 Rust 侧的列数
        if (mEnginePtr != 0) {
            return getColsFromRust(mEnginePtr);
        }
        return 80;
    }

    /** 获取活动滚动历史行数 */
    public int getActiveTranscriptRows() {
        if (mEnginePtr != 0) {
            return getActiveTranscriptRowsFromRust(mEnginePtr);
        }
        return 0;
    }

    /** 获取活动总行数（屏幕 + 滚动历史） */
    public int getActiveRows() {
        return getActiveTranscriptRows() + getRows();
    }

    /** 获取选定区域的文本 */
    public String getSelectedText(int x1, int y1, int x2, int y2) {
        if (mEnginePtr != 0) {
            return getSelectedTextFromRust(mEnginePtr, x1, y1, x2, y2);
        }
        return "";
    }

    /** 获取当前颜色数组（用于渲染） */
    public int[] getCurrentColors() {
        if (mEnginePtr != 0) {
            return getColorsFromRust(mEnginePtr);
        }
        return new int[259]; // 默认颜色数组
    }

    /** 重置颜色为默认值 */
    public void resetColors() {
        if (mEnginePtr != 0) {
            resetColorsFromRust(mEnginePtr);
        }
    }

    public TerminalBuffer getScreen() {
        // 返回 null，实际屏幕数据通过 Rust 共享内存访问
        // TerminalView 需要适配这个变化
        return null;
    }

    public synchronized void destroy() {
        if (mEnginePtr != 0) {
            destroyEngineRust(mEnginePtr);
            mEnginePtr = 0;
        }
    }

    // ========================================================================
    // JNI 方法声明
    // ========================================================================

    private static native long createEngineRustWithCallback(
        int columns, int rows, int cellWidthPixels, int cellHeightPixels,
        int transcriptRows, TerminalSessionClient client
    );

    private static native void processBatchRust(
        long enginePtr, byte[] batch, int length
    );

    private static native void resizeEngineRustFull(
        long enginePtr, int newCols, int newRows, 
        int cellWidthPixels, int cellHeightPixels
    );

    private static native void destroyEngineRust(long enginePtr);

    private static native String getTitleFromRust(long enginePtr);

    private static native int getCursorRowFromRust(long enginePtr);

    private static native int getCursorColFromRust(long enginePtr);

    private static native int getCursorStyleFromRust(long enginePtr);

    private static native boolean shouldCursorBeVisibleFromRust(long enginePtr);

    private static native boolean isCursorEnabledFromRust(long enginePtr);

    private static native boolean isReverseVideoFromRust(long enginePtr);

    private static native boolean isAlternateBufferActiveFromRust(long enginePtr);

    private static native boolean isCursorKeysApplicationModeFromRust(long enginePtr);

    private static native boolean isKeypadApplicationModeFromRust(long enginePtr);

    private static native boolean isMouseTrackingActiveFromRust(long enginePtr);

    private static native boolean isInsertModeActiveFromRust(long enginePtr);

    private static native int getScrollCounterFromRust(long enginePtr);

    private static native void readRowFromRust(long enginePtr, int row, char[] text, long[] styles);

    private static native int getRowsFromRust(long enginePtr);

    private static native int getColsFromRust(long enginePtr);

    private static native String getSelectedTextFromRust(long enginePtr, int x1, int y1, int x2, int y2);

    private static native void clearScrollCounterFromRust(long enginePtr);

    private static native boolean isAutoScrollDisabledFromRust(long enginePtr);

    private static native void toggleAutoScrollDisabledFromRust(long enginePtr);

    private static native void sendMouseEventFromRust(
        long enginePtr, int mouseButton, int column, int row, boolean pressed
    );

    private static native void sendKeyCodeFromRust(
        long enginePtr, int keyCode, String charStr, int metaState
    );

    private static native void pasteTextFromRust(long enginePtr, String text);

    private static native int getActiveTranscriptRowsFromRust(long enginePtr);

    private static native int[] getColorsFromRust(long enginePtr);

    private static native void resetColorsFromRust(long enginePtr);

    private static native void updateTerminalSessionClientFromRust(
        long enginePtr, TerminalSessionClient client
    );

    private static native void setCursorBlinkStateInRust(long enginePtr, boolean state);

    private static native void setCursorBlinkingEnabledInRust(long enginePtr, boolean enabled);
}
