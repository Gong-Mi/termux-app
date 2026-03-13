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

    /** Log tag. */
    private static final String LOG_TAG = "TerminalEmulator";

    private final TerminalOutput mSession;
    private TerminalBuffer mScreen;
    private final TerminalBuffer mMainBuffer;
    private final TerminalBuffer mAltBuffer;
    private final TerminalSessionClient mClient;

    private int mRows;
    private int mColumns;
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

    // ========================================================================
    // Rust Takeover
    // ========================================================================

    private volatile long mRustEnginePtr = 0;
    private final Handler mMainThreadHandler = new Handler(Looper.getMainLooper());
    public static final boolean USE_RUST_FULL_TAKEOVER = true;
    public static boolean sForceDisableRust = false;
    public static String sLastLoadStatus = "UNKNOWN";

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
        sLastLoadStatus = "CALLED: JNI_LOADED=" + JNI.sNativeLibrariesLoaded;
        mSession = session;
        mScreen = mMainBuffer = new TerminalBuffer(columns, (transcriptRows != null ? transcriptRows : 2000), rows);
        mAltBuffer = new TerminalBuffer(columns, rows, rows);
        mClient = client;
        mRows = rows;
        mColumns = columns;
        mTabStop = new boolean[mColumns];
        reset();

        if (USE_RUST_FULL_TAKEOVER && JNI.sNativeLibrariesLoaded && !sForceDisableRust) {
            try {
                mRustEnginePtr = createEngineRustWithCallback(columns, rows, (transcriptRows != null ? transcriptRows : 2000), cellWidthPixels, cellHeightPixels, new RustEngineCallback() {
                    @Override public void onScreenUpdate() { mMainThreadHandler.post(() -> { if (mClient != null && mSession instanceof TerminalSession) mClient.onTextChanged((TerminalSession) mSession); }); }
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
                mScreen.setRustEnginePtr(mRustEnginePtr);
                mAltBuffer.setRustEnginePtr(mRustEnginePtr);
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
            mForeColor = getForeColorFromRust(mRustEnginePtr);
            mBackColor = getBackColorFromRust(mRustEnginePtr);
            mEffect = getEffectFromRust(mRustEnginePtr);
        }
    }

    private void syncStateFromRust() {
        if (mRustEnginePtr != 0) {
            syncColorsFromRust();
            mCursorCol = getCursorColFromRust(mRustEnginePtr);
            mCursorRow = getCursorRowFromRust(mRustEnginePtr);
            mCurrentDecSetFlags = getDecsetFlagsFromRust(mRustEnginePtr);
            mInsertMode = isInsertModeActiveFromRust(mRustEnginePtr);
            
            // 全屏物理同步，确保单元测试可见内容
            int rows = mRows;
            int cols = mColumns;
            char[][] text = new char[rows][cols];
            long[][] style = new long[rows][cols];
            readFullScreenFromRust(mRustEnginePtr, text, style);
            
            mScreen.setScreenFirstRow(0);
            for (int i = 0; i < rows; i++) {
                TerminalRow row = mScreen.allocateFullLineIfNecessary(i);
                System.arraycopy(text[i], 0, row.mText, 0, cols);
                System.arraycopy(style[i], 0, row.mStyle, 0, cols);
                row.updateStatusAfterBatchWrite();
            }
        }
    }

    public void append(byte[] buffer, int length) {
        long ptr = mRustEnginePtr;
        if (ptr != 0) {
            try {
                processEngineRust(ptr, buffer, 0, length);
                syncStateFromRust();
            } catch (Exception e) { }
        }
    }

    public void paste(String text) {
        if (mRustEnginePtr != 0) pasteTextFromRust(mRustEnginePtr, text);
    }

    public int getCursorRow() { return (mRustEnginePtr != 0) ? getCursorRowFromRust(mRustEnginePtr) : mCursorRow; }
    public int getCursorCol() { return (mRustEnginePtr != 0) ? getCursorColFromRust(mRustEnginePtr) : mCursorCol; }
    public boolean isBracketedPasteMode() { return (mCurrentDecSetFlags & DECSET_BIT_BRACKETED_PASTE_MODE) != 0; }
    public TerminalBuffer getScreen() { return mScreen; }
    public void destroy() { if (mRustEnginePtr != 0) { long p = mRustEnginePtr; mRustEnginePtr = 0; destroyEngineRust(p); } }

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
    private static native void readFullScreenFromRust(long enginePtr, char[][] text, long[][] style);
    private static native void pasteTextFromRust(long enginePtr, String text);
}
