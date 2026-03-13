package com.termux.terminal;

import android.os.Handler;
import android.os.Looper;
import java.nio.ByteBuffer;
import java.nio.CharBuffer;
import java.nio.LongBuffer;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Objects;

/**
 * The terminal emulator.
 */
public final class TerminalEmulator {

    /** Log tag. */
    private static final String LOG_TAG = "TerminalEmulator";

    // --- Constants ---
    
    public static final int MOUSE_LEFT_BUTTON = 0;
    public static final int MOUSE_MIDDLE_BUTTON = 1;
    public static final int MOUSE_RIGHT_BUTTON = 2;
    public static final int MOUSE_LEFT_BUTTON_MOVED = 32;
    public static final int MOUSE_MIDDLE_BUTTON_MOVED = 33;
    public static final int MOUSE_RIGHT_BUTTON_MOVED = 34;
    public static final int MOUSE_WHEELUP_BUTTON = 64;
    public static final int MOUSE_WHEELDOWN_BUTTON = 65;
    
    public static final int UNICODE_REPLACEMENT_CHAR = 0xFFFD;

    public static final int TERMINAL_CURSOR_STYLE_BLOCK = 0;
    public static final int TERMINAL_CURSOR_STYLE_UNDERLINE = 1;
    public static final int TERMINAL_CURSOR_STYLE_BAR = 2;
    public static final int DEFAULT_TERMINAL_CURSOR_STYLE = TERMINAL_CURSOR_STYLE_BLOCK;

    public static final int TERMINAL_TRANSCRIPT_ROWS_MIN = 0;
    public static final int TERMINAL_TRANSCRIPT_ROWS_MAX = 50000;
    public static final int DEFAULT_TERMINAL_TRANSCRIPT_ROWS = 2000;

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

    // --- Fields ---

    private final TerminalOutput mSession;
    private TerminalBuffer mScreen;
    private final TerminalBuffer mMainBuffer;
    public final TerminalBuffer mAltBuffer;
    private TerminalSessionClient mClient;

    public int mRows;
    public int mColumns;
    private int mCursorRow;
    private int mCursorCol;
    private int mCursorStyle;
    private int mCurrentDecSetFlags;
    private boolean mCursorBlinkingEnabled;
    private boolean mCursorBlinkState;

    int mForeColor, mBackColor, mUnderlineColor;
    int mEffect;
    private int mScrollCounter = 0;
    private boolean mAutoScrollDisabled;
    private boolean[] mTabStop;
    public final TerminalColors mColors = new TerminalColors();
    private String mTitle;
    private boolean mInsertMode;
    private boolean mIsSyncingFromRust = false;  // 防止递归调用

    // ========================================================================
    // Rust Takeover
    // ========================================================================

    private volatile long mRustEnginePtr = 0;
    private ByteBuffer mSharedBuffer;
    private final Handler mMainThreadHandler = new Handler(Looper.getMainLooper());
    public static final boolean USE_RUST_FULL_TAKEOVER = true;
    public static boolean sForceDisableRust = false;
    public static String sLastLoadStatus = "UNKNOWN";

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

    public TerminalEmulator(TerminalOutput session, int columns, int rows, int cellWidthPixels, int cellHeightPixels, Integer transcriptRows, TerminalSessionClient client) {
        sLastLoadStatus = "CALLED: JNI_LOADED=" + JNI.sNativeLibrariesLoaded + (USE_RUST_FULL_TAKEOVER ? " USE_RUST=true" : "");
        mSession = session;
        int actualTranscriptRows = (transcriptRows != null ? transcriptRows : DEFAULT_TERMINAL_TRANSCRIPT_ROWS);
        mScreen = mMainBuffer = new TerminalBuffer(columns, actualTranscriptRows, rows);
        mAltBuffer = new TerminalBuffer(columns, rows, rows);
        mMainBuffer.setEmulator(this);
        mAltBuffer.setEmulator(this);
        mClient = client;
        mRows = rows;
        mColumns = columns;
        mTabStop = new boolean[mColumns];
        reset();

        if (USE_RUST_FULL_TAKEOVER && JNI.sNativeLibrariesLoaded && !sForceDisableRust) {
            try {
                mRustEnginePtr = createEngineRustWithCallback(columns, rows, actualTranscriptRows, cellWidthPixels, cellHeightPixels, new RustEngineCallback() {
                    @Override public void onScreenUpdate() { 
                        mMainThreadHandler.post(() -> { if (mClient != null && mSession instanceof TerminalSession) mClient.onTextChanged((TerminalSession) mSession); }); 
                    }
                    @Override public void reportTitleChange(String title) { mMainThreadHandler.post(() -> { mTitle = title; if (mClient != null && mSession instanceof TerminalSession) mClient.onTitleChanged((TerminalSession) mSession); }); }
                    @Override public void reportColorsChanged() { mMainThreadHandler.post(() -> { syncColorsFromRust(); if (mClient != null && mSession instanceof TerminalSession) mClient.onColorsChanged((TerminalSession) mSession); }); }
                    @Override public void reportCursorVisibility(boolean visible) { mMainThreadHandler.post(() -> { if (mClient != null) mClient.onTerminalCursorStateChange(visible); }); }
                    @Override public void onBell() { if (mSession != null) mSession.onBell(); }
                    @Override public void onCopyTextToClipboard(String text) { if (mSession != null) mSession.onCopyTextToClipboard(text); }
                    @Override public void onPasteTextFromClipboard() { if (mSession != null) mSession.onPasteTextFromClipboard(); }
                    @Override public void onWriteToSession(String data) { if (mSession != null) mSession.write(data); }
                    @Override public void onWriteToSessionBytes(byte[] data) { if (mSession != null) mSession.write(data); }
                    @Override public void reportColorResponse(String colorSpec) { if (mSession != null) mSession.write("\u001b]" + colorSpec + "\u0007"); }
                    @Override public void reportTerminalResponse(String response) { if (mSession != null) mSession.write(response); }
                });
                mMainBuffer.setRustEnginePtr(mRustEnginePtr);
                mAltBuffer.setRustEnginePtr(mRustEnginePtr);
                mSharedBuffer = createSharedBufferRust(mRustEnginePtr);
            } catch (Exception e) {
                mRustEnginePtr = 0;
            }
        }
    }

    public void reset() {
        mCursorRow = mCursorCol = 0;
        mCurrentDecSetFlags = DECSET_BIT_AUTOWRAP | DECSET_BIT_CURSOR_ENABLED;
        mForeColor = TextStyle.COLOR_INDEX_FOREGROUND;
        mBackColor = TextStyle.COLOR_INDEX_BACKGROUND;
        mEffect = 0;
        Arrays.fill(mTabStop, false);
        for (int i = 0; i < mColumns; i++) mTabStop[i] = (i & 7) == 0 && i != 0;
    }

    private void syncColorsFromRust() {
        if (mRustEnginePtr != 0) {
            try { mForeColor = getForeColorFromRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            try { mBackColor = getBackColorFromRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            try { mEffect = getEffectFromRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
        }
    }

    /** Aggressively synchronizes state from Rust to Java. */
    public synchronized void syncStateFromRustIfRequired() {
        // 防止递归调用：如果已经在 syncStateFromRust 中，直接返回
        if (mIsSyncingFromRust) {
            return;
        }
        if (mRustEnginePtr != 0) {
            syncStateFromRust();
        }
    }

    private void syncStateFromRust() {
        if (mRustEnginePtr == 0 || mIsSyncingFromRust) {
            return;
        }

        mIsSyncingFromRust = true;  // 标记开始同步

        try {
            syncColorsFromRust();
            try { mCursorCol = getCursorColFromRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            try { mCursorRow = getCursorRowFromRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            try { mCurrentDecSetFlags = getDecsetFlagsFromRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            try { mInsertMode = isInsertModeActiveFromRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }

            if (mSharedBuffer == null) {
                try { mSharedBuffer = createSharedBufferRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            }

            if (mSharedBuffer != null) {
                try { syncToSharedBufferRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }

                int rows = mRows;
                int cols = mColumns;
                int cellCount = rows * cols;

                TerminalBuffer targetBuffer = isAlternateBufferActive() ? mAltBuffer : mMainBuffer;
                mScreen = targetBuffer;
                targetBuffer.setScreenFirstRow(0);

                mSharedBuffer.clear();
                mSharedBuffer.position(12);
                CharBuffer textChars = mSharedBuffer.asCharBuffer();

                mSharedBuffer.clear();
                mSharedBuffer.position(12 + cellCount * 2);
                LongBuffer styleLongs = mSharedBuffer.asLongBuffer();

                for (int i = 0; i < rows; i++) {
                    TerminalRow row = targetBuffer.allocateFullLineIfNecessary(i);
                    textChars.position(i * cols);
                    textChars.get(row.mText, 0, cols);

                    styleLongs.position(i * cols);
                    styleLongs.get(row.mStyle, 0, cols);

                    row.updateStatusAfterBatchWrite();
                }
                try { clearSharedBufferVersionRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            }
        } catch (Exception e) {
            // 捕获任何意外异常，防止崩溃
        } finally {
            mIsSyncingFromRust = false;  // 重置标志
        }
    }

    public void updateTerminalSessionClient(TerminalSessionClient client) {
        this.mClient = client;
        if (mRustEnginePtr != 0) {
            try {
                updateTerminalSessionClientFromRust(mRustEnginePtr, client);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    public void resize(int columns, int rows, int cellWidthPixels, int cellHeightPixels) {
        this.mColumns = columns;
        this.mRows = rows;
        if (mRustEnginePtr != 0) {
            try { resizeEngineRustFull(mRustEnginePtr, columns, rows); } catch (UnsatisfiedLinkError e) { }
            try { mSharedBuffer = createSharedBufferRust(mRustEnginePtr); } catch (UnsatisfiedLinkError e) { }
            syncStateFromRust();
        }
    }

    public String getTitle() {
        if (mRustEnginePtr != 0) {
            try {
                return getTitleFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return mTitle;
            }
        }
        return mTitle;
    }

    public void append(byte[] buffer, int length) {
        if (mRustEnginePtr != 0) {
            try {
                processEngineRust(mRustEnginePtr, buffer, 0, length);
                syncStateFromRust();
            } catch (Exception e) { }
        }
    }

    public void paste(String text) {
        if (mRustEnginePtr != 0) {
            try {
                pasteTextFromRust(mRustEnginePtr, text);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    // Proxy methods for TerminalView
    public boolean isMouseTrackingActive() {
        if (mRustEnginePtr != 0) {
            try {
                return isMouseTrackingActiveFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                // Native method not found, fall back to false
                return false;
            }
        }
        return false;
    }

    public boolean isAlternateBufferActive() {
        if (mRustEnginePtr != 0) {
            try {
                return isAlternateBufferActiveFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return false;
            }
        }
        return false;
    }

    public boolean isAutoScrollDisabled() {
        if (mRustEnginePtr != 0) {
            try {
                return isAutoScrollDisabledFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return mAutoScrollDisabled;
            }
        }
        return mAutoScrollDisabled;
    }

    public void toggleAutoScrollDisabled() {
        mAutoScrollDisabled = !mAutoScrollDisabled;
        if (mRustEnginePtr != 0) {
            try {
                setAutoScrollDisabledInRust(mRustEnginePtr, mAutoScrollDisabled);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    public int getScrollCounter() {
        if (mRustEnginePtr != 0) {
            try {
                return getScrollCounterFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return mScrollCounter;
            }
        }
        return mScrollCounter;
    }

    public void clearScrollCounter() {
        if (mRustEnginePtr != 0) {
            try {
                clearScrollCounterFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        } else {
            mScrollCounter = 0;
        }
    }

    public void sendMouseEvent(int button, int x, int y, boolean pressed) {
        if (mRustEnginePtr != 0) {
            try {
                sendMouseEventToRust(mRustEnginePtr, button, x, y, pressed);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    public void setCursorBlinkState(boolean visible) {
        mCursorBlinkState = visible;
        if (mRustEnginePtr != 0) {
            try {
                setCursorBlinkStateInRust(mRustEnginePtr, visible);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    public void setCursorBlinkingEnabled(boolean enabled) {
        mCursorBlinkingEnabled = enabled;
        if (mRustEnginePtr != 0) {
            try {
                setCursorBlinkingEnabledInRust(mRustEnginePtr, enabled);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    public boolean isCursorEnabled() {
        if (mRustEnginePtr != 0) {
            try {
                return isCursorEnabledFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return true;
            }
        }
        return true;
    }

    public boolean isReverseVideo() {
        if (mRustEnginePtr != 0) {
            try {
                return isReverseVideoFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return false;
            }
        }
        return false;
    }

    public boolean shouldCursorBeVisible() {
        if (mRustEnginePtr != 0) {
            try {
                return shouldCursorBeVisibleFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return true;
            }
        }
        return true;
    }

    public int getCursorStyle() {
        if (mRustEnginePtr != 0) {
            try {
                return getCursorStyleFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return mCursorStyle;
            }
        }
        return mCursorStyle;
    }

    public boolean isCursorKeysApplicationMode() {
        if (mRustEnginePtr != 0) {
            try {
                return isCursorKeysApplicationModeFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return false;
            }
        }
        return false;
    }

    public boolean isKeypadApplicationMode() {
        if (mRustEnginePtr != 0) {
            try {
                return isKeypadApplicationModeFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return false;
            }
        }
        return false;
    }

    public void syncScreenBatchFromRust(int startRow, int numRows) {
        if (mRustEnginePtr != 0) {
            try {
                char[][] text = new char[numRows][mColumns];
                long[][] style = new long[numRows][mColumns];
                readScreenBatchFromRust(mRustEnginePtr, text, style, startRow, numRows);
                TerminalBuffer targetBuffer = isAlternateBufferActive() ? mAltBuffer : mMainBuffer;
                for (int i = 0; i < numRows; i++) {
                    TerminalRow row = targetBuffer.allocateFullLineIfNecessary(startRow + i);
                    System.arraycopy(text[i], 0, row.mText, 0, mColumns);
                    System.arraycopy(style[i], 0, row.mStyle, 0, mColumns);
                    row.updateStatusAfterBatchWrite();
                }
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    public void getRowContent(int row, char[] text, long[] style) {
        syncStateFromRustIfRequired();
        TerminalBuffer targetBuffer = isAlternateBufferActive() ? mAltBuffer : mMainBuffer;
        TerminalRow terminalRow = targetBuffer.allocateFullLineIfNecessary(row);
        System.arraycopy(terminalRow.mText, 0, text, 0, mColumns);
        System.arraycopy(terminalRow.mStyle, 0, style, 0, mColumns);
    }

    public String getSelectedText(int x1, int y1, int x2, int y2) {
        syncStateFromRustIfRequired();
        return mScreen.getSelectedText(x1, y1, x2, y2);
    }

    public int getCursorRow() {
        syncStateFromRustIfRequired();
        if (mRustEnginePtr != 0) {
            try {
                return getCursorRowFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return mCursorRow;
            }
        }
        return mCursorRow;
    }

    public int getCursorCol() {
        syncStateFromRustIfRequired();
        if (mRustEnginePtr != 0) {
            try {
                return getCursorColFromRust(mRustEnginePtr);
            } catch (UnsatisfiedLinkError e) {
                return mCursorCol;
            }
        }
        return mCursorCol;
    }
    
    public boolean isBracketedPasteMode() { return (mCurrentDecSetFlags & DECSET_BIT_BRACKETED_PASTE_MODE) != 0; }
    public TerminalBuffer getScreen() { return mScreen; }
    public void destroy() {
        if (mRustEnginePtr != 0) {
            long p = mRustEnginePtr;
            mRustEnginePtr = 0;
            try {
                destroyEngineRust(p);
            } catch (UnsatisfiedLinkError e) { /* Ignore */ }
        }
    }

    // Native Declarations
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
    public static native void readFullScreenFromRust(long enginePtr, char[][] text, long[][] style);
    public static native void readScreenBatchFromRust(long enginePtr, char[][] text, long[][] style, int startRow, int numRows);
    private static native void pasteTextFromRust(long enginePtr, String text);
    private static native void updateTerminalSessionClientFromRust(long enginePtr, Object client);
    private static native void resizeEngineRustFull(long enginePtr, int cols, int rows);
    private static native String getTitleFromRust(long enginePtr);
    public static native int getActiveTranscriptRowsFromRust(long enginePtr);
    private static native boolean isMouseTrackingActiveFromRust(long enginePtr);
    private static native boolean isAlternateBufferActiveFromRust(long enginePtr);
    private static native boolean isAutoScrollDisabledFromRust(long enginePtr);
    private static native void setAutoScrollDisabledInRust(long enginePtr, boolean disabled);
    private static native int getScrollCounterFromRust(long enginePtr);
    private static native void clearScrollCounterFromRust(long enginePtr);
    private static native void sendMouseEventToRust(long enginePtr, int button, int x, int y, boolean pressed);
    private static native void setCursorBlinkStateInRust(long enginePtr, boolean visible);
    private static native void setCursorBlinkingEnabledInRust(long enginePtr, boolean enabled);
    private static native boolean isCursorEnabledFromRust(long enginePtr);
    private static native boolean isReverseVideoFromRust(long enginePtr);
    private static native boolean shouldCursorBeVisibleFromRust(long enginePtr);
    private static native int getCursorStyleFromRust(long enginePtr);
    private static native boolean isCursorKeysApplicationModeFromRust(long enginePtr);
    private static native boolean isKeypadApplicationModeFromRust(long enginePtr);

    // Phase 2: DirectByteBuffer Shared Buffer Support
    private static native java.nio.ByteBuffer createSharedBufferRust(long enginePtr);
    private static native void syncToSharedBufferRust(long enginePtr);
    private static native boolean getSharedBufferVersionRust(long enginePtr);
    private static native void clearSharedBufferVersionRust(long enginePtr);
    private static native void destroySharedBufferRust(long enginePtr);
}
