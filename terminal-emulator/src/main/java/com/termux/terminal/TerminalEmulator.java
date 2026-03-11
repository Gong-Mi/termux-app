package com.termux.terminal;

import android.util.Base64;

import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Locale;
import java.util.Objects;
import java.util.Stack;

/**
 * Renders text into a screen. Contains all the terminal-specific knowledge and state. Emulates a subset of the X Window
 * System xterm terminal, which in turn is an emulator for a subset of the Digital Equipment Corporation vt100 terminal.
 */
public final class TerminalEmulator {

    /** Log unknown or unimplemented escape sequences received from the shell process. */
    private static final boolean LOG_ESCAPE_SEQUENCES = false;

    public static final int MOUSE_LEFT_BUTTON = 0;

    /** Mouse moving while having left mouse button pressed. */
    public static final int MOUSE_LEFT_BUTTON_MOVED = 32;
    public static final int MOUSE_WHEELUP_BUTTON = 64;
    public static final int MOUSE_WHEELDOWN_BUTTON = 65;

    /** Used for invalid data - http://en.wikipedia.org/wiki/Replacement_character#Replacement_character */
    public static final int UNICODE_REPLACEMENT_CHAR = 0xFFFD;

    /** Escape processing: Not currently in an escape sequence. */
    private static final int ESC_NONE = 0;
    /** Escape processing: Have seen an ESC character - proceed to {@link #doEsc(int)} */
    private static final int ESC = 1;
    /** Escape processing: Have seen ESC POUND */
    private static final int ESC_POUND = 2;
    /** Escape processing: Have seen ESC and a character-set-select ( char */
    private static final int ESC_SELECT_LEFT_PAREN = 3;
    /** Escape processing: Have seen ESC and a character-set-select ) char */
    private static final int ESC_SELECT_RIGHT_PAREN = 4;
    /** Escape processing: "ESC [" or CSI (Control Sequence Introducer). */
    private static final int ESC_CSI = 6;
    /** Escape processing: ESC [ ? */
    private static final int ESC_CSI_QUESTIONMARK = 7;
    /** Escape processing: ESC [ $ */
    private static final int ESC_CSI_DOLLAR = 8;
    /** Escape processing: ESC % */
    private static final int ESC_PERCENT = 9;
    /** Escape processing: ESC ] (AKA OSC - Operating System Controls) */
    private static final int ESC_OSC = 10;
    /** Escape processing: ESC ] (AKA OSC - Operating System Controls) ESC */
    private static final int ESC_OSC_ESC = 11;
    /** Escape processing: ESC [ > */
    private static final int ESC_CSI_BIGGERTHAN = 12;
    /** Escape procession: "ESC P" or Device Control String (DCS) */
    private static final int ESC_P = 13;
    /** Escape processing: CSI > */
    private static final int ESC_CSI_QUESTIONMARK_ARG_DOLLAR = 14;
    /** Escape processing: CSI $ARGS ' ' */
    private static final int ESC_CSI_ARGS_SPACE = 15;
    /** Escape processing: CSI $ARGS '*' */
    private static final int ESC_CSI_ARGS_ASTERIX = 16;
    /** Escape processing: CSI " */
    private static final int ESC_CSI_DOUBLE_QUOTE = 17;
    /** Escape processing: CSI ' */
    private static final int ESC_CSI_SINGLE_QUOTE = 18;
    /** Escape processing: CSI ! */
    private static final int ESC_CSI_EXCLAMATION = 19;
    /** Escape processing: "ESC _" or Application Program Command (APC). */
    private static final int ESC_APC = 20;
    /** Escape processing: "ESC _" or Application Program Command (APC), followed by Escape. */
    private static final int ESC_APC_ESCAPE = 21;
    /** Escape processing: ESC [ <parameter bytes> */
    private static final int ESC_CSI_UNSUPPORTED_PARAMETER_BYTE = 22;
    /** Escape processing: ESC [ <parameter bytes> <intermediate bytes> */
    private static final int ESC_CSI_UNSUPPORTED_INTERMEDIATE_BYTE = 23;

    /** The number of parameter arguments including colon separated sub-parameters. */
    private static final int MAX_ESCAPE_PARAMETERS = 32;

    /** Needs to be large enough to contain reasonable OSC 52 pastes. */
    private static final int MAX_OSC_STRING_LENGTH = 8192;

    /** DECSET 1 - application cursor keys. */
    private static final int DECSET_BIT_APPLICATION_CURSOR_KEYS = 1;
    private static final int DECSET_BIT_REVERSE_VIDEO = 1 << 1;
    private static final int DECSET_BIT_ORIGIN_MODE = 1 << 2;
    private static final int DECSET_BIT_AUTOWRAP = 1 << 3;
    /** DECSET 25 - if the cursor should be enabled, {@link #isCursorEnabled()}. */
    private static final int DECSET_BIT_CURSOR_ENABLED = 1 << 4;
    private static final int DECSET_BIT_APPLICATION_KEYPAD = 1 << 5;
    /** DECSET 1000 - if to report mouse press&release events. */
    private static final int DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE = 1 << 6;
    /** DECSET 1002 - like 1000, but report moving mouse while pressed. */
    private static final int DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT = 1 << 7;
    /** DECSET 1004 - send focus gain/loss. */
    private static final int DECSET_BIT_SEND_FOCUS_EVENTS = 1 << 8;
    /** DECSET 1006 - SGR-like mouse protocol (the modern sane choice). */
    private static final int DECSET_BIT_MOUSE_PROTOCOL_SGR = 1 << 9;
    /** DECSET 2004 - see {@link #paste(String)} */
    private static final int DECSET_BIT_BRACKETED_PASTE_MODE = 1 << 10;
    /** Toggled with DECLRMM - http://www.vt100.net/docs/vt510-rm/DECLRMM */
    private static final int DECSET_BIT_LEFTRIGHT_MARGIN_MODE = 1 << 11;
    /** Not really DECSET bit... - http://www.vt100.net/docs/vt510-rm/DECSACE */
    private static final int DECSET_BIT_RECTANGULAR_CHANGEATTRIBUTE = 1 << 12;


    private String mTitle;
    private final Stack<String> mTitleStack = new Stack<>();

    /** The cursor position. Between (0,0) and (mRows-1, mColumns-1). */
    private int mCursorRow, mCursorCol;

    /** The number of character rows and columns in the terminal screen. */
    public int mRows, mColumns;

    /** Size of a terminal cell in pixels. */
    private int mCellWidthPixels, mCellHeightPixels;

    /** The number of terminal transcript rows that can be scrolled back to. */
    public static final int TERMINAL_TRANSCRIPT_ROWS_MIN = 100;
    public static final int TERMINAL_TRANSCRIPT_ROWS_MAX = 50000;
    public static final int DEFAULT_TERMINAL_TRANSCRIPT_ROWS = 2000;


    /* The supported terminal cursor styles. */

    public static final int TERMINAL_CURSOR_STYLE_BLOCK = 0;
    public static final int TERMINAL_CURSOR_STYLE_UNDERLINE = 1;
    public static final int TERMINAL_CURSOR_STYLE_BAR = 2;
    public static final int DEFAULT_TERMINAL_CURSOR_STYLE = TERMINAL_CURSOR_STYLE_BLOCK;
    public static final Integer[] TERMINAL_CURSOR_STYLES_LIST = new Integer[]{TERMINAL_CURSOR_STYLE_BLOCK, TERMINAL_CURSOR_STYLE_UNDERLINE, TERMINAL_CURSOR_STYLE_BAR};

    /** The terminal cursor styles. */
    private int mCursorStyle = DEFAULT_TERMINAL_CURSOR_STYLE;


    /** The normal screen buffer. Stores the characters that appear on the screen of the emulated terminal. */
    private final TerminalBuffer mMainBuffer;
    final TerminalBuffer mAltBuffer;
    /** The current screen buffer, pointing at either {@link #mMainBuffer} or {@link #mAltBuffer}. */
    private TerminalBuffer mScreen;

    /** The terminal session this emulator is bound to. */
    private final TerminalOutput mSession;

    TerminalSessionClient mClient;

    /** Keeps track of the current argument of the current escape sequence. Ranges from 0 to MAX_ESCAPE_PARAMETERS-1. */
    private int mArgIndex;
    /** Holds the arguments of the current escape sequence. */
    private final int[] mArgs = new int[MAX_ESCAPE_PARAMETERS];
    /** Holds the bit flags which arguments are sub parameters (after a colon) - bit N is set if <code>mArgs[N]</code> is a sub parameter. */
    private int mArgsSubParamsBitSet = 0;

    /** Holds OSC and device control arguments, which can be strings. */
    private final StringBuilder mOSCOrDeviceControlArgs = new StringBuilder();

    /**
     * True if the current escape sequence should continue, false if the current escape sequence should be terminated.
     * Used when parsing a single character.
     */
    private boolean mContinueSequence;

    /** The current state of the escape sequence state machine. One of the ESC_* constants. */
    private int mEscapeState;

    /** State saved by DECSC and restored by DECRC. */
    private static final class SavedScreenState {
        int mSavedCursorRow, mSavedCursorCol;
        int mSavedEffect, mSavedForeColor, mSavedBackColor;
        int mSavedDecFlags;
        boolean mUseLineDrawingG0, mUseLineDrawingG1, mUseLineDrawingUsesG0;
    }

    private final SavedScreenState mSavedStateMain = new SavedScreenState();
    private final SavedScreenState mSavedStateAlt = new SavedScreenState();

    /** http://www.vt100.net/docs/vt102-ug/table5-15.html */
    private boolean mUseLineDrawingG0, mUseLineDrawingG1, mUseLineDrawingUsesG0 = true;

    /**
     * @see TerminalEmulator#mapDecSetBitToInternalBit(int)
     */
    private int mCurrentDecSetFlags, mSavedDecSetFlags;

    /**
     * If insert mode (as opposed to replace mode) is active. In insert mode new characters are inserted, pushing
     * existing text to the right. Characters moved past the right margin are lost.
     */
    private boolean mInsertMode;

    /** An array of tab stops. mTabStop[i] is true if there is a tab stop set for column i. */
    private boolean[] mTabStop;

    /**
     * Top margin of screen for scrolling ranges from 0 to mRows-2. Bottom margin ranges from mTopMargin + 2 to mRows
     * (Defines the first row after the scrolling region). Left/right margin in [0, mColumns].
     */
    private int mTopMargin, mBottomMargin, mLeftMargin, mRightMargin;

    /**
     * If the next character to be emitted will be automatically wrapped to the next line.
     */
    private boolean mAboutToAutoWrap;

    /**
     * If the cursor blinking is enabled.
     */
    private boolean mCursorBlinkingEnabled;

    /**
     * If currently cursor should be in a visible state or not if {@link #mCursorBlinkingEnabled}
     * is {@code true}.
     */
    private boolean mCursorBlinkState;

    /**
     * Current foreground, background and underline colors. Can either be a color index in [0,259] or a truecolor (24-bit) value.
     */
    int mForeColor, mBackColor, mUnderlineColor;

    /** Current {@link TextStyle} effect. */
    int mEffect;

    /**
     * The number of scrolled lines since last calling {@link #clearScrollCounter()}.
     */
    private int mScrollCounter = 0;

    /** If automatic scrolling of terminal is disabled */
    private boolean mAutoScrollDisabled;

    private byte mUtf8ToFollow, mUtf8Index;
    private final byte[] mUtf8InputBuffer = new byte[4];
    private int mLastEmittedCodePoint = -1;

    public final TerminalColors mColors = new TerminalColors();

    private static final String LOG_TAG = "TerminalEmulator";

    // ========================================================================
    // Rust Full Takeover 逻辑
    // ========================================================================

    /** 指向 Rust 引擎对象的原生指针 */
    private long mRustEnginePtr = 0;

    /**
     * 是否开启 Rust 全接管模式。
     * 注意：Rust 引擎目前仍在开发中，存在以下已知问题：
     * - 备用屏幕缓冲区支持不完整
     * - 某些情况下可能出现 JNI borrow checker 问题
     * 建议在生产环境中使用 false，开发测试可使用 true
     */
    public static final boolean USE_RUST_FULL_TAKEOVER = true;

    private static boolean sRustLibLoaded = false;

    static {
        try {
            System.loadLibrary("termux_rust");
            sRustLibLoaded = true;
        } catch (UnsatisfiedLinkError e) {
            // Ignore
        }
    }

    /** Rust 回调接口实现 */
    public interface RustEngineCallback {
        void onScreenUpdate();
        void reportTitleChange(String title);
        void reportColorsChanged();
        void reportCursorVisibility(boolean visible);
        void onCopyTextToClipboard(String text);
        void onPasteTextFromClipboard();
        void onWriteToSession(String data);
        void reportColorResponse(String colorSpec);
        void reportTerminalResponse(String response);
    }

    private boolean isDecsetInternalBitSet(int bit) {
        return (mCurrentDecSetFlags & bit) != 0;
    }

    private void setDecsetinternalBit(int internalBit, boolean set) {
        if (set) {
            if (internalBit == DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE) {
                setDecsetinternalBit(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT, false);
            } else if (internalBit == DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT) {
                setDecsetinternalBit(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE, false);
            }
        }
        if (set) {
            mCurrentDecSetFlags |= internalBit;
        } else {
            mCurrentDecSetFlags &= ~internalBit;
        }
    }

    static int mapDecSetBitToInternalBit(int decsetBit) {
        switch (decsetBit) {
            case 1: return DECSET_BIT_APPLICATION_CURSOR_KEYS;
            case 5: return DECSET_BIT_REVERSE_VIDEO;
            case 6: return DECSET_BIT_ORIGIN_MODE;
            case 7: return DECSET_BIT_AUTOWRAP;
            case 25: return DECSET_BIT_CURSOR_ENABLED;
            case 66: return DECSET_BIT_APPLICATION_KEYPAD;
            case 69: return DECSET_BIT_LEFTRIGHT_MARGIN_MODE;
            case 1000: return DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE;
            case 1002: return DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT;
            case 1004: return DECSET_BIT_SEND_FOCUS_EVENTS;
            case 1006: return DECSET_BIT_MOUSE_PROTOCOL_SGR;
            case 2004: return DECSET_BIT_BRACKETED_PASTE_MODE;
            default: return -1;
        }
    }

    public TerminalEmulator(TerminalOutput session, int columns, int rows, int cellWidthPixels, int cellHeightPixels, Integer transcriptRows, TerminalSessionClient client) {
        mSession = session;
        mScreen = mMainBuffer = new TerminalBuffer(columns, getTerminalTranscriptRows(transcriptRows), rows);
        mAltBuffer = new TerminalBuffer(columns, rows, rows);
        mClient = client;
        mRows = rows;
        mColumns = columns;
        mCellWidthPixels = cellWidthPixels;
        mCellHeightPixels = cellHeightPixels;

        mTabStop = new boolean[mColumns];
        reset();

        if (USE_RUST_FULL_TAKEOVER && sRustLibLoaded) {
            try {
                mRustEnginePtr = createEngineRustWithCallback(columns, rows, getTerminalTranscriptRows(transcriptRows), mCellWidthPixels, mCellHeightPixels, new RustEngineCallback() {
                    @Override
                    public void onScreenUpdate() {
                        if (mClient != null && TerminalEmulator.this.mSession instanceof TerminalSession) {
                            mClient.onTextChanged((TerminalSession) TerminalEmulator.this.mSession);
                        }
                    }

                    @Override
                    public void reportTitleChange(String title) {
                        mTitle = title;
                        if (mClient != null && TerminalEmulator.this.mSession instanceof TerminalSession) {
                            mClient.onTitleChanged((TerminalSession) TerminalEmulator.this.mSession);
                        }
                    }

                    @Override
                    public void reportColorsChanged() {
                        syncColorsFromRust();
                        if (mClient != null && TerminalEmulator.this.mSession instanceof TerminalSession) {
                            mClient.onColorsChanged((TerminalSession) TerminalEmulator.this.mSession);
                        }
                    }

                    @Override
                    public void reportCursorVisibility(boolean visible) {
                        if (mClient != null) {
                            mClient.onTerminalCursorStateChange(visible);
                        }
                    }

                    @Override
                    public void onCopyTextToClipboard(String text) {
                        if (mSession != null) {
                            mSession.onCopyTextToClipboard(text);
                        }
                    }

                    @Override
                    public void onPasteTextFromClipboard() {
                        if (mSession != null) {
                            mSession.onPasteTextFromClipboard();
                        }
                    }

                    @Override
                    public void onWriteToSession(String data) {
                        if (mSession != null) {
                            mSession.write(data);
                        }
                    }

                    @Override
                    public void reportColorResponse(String colorSpec) {
                        if (mSession != null) {
                            mSession.write("\u001b]" + colorSpec + "\u0007");
                        }
                    }

                    @Override
                    public void reportTerminalResponse(String response) {
                        if (mSession != null) {
                            mSession.write(response);
                        }
                    }
                });
            } catch (UnsatisfiedLinkError e) {
                mRustEnginePtr = 0;
            }
        }
    }

    /**
     * 销毁终端仿真器，释放底层 Rust 引擎内存。
     */
    public void destroy() {
        if (mRustEnginePtr != 0) {
            destroyEngineRust(mRustEnginePtr);
            mRustEnginePtr = 0;
        }
    }

    /**
     * 从 Rust 引擎同步颜色到 Java。
     */
    private void syncColorsFromRust() {
        if (mRustEnginePtr != 0) {
            getColorsFromRust(mRustEnginePtr, mColors.mCurrentColors);
        }
    }

    public void updateTerminalSessionClient(TerminalSessionClient client) {
        mClient = client;
        setCursorStyle();
        setCursorBlinkState(true);
    }

    public TerminalBuffer getScreen() {
        return mScreen;
    }

    public boolean isAlternateBufferActive() {
        return mScreen == mAltBuffer;
    }

    private int getTerminalTranscriptRows(Integer transcriptRows) {
        if (transcriptRows == null || transcriptRows < TERMINAL_TRANSCRIPT_ROWS_MIN || transcriptRows > TERMINAL_TRANSCRIPT_ROWS_MAX)
            return DEFAULT_TERMINAL_TRANSCRIPT_ROWS;
        else
            return transcriptRows;
    }

    public void sendMouseEvent(int mouseButton, int column, int row, boolean pressed) {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            sendMouseEventFromRust(mRustEnginePtr, mouseButton, column, row, pressed);
            return;
        }
        if (column < 1) column = 1;
        if (column > mColumns) column = mColumns;
        if (row < 1) row = 1;
        if (row > mRows) row = mRows;

        if (mouseButton == MOUSE_LEFT_BUTTON_MOVED && !isDecsetInternalBitSet(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT)) {
            // Do not send tracking.
        } else if (isDecsetInternalBitSet(DECSET_BIT_MOUSE_PROTOCOL_SGR)) {
            mSession.write(String.format("\033[<%d;%d;%d" + (pressed ? 'M' : 'm'), mouseButton, column, row));
        } else {
            mouseButton = pressed ? mouseButton : 3;
            boolean out_of_bounds = column > 255 - 32 || row > 255 - 32;
            if (!out_of_bounds) {
                byte[] data = {'\033', '[', 'M', (byte) (32 + mouseButton), (byte) (32 + column), (byte) (32 + row)};
                mSession.write(data, 0, data.length);
            }
        }
    }

    public void resize(int columns, int rows, int cellWidthPixels, int cellHeightPixels) {
        this.mCellWidthPixels = cellWidthPixels;
        this.mCellHeightPixels = cellHeightPixels;

        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            resizeEngineRustFull(mRustEnginePtr, columns, rows);
        }

        if (mRows == rows && mColumns == columns) {
            return;
        } else if (columns < 2 || rows < 2) {
            throw new IllegalArgumentException("rows=" + rows + ", columns=" + columns);
        }

        if (mRows != rows) {
            mRows = rows;
            mTopMargin = 0;
            mBottomMargin = mRows;
        }
        if (mColumns != columns) {
            int oldColumns = mColumns;
            mColumns = columns;
            boolean[] oldTabStop = mTabStop;
            mTabStop = new boolean[mColumns];
            setDefaultTabStops();
            int toTransfer = Math.min(oldColumns, columns);
            System.arraycopy(oldTabStop, 0, mTabStop, 0, toTransfer);
            mLeftMargin = 0;
            mRightMargin = mColumns;
        }

        resizeScreen();
    }

    private void resizeScreen() {
        final int[] cursor = {mCursorCol, mCursorRow};
        int newTotalRows = (mScreen == mAltBuffer) ? mRows : mMainBuffer.mTotalRows;
        mScreen.resize(mColumns, mRows, newTotalRows, cursor, getStyle(), isAlternateBufferActive());
        mCursorCol = cursor[0];
        mCursorRow = cursor[1];
    }

    public int getCursorRow() {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            return getCursorRowFromRust(mRustEnginePtr);
        }
        return mCursorRow;
    }

    public int getCursorCol() {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            return getCursorColFromRust(mRustEnginePtr);
        }
        return mCursorCol;
    }

    public int getCursorStyle() {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            return getCursorStyleFromRust(mRustEnginePtr);
        }
        return mCursorStyle;
    }

    public boolean shouldCursorBeVisible() {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            return shouldCursorBeVisibleFromRust(mRustEnginePtr);
        }
        if (!isCursorEnabled())
            return false;
        else
            return mCursorBlinkingEnabled ? mCursorBlinkState : true;
    }

    public boolean isReverseVideo() {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            return isReverseVideoFromRust(mRustEnginePtr);
        }
        return isDecsetInternalBitSet(DECSET_BIT_REVERSE_VIDEO);
    }

    public String getTitle() {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            return getTitleFromRust(mRustEnginePtr);
        }
        return mTitle;
    }

    public void syncRowFromRust(int row) {
        if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
            TerminalRow lineObject = mScreen.allocateFullLineIfNecessary(mScreen.externalToInternalRow(row));
            readRowFromRust(mRustEnginePtr, row, lineObject.mText, lineObject.mStyle);
        }
    }

    public void setCursorStyle() {
        Integer cursorStyle = null;
        if (mClient != null)
            cursorStyle = mClient.getTerminalCursorStyle();

        if (cursorStyle == null || !Arrays.asList(TERMINAL_CURSOR_STYLES_LIST).contains(cursorStyle))
            mCursorStyle = DEFAULT_TERMINAL_CURSOR_STYLE;
        else
            mCursorStyle = cursorStyle;
    }

    public boolean isCursorEnabled() {
        return isDecsetInternalBitSet(DECSET_BIT_CURSOR_ENABLED);
    }

    public void setCursorBlinkingEnabled(boolean cursorBlinkingEnabled) {
        this.mCursorBlinkingEnabled = cursorBlinkingEnabled;
    }

    public void setCursorBlinkState(boolean cursorBlinkState) {
        this.mCursorBlinkState = cursorBlinkState;
    }

    public boolean isKeypadApplicationMode() {
        return isDecsetInternalBitSet(DECSET_BIT_APPLICATION_KEYPAD);
    }

    public boolean isCursorKeysApplicationMode() {
        return isDecsetInternalBitSet(DECSET_BIT_APPLICATION_CURSOR_KEYS);
    }

    public boolean isMouseTrackingActive() {
        return isDecsetInternalBitSet(DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE) || isDecsetInternalBitSet(DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT);
    }

    private void setDefaultTabStops() {
        for (int i = 0; i < mColumns; i++)
            mTabStop[i] = (i & 7) == 0 && i != 0;
    }

    public final Object mDataLock = new Object();

    public void append(byte[] buffer, int length) {
        synchronized (mDataLock) {
            if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
                try {
                    processEngineRust(mRustEnginePtr, buffer, 0, length);
                    return;
                } catch (Exception e) {
                    android.util.Log.e("Termux", "Rust engine process error", e);
                }
            }

            int i = 0;
            while (i < length) {
                if (sRustLibLoaded && mUtf8ToFollow == 0 && mEscapeState == ESC_NONE) {
                    try {
                        boolean useLineDrawing = mUseLineDrawingUsesG0 ? mUseLineDrawingG0 : mUseLineDrawingG1;
                        int rustProcessed = processBatchRust(buffer, i, length - i, useLineDrawing);
                        if (rustProcessed > 0) {
                            writeBatch(buffer, i, rustProcessed);
                            i += rustProcessed;
                            continue;
                        }
                    } catch (UnsatisfiedLinkError e) {
                        // Fallback
                    }
                }
                processByte(buffer[i++]);
            }
        }
    }

    public void paste(String text) {
        synchronized (mDataLock) {
            if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
                pasteTextFromRust(mRustEnginePtr, text);
                return;
            }
            text = text.replaceAll("(\u001B|[\u0080-\u009F])", "");
            text = text.replaceAll("\r?\n", "\r");
            boolean bracketed = isDecsetInternalBitSet(DECSET_BIT_BRACKETED_PASTE_MODE);
            if (bracketed) mSession.write("\033[200~");
            mSession.write(text);
            if (bracketed) mSession.write("\033[201~");
        }
    }

    public String getSelectedText(int x1, int y1, int x2, int y2) {
        synchronized (mDataLock) {
            if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
                for (int y = y1; y <= y2; y++) {
                    syncRowFromRust(y);
                }
            }
            return mScreen.getSelectedText(x1, y1, x2, y2);
        }
    }

    /** 恢复 TerminalView 需要的方法 */
    public int getScrollCounter() {
        synchronized (mDataLock) {
            if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
                return getScrollCounterFromRust(mRustEnginePtr);
            }
            return mScrollCounter;
        }
    }

    public void clearScrollCounter() {
        synchronized (mDataLock) {
            if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
                clearScrollCounterFromRust(mRustEnginePtr);
            }
            mScrollCounter = 0;
        }
    }

    public boolean isAutoScrollDisabled() {
        synchronized (mDataLock) {
            if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
                return isAutoScrollDisabledFromRust(mRustEnginePtr);
            }
            return mAutoScrollDisabled;
        }
    }

    public void toggleAutoScrollDisabled() {
        synchronized (mDataLock) {
            if (USE_RUST_FULL_TAKEOVER && mRustEnginePtr != 0) {
                toggleAutoScrollDisabledFromRust(mRustEnginePtr);
            } else {
                mAutoScrollDisabled = !mAutoScrollDisabled;
            }
        }
    }

    @Override
    protected void finalize() throws Throwable {
        try {
            if (mRustEnginePtr != 0) {
                destroyEngineRust(mRustEnginePtr);
                mRustEnginePtr = 0;
            }
        } finally {
            super.finalize();
        }
    }

    private void emitCodePoint(int codePoint) {
        mScreen.setChar(mCursorCol, mCursorRow, codePoint, getStyle());
        mLastEmittedCodePoint = codePoint;
        int advance = WcWidth.width(codePoint);
        if (mCursorCol + advance < mColumns) {
            mCursorCol += advance;
        } else {
            mAboutToAutoWrap = true;
        }
    }

    private long getStyle() {
        return TextStyle.encode(mForeColor, mBackColor, mEffect);
    }

    private void blockClear(int x, int y, int width) {
        blockClear(x, y, width, 1);
    }

    private void blockClear(int x, int y, int width, int height) {
        mScreen.blockSet(x, y, width, height, ' ', getStyle());
    }

    private void setCursorRow(int row) {
        mCursorRow = row;
        mAboutToAutoWrap = false;
    }

    private void setCursorCol(int col) {
        mCursorCol = col;
        mAboutToAutoWrap = false;
    }

    private void setCursorRowCol(int row, int col) {
        mCursorRow = row;
        mCursorCol = col;
        mAboutToAutoWrap = false;
    }

    private void setCursorPosition(int x, int y) {
        int effectiveTopMargin = 0;
        int effectiveBottomMargin = mRows;
        if (isDecsetInternalBitSet(DECSET_BIT_ORIGIN_MODE)) {
            effectiveTopMargin = mTopMargin;
            effectiveBottomMargin = mBottomMargin;
        }
        mCursorRow = Math.max(effectiveTopMargin, Math.min(effectiveTopMargin + y, effectiveBottomMargin - 1));
        mCursorCol = Math.max(0, Math.min(x, mColumns - 1));
        mAboutToAutoWrap = false;
    }

    private void setCursorColRespectingOriginMode(int col) {
        setCursorPosition(col, isDecsetInternalBitSet(DECSET_BIT_ORIGIN_MODE) ? mCursorRow - mTopMargin : mCursorRow);
    }

    private void scrollDownOneLine() {
        mScrollCounter++;
        mScreen.scrollDownOneLine(mTopMargin, mBottomMargin, getStyle());
    }

    public void reset() {
        mCursorRow = mCursorCol = 0;
        mArgIndex = 0;
        mContinueSequence = false;
        mEscapeState = ESC_NONE;
        mCurrentDecSetFlags = 0;
        setDecsetinternalBit(DECSET_BIT_AUTOWRAP, true);
        setDecsetinternalBit(DECSET_BIT_CURSOR_ENABLED, true);
        mTopMargin = 0;
        mBottomMargin = mRows;
        mLeftMargin = 0;
        mRightMargin = mColumns;
        mAboutToAutoWrap = false;
        mForeColor = TextStyle.COLOR_INDEX_FOREGROUND;
        mBackColor = TextStyle.COLOR_INDEX_BACKGROUND;
        mEffect = 0;
        mUtf8ToFollow = mUtf8Index = 0;
        Arrays.fill(mTabStop, false);
        setDefaultTabStops();
    }

    // Native 方法定义
    private static native long createEngineRustWithCallback(int cols, int rows, int totalRows, int cellWidth, int cellHeight, Object callbackObj);
    private static native void processEngineRust(long enginePtr, byte[] input, int offset, int length);
    private static native void readRowFromRust(long enginePtr, int row, char[] destText, long[] destStyle);
    private static native void resizeEngineRustFull(long enginePtr, int newCols, int newRows);
    private static native int getCursorColFromRust(long enginePtr);
    private static native int getCursorRowFromRust(long enginePtr);
    private static native int getCursorStyleFromRust(long enginePtr);
    private static native boolean shouldCursorBeVisibleFromRust(long enginePtr);
    private static native boolean isReverseVideoFromRust(long enginePtr);
    private static native String getTitleFromRust(long enginePtr);
    private static native void sendMouseEventFromRust(long enginePtr, int mouseButton, int column, int row, boolean pressed);
    private static native void sendKeyCodeFromRust(long enginePtr, int keyCode, String keyChar, int keyMod);
    private static native void reportFocusGainFromRust(long enginePtr);
    private static native void reportFocusLossFromRust(long enginePtr);
    private static native void pasteTextFromRust(long enginePtr, String text);
    private static native void destroyEngineRust(long enginePtr);
    private static native void getColorsFromRust(long enginePtr, int[] colors);
    private static native int processBatchRust(byte[] buffer, int offset, int length, boolean useLineDrawing);
    private static native void writeASCIIBatchNative(byte[] src, int srcOffset, char[] destText, long[] destStyle, int destOffset, int length, long style, boolean useLineDrawing);

    /** 恢复滚动相关的 Native 方法 */
    private static native int getScrollCounterFromRust(long enginePtr);
    private static native void clearScrollCounterFromRust(long enginePtr);
    private static native boolean isAutoScrollDisabledFromRust(long enginePtr);
    private static native void toggleAutoScrollDisabledFromRust(long enginePtr);

    // ========================================================================
    // 旧的或兼容性方法 (解析逻辑略)
    // ========================================================================
    private void processByte(byte b) { }
    private void processCodePoint(int b) { }
    private void writeBatch(byte[] data, int offset, int length) {
        for (int i = 0; i < length; i++) {
            emitCodePoint(data[offset + i]);
        }
    }
}
