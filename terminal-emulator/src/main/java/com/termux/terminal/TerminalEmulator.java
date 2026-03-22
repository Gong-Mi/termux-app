package com.termux.terminal;

/**
 * Terminal Emulator - Rust 实现的 Java 包装类
 * 所有实际逻辑都在 Rust 中实现，此类仅包含 JNI 调用
 */
public final class TerminalEmulator {

    static {
        System.loadLibrary("termux_rust");
    }

    // 原生引擎指针
    private long mEnginePtr;
    // 持有回调对象的强引用，防止被 GC 回收
    private final RustEngineCallback mRustCallback;

    // --- 静态常量定义 (与旧版 Java 保持一致以兼容 UI 层) ---
    public static final int TERMINAL_CURSOR_STYLE_BLOCK = 0;
    public static final int TERMINAL_CURSOR_STYLE_BAR = 1;
    public static final int TERMINAL_CURSOR_STYLE_UNDERLINE = 2;

    /** 鼠标按键定义 */
    public static final int MOUSE_LEFT_BUTTON = 0;
    public static final int MOUSE_MIDDLE_BUTTON = 1;
    public static final int MOUSE_RIGHT_BUTTON = 2;
    public static final int MOUSE_LEFT_BUTTON_MOVED = 32;
    public static final int MOUSE_WHEELUP_BUTTON = 64;
    public static final int MOUSE_WHEELDOWN_BUTTON = 65;

    /** Unicode 替换字符 */
    public static final int UNICODE_REPLACEMENT_CHAR = 0xFFFD;

    /** 默认滚动历史行数 */
    public static final int DEFAULT_TERMINAL_TRANSCRIPT_ROWS = 600;
    /** 最小滚动历史行数 */
    public static final int TERMINAL_TRANSCRIPT_ROWS_MIN = 0;
    /** 最大滚动历史行数 */
    public static final int TERMINAL_TRANSCRIPT_ROWS_MAX = 50000;
    /** 默认光标样式 */
    public static final int DEFAULT_TERMINAL_CURSOR_STYLE = TERMINAL_CURSOR_STYLE_BLOCK;

    /**
     * 初始化终端引擎
     */
    public TerminalEmulator(TerminalOutput session, int columns, int rows, 
                           int cellWidthPixels, int cellHeightPixels, 
                           Integer transcriptRows, TerminalSessionClient client) {
        // 创建独立的回调对象并持有强引用
        this.mRustCallback = new RustEngineCallback(client);
        
        mEnginePtr = createEngineRustWithCallback(
            columns, rows, cellWidthPixels, cellHeightPixels, 
            transcriptRows != null ? transcriptRows : 600,
            mRustCallback
        );
    }

    /**
     * 处理输入数据
     */
    public synchronized void append(byte[] batch, int length) {
        if (mEnginePtr != 0) {
            try {
                processBatchRust(mEnginePtr, batch, length);
            } catch (Exception e) {
                android.util.Log.e("Termux-JNI", "Error in processBatchRust", e);
            }
        }
    }

    /**
     * 处理单个 Unicode 码点
     * @param codePoint Unicode 码点
     */
    public synchronized void processCodePoint(int codePoint) {
        if (mEnginePtr != 0) {
            try {
                processCodePointRust(mEnginePtr, codePoint);
            } catch (Exception e) {
                android.util.Log.e("Termux-JNI", "Error in processCodePointRust", e);
            }
        }
    }

    /**
     * 检查终端引擎是否仍然有效（未被销毁）
     */
    public synchronized boolean isAlive() {
        return mEnginePtr != 0;
    }

    /**
     * 调整终端大小
     */
    public synchronized void resize(int columns, int rows, int cellWidthPixels, int cellHeightPixels) {
        if (mEnginePtr != 0) {
            resizeEngineRustFull(mEnginePtr, columns, rows, cellWidthPixels, cellHeightPixels);
        }
    }

    public void updateTerminalSessionClient(TerminalSessionClient client) {
        // 更新回调对象中的 client 引用
        if (mRustCallback != null) {
            // 注意：如果 RustEngineCallback 提供了更新方法，在这里调用
        }
        if (mEnginePtr != 0) {
            updateTerminalSessionClientFromRust(mEnginePtr, client);
        }
    }

    public String getTitle() {
        if (mEnginePtr != 0) {
            return getTitleFromRust(mEnginePtr);
        }
        return null;
    }

    public void reset() {
        resetColors();
    }

    /**
     * 设置终端光标样式
     * @param cursorStyle 光标样式 (0=块，1=下划线，2=竖条)
     */
    public void setCursorStyle(int cursorStyle) {
        if (mEnginePtr != 0) {
            setCursorStyleFromRust(mEnginePtr, cursorStyle);
        }
    }

    /**
     * 执行 DECSET/DECRST 命令（设置/重置 DEC 私有模式）
     * @param setting true=DECSET, false=DECRST
     * @param mode DEC 模式编号
     */
    public void doDecSetOrReset(boolean setting, int mode) {
        if (mEnginePtr != 0) {
            doDecSetOrResetFromRust(mEnginePtr, setting, mode);
        }
    }

    /** 获取光标列 */
    public int getCursorCol() {
        if (mEnginePtr != 0) return getCursorColFromRust(mEnginePtr);
        return 0;
    }

    /** 获取光标行 */
    public int getCursorRow() {
        if (mEnginePtr != 0) return getCursorRowFromRust(mEnginePtr);
        return 0;
    }

    /** 获取光标样式 */
    public int getCursorStyle() {
        if (mEnginePtr != 0) return getCursorStyleFromRust(mEnginePtr);
        return TERMINAL_CURSOR_STYLE_BLOCK;
    }

    public void setCursorBlinkState(boolean state) {
        if (mEnginePtr != 0) setCursorBlinkStateInRust(mEnginePtr, state);
    }

    public void setCursorBlinkingEnabled(boolean enabled) {
        if (mEnginePtr != 0) setCursorBlinkingEnabledInRust(mEnginePtr, enabled);
    }

    public boolean isCursorEnabled() {
        if (mEnginePtr != 0) return isCursorEnabledFromRust(mEnginePtr);
        return true;
    }

    /** 光标是否应可见 */
    public boolean shouldCursorBeVisible() {
        if (mEnginePtr != 0) return shouldCursorBeVisibleFromRust(mEnginePtr);
        return true;
    }

    /** 终端是否处于反色模式 */
    public boolean isReverseVideo() {
        if (mEnginePtr != 0) return isReverseVideoFromRust(mEnginePtr);
        return false;
    }

    /** 备用缓冲区是否处于活动状态 */
    public boolean isAlternateBufferActive() {
        if (mEnginePtr != 0) return isAlternateBufferActiveFromRust(mEnginePtr);
        return false;
    }

    /** 是否处于应用光标键模式 */
    public boolean isCursorKeysApplicationMode() {
        if (mEnginePtr != 0) return isCursorKeysApplicationModeFromRust(mEnginePtr);
        return false;
    }

    /** 是否处于应用小键盘模式 */
    public boolean isKeypadApplicationMode() {
        if (mEnginePtr != 0) return isKeypadApplicationModeFromRust(mEnginePtr);
        return false;
    }

    /** 鼠标跟踪是否处于活动状态 */
    public boolean isMouseTrackingActive() {
        if (mEnginePtr != 0) return isMouseTrackingActiveFromRust(mEnginePtr);
        return false;
    }

    /** 滚动历史行数 */
    public int getScrollCounter() {
        if (mEnginePtr != 0) return getScrollCounterFromRust(mEnginePtr);
        return 0;
    }

    /** 清除滚动计数 */
    public void clearScrollCounter() {
        if (mEnginePtr != 0) clearScrollCounterFromRust(mEnginePtr);
    }

    /** 总行数 (含显示) */
    public int getRows() {
        if (mEnginePtr != 0) return getRowsFromRust(mEnginePtr);
        return 0;
    }

    /** 总列数 */
    public int getCols() {
        if (mEnginePtr != 0) return getColsFromRust(mEnginePtr);
        return 0;
    }

    /** 活动历史行数 (不含显示区域) */
    public int getActiveTranscriptRows() {
        if (mEnginePtr != 0) return getActiveTranscriptRowsFromRust(mEnginePtr);
        return 0;
    }

    /** 活动行数 (历史 + 显示) */
    public int getActiveRows() {
        return getActiveTranscriptRows() + getRows();
    }

    /** 是否禁用自动滚动 */
    public boolean isAutoScrollDisabled() {
        if (mEnginePtr != 0) return isAutoScrollDisabledFromRust(mEnginePtr);
        return false;
    }

    /** 切换自动滚动禁用状态 */
    public void toggleAutoScrollDisabled() {
        if (mEnginePtr != 0) toggleAutoScrollDisabledFromRust(mEnginePtr);
    }

    /** 读取一行的数据（用于渲染） */
    public void readRow(int row, char[] text, long[] styles) {
        if (mEnginePtr != 0) {
            readRowFromRust(mEnginePtr, row, text, styles);
        }
    }

    /** 获取选定区域的文本 */
    public String getSelectedText(int x1, int y1, int x2, int y2) {
        if (mEnginePtr != 0) {
            return getSelectedTextFromRust(mEnginePtr, x1, y1, x2, y2);
        }
        return "";
    }

    /** 获取指定位置的单词 */
    public String getWordAtLocation(int x, int y) {
        if (mEnginePtr != 0) {
            return getWordAtLocationFromRust(mEnginePtr, x, y);
        }
        return "";
    }

    /** 获取所有文字 */
    public String getTranscriptText() {
        if (mEnginePtr != 0) {
            return getTranscriptTextFromRust(mEnginePtr);
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

    /** 发送鼠标事件 */
    public void sendMouseEvent(int button, int col, int row, boolean pressed) {
        if (mEnginePtr != 0) {
            sendMouseEventFromRust(mEnginePtr, button, col, row, pressed);
        }
    }

    /** 发送按键事件 */
    public void sendKeyEvent(int keyCode, int metaState) {
        if (mEnginePtr != 0) {
            sendKeyCodeFromRust(mEnginePtr, keyCode, null, metaState);
        }
    }

    /** 发送字符输入 */
    public void sendCharEvent(char c, int metaState) {
        if (mEnginePtr != 0) {
            sendKeyCodeFromRust(mEnginePtr, 0, String.valueOf(c), metaState);
        }
    }

    /** 粘贴文本 */
    public void paste(String text) {
        if (mEnginePtr != 0) {
            pasteTextFromRust(mEnginePtr, text);
        }
    }

    public void resetColors() {
        if (mEnginePtr != 0) {
            resetColorsFromRust(mEnginePtr);
        }
    }

    /**
     * 获取终端缓冲区（兼容性方法）
     * Rust 版本使用共享内存访问屏幕数据，此方法返回 null
     * @deprecated 直接使用 readRow() 方法访问屏幕数据
     */
    @Deprecated
    public TerminalBuffer getScreen() {
        // Rust 版本使用共享内存，不创建 TerminalBuffer 对象
        // 如果需要访问屏幕数据，请使用 readRow() 方法
        return null;
    }

    public synchronized void destroy() {
        if (mEnginePtr != 0) {
            destroyEngineRust(mEnginePtr);
            mEnginePtr = 0;
        }
    }

    /**
     * 返回终端模拟器的字符串表示（用于调试）
     */
    @Override
    public String toString() {
        if (mEnginePtr == 0) {
            return "TerminalEmulator[destroyed]";
        }
        return String.format(java.util.Locale.US,
            "TerminalEmulator[cursor=(%d,%d),style=%d,size=%dx%d,rows=%d,cols=%d,alt=%b]",
            getCursorRow(), getCursorCol(), getCursorStyle(),
            getRows(), getCols(), getActiveRows(), getActiveTranscriptRows(),
            isAlternateBufferActive());
    }

    // --- Native 接口 ---
    private static native long createEngineRustWithCallback(
        int cols, int rows, int cw, int ch, int totalRows, RustEngineCallback callback
    );

    private static native void destroyEngineRust(long enginePtr);

    private static native void processBatchRust(long enginePtr, byte[] batch, int length);

    private static native void processCodePointRust(long enginePtr, int codePoint);

    private static native void resizeEngineRustFull(
        long enginePtr, int cols, int rows, int cw, int ch
    );

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
    private static native void readRowFromRust(long enginePtr, int row, char[] text, long[] styles);
    private static native String getSelectedTextFromRust(long enginePtr, int x1, int y1, int x2, int y2);
    private static native String getWordAtLocationFromRust(long enginePtr, int x, int y);
    private static native String getTranscriptTextFromRust(long enginePtr);
    private static native String getTitleFromRust(long enginePtr);

    private static native void sendMouseEventFromRust(
        long enginePtr, int button, int col, int row, boolean pressed
    );

    private static native void sendKeyCodeFromRust(
        long enginePtr, int keyCode, String charStr, int metaState
    );

    private static native void pasteTextFromRust(long enginePtr, String text);
    private static native int[] getColorsFromRust(long enginePtr);
    private static native void resetColorsFromRust(long enginePtr);
    private static native void updateTerminalSessionClientFromRust(long enginePtr, TerminalSessionClient client);
    private static native void setCursorBlinkStateInRust(long enginePtr, boolean state);
    private static native void setCursorBlinkingEnabledInRust(long enginePtr, boolean enabled);
}
