package com.termux.terminal;

import android.os.Handler;
import android.os.Looper;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Objects;

/**
 * The terminal emulator.
 */
public final class TerminalEmulator {

    /** Log tag for terminal emulator. */
    private static final String LOG_TAG = "TerminalEmulator";

    /** The output to which terminal output is written. */
    private final TerminalOutput mSession;

    /** The screen buffer. */
    private TerminalBuffer mScreen;

    /** The main screen buffer. */
    private final TerminalBuffer mMainBuffer;

    /** The alternate screen buffer. */
    private final TerminalBuffer mAltBuffer;

    /** The client for terminal emulator. */
    private final TerminalSessionClient mClient;

    /** The number of rows in the terminal. */
    private int mRows;

    /** The number of columns in the terminal. */
    private int mColumns;

    /** The cursor row. */
    private int mCursorRow;

    /** The cursor column. */
    private int mCursorCol;

    /** The current style. */
    private int mCursorStyle;

    /** The current DECSET flags. */
    private int mCurrentDecSetFlags;

    /** If the cursor should blink. */
    private boolean mCursorBlinkingEnabled;

    /** If the cursor is currently in a visible state (on/off for blinking). */
    private boolean mCursorBlinkState;

    /** The current foreground, background and underline colors. */
    int mForeColor, mBackColor, mUnderlineColor;

    /** Current {@link TextStyle} effect. */
    int mEffect;

    /** The number of scrolled lines since last calling {@link #clearScrollCounter()}. */
    private int mScrollCounter = 0;

    /** If automatic scrolling of terminal is disabled. */
    private boolean mAutoScrollDisabled;

    /** Tab stops. */
    private boolean[] mTabStop;

    /** Terminal colors. */
    public final TerminalColors mColors = new TerminalColors();

    /** Current title. */
    private String mTitle;

    /** If in insert mode. */
    private boolean mInsertMode;

    // ========================================================================
    // Constants
    // ========================================================================

    private static final int TERMINAL_TRANSCRIPT_ROWS_MIN = 0;
    private static final int TERMINAL_TRANSCRIPT_ROWS_MAX = 50000;
    private static final int DEFAULT_TERMINAL_TRANSCRIPT_ROWS = 2000;

    public static final int DEFAULT_TERMINAL_CURSOR_STYLE = TerminalSessionClient.CURSOR_STYLE_BLOCK;
    public static final Integer[] TERMINAL_CURSOR_STYLES_LIST = new Integer[]{
            TerminalSessionClient.CURSOR_STYLE_BLOCK,
            TerminalSessionClient.CURSOR_STYLE_BAR,
            TerminalSessionClient.CURSOR_STYLE_UNDERLINE
    };

    /** DECSET bits. */
    private static final int DECSET_BIT_APPLICATION_CURSOR_KEYS = 1;
    private static final int DECSET_BIT_REVERSE_VIDEO = 1 << 1;
    private static final int DECSET_BIT_ORIGIN_MODE = 1 << 2;
    private static final int DECSET_BIT_AUTOWRAP = 1 << 3;
    private static final int DECSET_BIT_CURSOR_ENABLED = 1 << 4;
    private static final int DECSET_BIT_APPLICATION_KEYPAD = 1 << 5;
    private static final int DECSET_BIT_LEFTRIGHT_MARGIN_MODE = 1 << 6;
    private static final int DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE = 1 << 7;
    private static final int DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT = 1 << 8;
    private static final int DECSET_BIT_SEND_FOCUS_EVENTS = 1 << 9;
    private static final int DECSET_BIT_MOUSE_PROTOCOL_SGR = 1 << 10;
    private static final int DECSET_BIT_BRACKETED_PASTE_MODE = 1 << 11;

    // ========================================================================
    // Rust Full Takeover
    // ========================================================================

    /** 指向 Rust 引擎对象的原生指针 */
    private volatile long mRustEnginePtr = 0;

    /** 主线程 Handler */
    private final Handler mMainThreadHandler = new Handler(Looper.getMainLooper());

    /** 是否开启 Rust 全接管模式 */
    public static final boolean USE_RUST_FULL_TAKEOVER = true;

    /** 强制禁用 Rust 引擎的开关 (测试用) */
    public static boolean sForceDisableRust = false;

    /** 全量同步开关 (内容校验用) */
    public static boolean sEnableFullSyncForTests = false;

    public interface RustEngineCallback {
        void onScreenUpdate();
        void reportTitleChange(String title);
        void reportColorsChanged();
        void reportCursorVisibility(boolean visible);
        void onBell();
        void onCopyTextToClipboard(String text);
        void onPasteTextFromClipboard();
        void onWriteToSession(String data);
        void onWriteToSessionBytes(byte[] data);
        void reportColorResponse(String colorSpec);
        void reportTerminalResponse(String response);
    }

    // ========================================================================
    // Constructor and Lifecycle
    // ========================================================================

    public TerminalEmulator(TerminalOutput session, int columns, int rows, int cellWidthPixels, int cellHeightPixels, Integer transcriptRows, TerminalSessionClient client) {
        mSession = session;
        mScreen = mMainBuffer = new TerminalBuffer(columns, getTerminalTranscriptRows(transcriptRows), rows);
        mAltBuffer = new TerminalBuffer(columns, rows, rows);
        mClient = client;
        mRows = rows;
        mColumns = columns;
        mTabStop = new boolean[mColumns];
        reset();

        if (USE_RUST_FULL_TAKEOVER && JNI.sNativeLibrariesLoaded && !sForceDisableRust) {
            try {
                mRustEnginePtr = createEngineRustWithCallback(columns, rows, getTerminalTranscriptRows(transcriptRows), cellWidthPixels, cellHeightPixels, new RustEngineCallback() {
                    @Override
                    public void onScreenUpdate() {
                        mMainThreadHandler.post(() -> {
                            if (mClient != null && mSession instanceof TerminalSession) {
                                mClient.onTextChanged((TerminalSession) mSession);
                            }
                        });
                    }

                    @Override
                    public void reportTitleChange(String title) {
                        mMainThreadHandler.post(() -> {
                            mTitle = title;
                            if (mClient != null && mSession instanceof TerminalSession) {
                                mClient.onTitleChanged((TerminalSession) mSession);
                            }
                        });
                    }

                    @Override
                    public void reportColorsChanged() {
                        mMainThreadHandler.post(() -> {
                            syncColorsFromRust();
                            if (mClient != null && mSession instanceof TerminalSession) {
                                mClient.onColorsChanged((TerminalSession) mSession);
                            }
                        });
                    }

                    @Override
                    public void reportCursorVisibility(boolean visible) {
                        mMainThreadHandler.post(() -> {
                            if (mClient != null) {
                                mClient.onTerminalCursorStateChange(visible);
                            }
                        });
                    }

                    @Override
                    public void onBell() {
                        if (mSession != null) mSession.onBell();
                    }

                    @Override
                    public void onCopyTextToClipboard(String text) {
                        if (mSession != null) mSession.onCopyTextToClipboard(text);
                    }

                    @Override
                    public void onPasteTextFromClipboard() {
                        if (mSession != null) mSession.onPasteTextFromClipboard();
                    }

                    @Override
                    public void onWriteToSession(String data) {
                        if (mSession != null) mSession.write(data);
                    }

                    @Override
                    public void onWriteToSessionBytes(byte[] data) {
                        if (mSession != null) mSession.write(data);
                    }

                    @Override
                    public void reportColorResponse(String colorSpec) {
                        if (mSession != null) mSession.write("\u001b]" + colorSpec + "\u0007");
                    }

                    @Override
                    public void reportTerminalResponse(String response) {
                        if (mSession != null) mSession.write(response);
                    }
                });

                mScreen.setRustEnginePtr(mRustEnginePtr);
                mAltBuffer.setRustEnginePtr(mRustEnginePtr);
            } catch (Exception e) {
                android.util.Log.e(LOG_TAG, "Failed to initialize Rust engine", e);
                mRustEnginePtr = 0;
            }
        }
    }

    public void close() {
        long ptr = mRustEnginePtr;
        if (ptr != 0) {
            mRustEnginePtr = 0;
            destroyEngineRust(ptr);
        }
    }

    public void reset() {
        mCursorRow = mCursorCol = 0;
        mCurrentDecSetFlags = 0;
        mCurrentDecSetFlags |= DECSET_BIT_AUTOWRAP;
        mCurrentDecSetFlags |= DECSET_BIT_CURSOR_ENABLED;
        mForeColor = TextStyle.COLOR_INDEX_FOREGROUND;
        mBackColor = TextStyle.COLOR_INDEX_BACKGROUND;
        mEffect = 0;
        Arrays.fill(mTabStop, false);
        setDefaultTabStops();
    }

    private void setDefaultTabStops() {
        for (int i = 0; i < mColumns; i++) mTabStop[i] = (i & 7) == 0 && i != 0;
    }

    private int getTerminalTranscriptRows(Integer transcriptRows) {
        if (transcriptRows == null || transcriptRows < TERMINAL_TRANSCRIPT_ROWS_MIN || transcriptRows > TERMINAL_TRANSCRIPT_ROWS_MAX)
            return DEFAULT_TERMINAL_TRANSCRIPT_ROWS;
        return transcriptRows;
    }

    // ========================================================================
    // State Synchronization
    // ========================================================================

    private void syncColorsFromRust() {
        if (mRustEnginePtr != 0) {
            mForeColor = getForeColorFromRust(mRustEnginePtr);
            mBackColor = getBackColorFromRust(mRustEnginePtr);
            mEffect = getEffectFromRust(mRustEnginePtr);
        }
    }

    private void syncStateFromRust() {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            syncColorsFromRust();
            mCursorCol = getCursorColFromRust(mRustEnginePtr);
            mCursorRow = getCursorRowFromRust(mRustEnginePtr);
            mCurrentDecSetFlags = getDecsetFlagsFromRust(mRustEnginePtr);
            mInsertMode = isInsertModeActiveFromRust(mRustEnginePtr);
            
            if (sEnableFullSyncForTests) {
                int rows = mRows;
                char[][] text = new char[rows][mColumns];
                long[][] style = new long[rows][mColumns];
                readFullScreenFromRust(mRustEnginePtr, text, style);
                
                mScreen.setScreenFirstRow(0);
                for (int i = 0; i < rows; i++) {
                    TerminalRow row = mScreen.allocateFullLineIfNecessary(i);
                    System.arraycopy(text[i], 0, row.mText, 0, mColumns);
                    System.arraycopy(style[i], 0, row.mStyle, 0, mColumns);
                    row.updateStatusAfterBatchWrite();
                }
            }
        }
    }

    // ========================================================================
    // API Methods
    // ========================================================================

    public void append(byte[] buffer, int length) {
        long ptr = mRustEnginePtr;
        if (ptr != 0) {
            try {
                processEngineRust(ptr, buffer, 0, length);
                syncStateFromRust();
            } catch (Exception e) {
                android.util.Log.e(LOG_TAG, "Rust process error", e);
            }
        }
    }

    public void paste(String text) {
        long ptr = mRustEnginePtr;
        if (ptr != 0) {
            pasteTextFromRust(ptr, text);
        } else {
            // Mainline Java implementation fallback...
            boolean bracketed = isBracketedPasteMode();
            if (bracketed) mSession.write("\033[200~");
            mSession.write(text.replaceAll("\r?\n", "\r"));
            if (bracketed) mSession.write("\033[201~");
        }
    }

    public int getCursorRow() { return (mRustEnginePtr != 0) ? getCursorRowFromRust(mRustEnginePtr) : mCursorRow; }
    public int getCursorCol() { return (mRustEnginePtr != 0) ? getCursorColFromRust(mRustEnginePtr) : mCursorCol; }
    public boolean isBracketedPasteMode() { return (mCurrentDecSetFlags & DECSET_BIT_BRACKETED_PASTE_MODE) != 0; }
    public TerminalBuffer getScreen() { return mScreen; }

    // ========================================================================
    // Native Declarations
    // ========================================================================

    private static native long createEngineRustWithCallback(int cols, int rows, int totalRows, int cellWidth, int cellHeight, Object callbackObj);
    private static native void processEngineRust(long enginePtr, byte[] input, int offset, int length);
    private static native void destroyEngineRust(long enginePtr);
    private static native int getCursorColFromRust(long enginePtr);
    private static native int getCursorRowFromRust(long enginePtr);
    private static native int getForeColorFromRust(long enginePtr);
    private static native int getBackColorFromRust(long enginePtr);
    private static native int getEffectFromRust(long enginePtr);
    private static native int getDecsetFlagsFromRust(long enginePtr);
    private static native boolean isInsertModeActiveFromRust(long enginePtr);
    private static native void readFullScreenFromRust(long enginePtr, char[][] text, long[][] style);
    private static native void pasteTextFromRust(long enginePtr, String text);
}
