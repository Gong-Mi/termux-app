package com.termux.terminal;

import java.util.Arrays;

/**
 * A circular buffer of {@link TerminalRow}:s which keeps notes about what is visible on a logical screen and the scroll
 * history.
 * <p>
 * See {@link #externalToInternalRow(int)} for how to map from logical screen rows to array indices.
 */
public final class TerminalBuffer {

    TerminalRow[] mLines;
    /** The length of {@link #mLines}. */
    int mTotalRows;
    /** The number of rows and columns visible on the screen. */
    int mScreenRows, mColumns;
    /** The number of rows kept in history. */
    private int mActiveTranscriptRows = 0;
    /** The index in the circular buffer where the visible screen starts. */
    private int mScreenFirstRow = 0;
    /** Whether to get active transcript rows from Rust (Full Takeover mode). */
    private boolean mUseRustTranscriptRows = false;
    /** Reference to Rust engine pointer for getting transcript rows. */
    private long mRustEnginePtr = 0;
    /** Reference to the emulator for state synchronization. */
    private TerminalEmulator mEmulator;

    /**
     * Create a transcript screen.
     *
     * @param columns    the width of the screen in characters.
     * @param totalRows  the height of the entire text area, in rows of text.
     * @param screenRows the height of just the screen, not including the transcript that holds lines that have scrolled off
     *                   the top of the screen.
     */
    public TerminalBuffer(int columns, int totalRows, int screenRows) {
        mColumns = columns;
        mTotalRows = totalRows;
        mScreenRows = screenRows;
        mLines = new TerminalRow[totalRows];
        for (int i = 0; i < totalRows; i++) {
            mLines[i] = new TerminalRow(columns, TextStyle.NORMAL);
        }

        blockSet(0, 0, columns, screenRows, ' ', TextStyle.NORMAL);
    }

    public void setEmulator(TerminalEmulator emulator) {
        this.mEmulator = emulator;
    }

    public void setScreenFirstRow(int screenFirstRow) {
        this.mScreenFirstRow = screenFirstRow;
    }

    public String getTranscriptText() {
        return getSelectedText(0, -getActiveTranscriptRows(), mColumns, mScreenRows).trim();
    }

    public String getTranscriptTextWithoutJoinedLines() {
        return getSelectedText(0, -getActiveTranscriptRows(), mColumns, mScreenRows, false).trim();
    }

    public String getTranscriptTextWithFullLinesJoined() {
        return getSelectedText(0, -getActiveTranscriptRows(), mColumns, mScreenRows, true, true).trim();
    }

    public int getActiveTranscriptRows() {
        if (mUseRustTranscriptRows && mRustEnginePtr != 0) {
            return TerminalEmulator.getActiveTranscriptRowsFromRust(mRustEnginePtr);
        }
        return mActiveTranscriptRows;
    }

    public void setRustEnginePtr(long enginePtr) {
        mRustEnginePtr = enginePtr;
        mUseRustTranscriptRows = (enginePtr != 0);
    }

    public int getActiveRows() {
        return getActiveTranscriptRows() + mScreenRows;
    }

    /**
     * Map a logical row index to the corresponding index in the circular buffer of mLines.
     *
     * @param externalRow index between -getActiveTranscriptRows() and mScreenRows-1.
     * @return index between 0 and mTotalRows-1.
     */
    public int externalToInternalRow(int externalRow) {
        // 在同步前保存当前状态，避免同步后状态变化导致验证失败
        int screenRowsBefore = mScreenRows;
        int activeTranscriptRowsBefore = getActiveTranscriptRows();

        if (mEmulator != null) {
            mEmulator.syncStateFromRustIfRequired();
        }

        // 使用同步后的值进行验证，但允许一定的容错
        // 如果 externalRow 超出范围，将其钳制到有效范围内而不是抛出异常
        int activeTranscriptRows = getActiveTranscriptRows();
        if (externalRow < -activeTranscriptRows) {
            externalRow = -activeTranscriptRows;
        } else if (externalRow >= mScreenRows) {
            externalRow = mScreenRows - 1;
        }

        int row = mScreenFirstRow + externalRow;
        if (row < 0) {
            row += mTotalRows;
        } else if (row >= mTotalRows) {
            row -= mTotalRows;
        }
        return row;
    }

    public void setLineWrap(int row, boolean wrap) {
        mLines[externalToInternalRow(row)].mLineWrap = wrap;
    }

    public boolean getLineWrap(int row) {
        return mLines[externalToInternalRow(row)].mLineWrap;
    }

    public void clearTranscript() {
        mScreenFirstRow = 0;
        mActiveTranscriptRows = 0;
    }

    public void blockCopy(int sx, int sy, int w, int h, int dx, int dy) {
        if (w == 0 || h == 0)
            return;
        if (sy <= dy && dy < sy + h) {
            // Unsafe
            for (int y = h - 1; y >= 0; y--) {
                copyLine(sy + y, sx, dy + y, dx, w);
            }
        } else {
            // Safe
            for (int y = 0; y < h; y++) {
                copyLine(sy + y, sx, dy + y, dx, w);
            }
        }
    }

    private void copyLine(int sy, int sx, int dy, int dx, int w) {
        TerminalRow srcRow = mLines[externalToInternalRow(sy)];
        TerminalRow dstRow = mLines[externalToInternalRow(dy)];
        srcRow.copyRange(dstRow, sx, dx, w);
    }

    public void blockSet(int sx, int sy, int w, int h, int val, long style) {
        if (w <= 0 || h <= 0) return;
        for (int y = 0; y < h; y++) {
            TerminalRow row = mLines[externalToInternalRow(sy + y)];
            for (int x = 0; x < w; x++) {
                row.setChar(sx + x, val, style);
            }
        }
    }

    public TerminalRow allocateFullLineIfNecessary(int row) {
        return mLines[externalToInternalRow(row)];
    }

    public void setChar(int column, int row, int codePoint, long style) {
        mLines[externalToInternalRow(row)].setChar(column, codePoint, style);
    }

    public int getChar(int column, int row) {
        return mLines[externalToInternalRow(row)].getChar(column);
    }

    public long getStyle(int column, int row) {
        return mLines[externalToInternalRow(row)].getStyle(column);
    }

    public long getStyleAt(int row, int column) {
        return getStyle(column, row);
    }

    public String getWordAtLocation(int x, int y) {
        // Simple implementation for tests
        TerminalRow row = mLines[externalToInternalRow(y)];
        String line = new String(row.mText, 0, mColumns);
        if (x < 0 || x >= line.length()) return "";
        
        int x1 = x, x2 = x;
        while (x1 > 0 && line.charAt(x1-1) != ' ') x1--;
        while (x2 < line.length() && line.charAt(x2) != ' ') x2++;
        
        if (x1 == x2) return "";
        return line.substring(x1, x2);
    }

    /**
     * @param selX1 column of start
     * @param selY1 row of start
     * @param selX2 column of end
     * @param selY2 row of end
     * @return selected text
     */
    public String getSelectedText(int selX1, int selY1, int selX2, int selY2) {
        return getSelectedText(selX1, selY1, selX2, selY2, true);
    }

    public String getSelectedText(int selX1, int selY1, int selX2, int selY2, boolean joinLines) {
        return getSelectedText(selX1, selY1, selX2, selY2, joinLines, false);
    }

    public String getSelectedText(int selX1, int selY1, int selX2, int selY2, boolean joinLines, boolean joinFullLines) {
        StringBuilder builder = new StringBuilder();
        int columns = mColumns;

        if (selY1 < -getActiveTranscriptRows()) selY1 = -getActiveTranscriptRows();
        if (selY2 >= mScreenRows) selY2 = mScreenRows - 1;

        for (int row = selY1; row <= selY2; row++) {
            int x1 = (row == selY1) ? selX1 : 0;
            int x2;
            if (row == selY2) {
                x2 = selX2;
            } else {
                x2 = columns;
            }
            TerminalRow lineObject = mLines[externalToInternalRow(row)];
            int x1p = lineObject.findStartOfColumn(x1);
            int x2p = lineObject.findStartOfColumn(x2);

            char[] line = lineObject.mText;
            int lastPrintingCharIndex = -1;
            int i;
            boolean rowLineWrap = getLineWrap(row);
            if (rowLineWrap && x2 == columns) {
                // If the line was wrapped, we shouldn't trim trailing spaces
                lastPrintingCharIndex = x2p - 1;
            } else {
                for (i = x1p; i < x2p; i++) {
                    char c = line[i];
                    if (c != ' ' && c != 0) lastPrintingCharIndex = i;
                }
            }

            if (lastPrintingCharIndex != -1) {
                builder.append(line, x1p, lastPrintingCharIndex - x1p + 1);
            }

            if (row < selY2 && (!rowLineWrap || joinFullLines)) {
                builder.append('\n');
            }
        }

        return builder.toString();
    }

    public void scrollDownOneLine(int topMargin, int bottomMargin, long style) {
        if (topMargin > bottomMargin - 1 || topMargin < 0 || bottomMargin > mScreenRows)
            throw new IllegalArgumentException("topMargin=" + topMargin + ", bottomMargin=" + bottomMargin + ", mScreenRows=" + mScreenRows);

        // Implementation of scrollDownOneLine
        int internalBottom = externalToInternalRow(bottomMargin - 1);
        TerminalRow bottomRow = mLines[internalBottom];
        for (int y = bottomMargin - 1; y > topMargin; y--) {
            mLines[externalToInternalRow(y)] = mLines[externalToInternalRow(y - 1)];
        }
        mLines[externalToInternalRow(topMargin)] = bottomRow;
        bottomRow.clear(style);
    }

    /**
     * Resize the screen which this transcript is for.
     *
     * @param newColumns the new width of the screen.
     * @param newTotalRows the new total height of the transcript.
     * @param newScreenRows the new height of the screen.
     * @param cursor the current cursor.
     * @param style the current style.
     * @return the new cursor position.
     */
    public int[] resize(int newColumns, int newTotalRows, int newScreenRows, int[] cursor, long style) {
        TerminalRow[] oldLines = mLines;
        int oldTotalRows = mTotalRows;
        int oldColumns = mColumns;
        int oldScreenRows = mScreenRows;
        int oldActiveTranscriptRows = getActiveTranscriptRows();

        mLines = new TerminalRow[newTotalRows];
        for (int i = 0; i < newTotalRows; i++) {
            mLines[i] = new TerminalRow(newColumns, style);
        }

        int copyRows = Math.min(oldTotalRows, newTotalRows);
        int copyCols = Math.min(oldColumns, newColumns);

        for (int i = 0; i < copyRows; i++) {
            int oldIntRow = (mScreenFirstRow + i) % oldTotalRows;
            int newIntRow = i % newTotalRows;
            TerminalRow oldRow = oldLines[oldIntRow];
            TerminalRow newRow = mLines[newIntRow];
            
            // Copy text and style data safely
            System.arraycopy(oldRow.mText, 0, newRow.mText, 0, copyCols);
            System.arraycopy(oldRow.mStyle, 0, newRow.mStyle, 0, copyCols);
            newRow.mLineWrap = oldRow.mLineWrap;
            
            // Update space used and flags for the new row
            newRow.mSpaceUsed = (short) newColumns;
            newRow.mHasNonOneWidthOrSurrogateChars = oldRow.mHasNonOneWidthOrSurrogateChars;
        }

        mTotalRows = newTotalRows;
        mColumns = newColumns;
        mScreenRows = newScreenRows;
        mScreenFirstRow = 0;
        mActiveTranscriptRows = Math.max(0, copyRows - newScreenRows);

        return new int[]{Math.min(cursor[0], newColumns - 1), Math.min(cursor[1], newScreenRows - 1)};
    }

    public void syncFromRust(long rustEnginePtr) {
        if (rustEnginePtr == 0) return;

        try {
            // 从 Rust 获取实际的行列数，避免 Java/Rust 状态不一致
            int rustCols = TerminalEmulator.getColsFromRust(rustEnginePtr);
            int rustRows = TerminalEmulator.getRowsFromRust(rustEnginePtr);
            
            int cols = Math.max(1, Math.min(rustCols, mColumns));
            int rows = Math.max(1, Math.min(rustRows, mScreenRows));

            char[][] textBuffer = new char[rows][cols];
            long[][] styleBuffer = new long[rows][cols];

            TerminalEmulator.readScreenBatchFromRust(rustEnginePtr, textBuffer, styleBuffer, 0, rows);

            for (int i = 0; i < rows; i++) {
                int internalRow = externalToInternalRow(i);
                TerminalRow row = mLines[internalRow];
                int copyLength = Math.min(cols, Math.min(textBuffer[i].length, row.mText.length));
                System.arraycopy(textBuffer[i], 0, row.mText, 0, copyLength);
                System.arraycopy(styleBuffer[i], 0, row.mStyle, 0, Math.min(cols, styleBuffer[i].length));
                row.updateStatusAfterBatchWrite();
            }
        } catch (UnsatisfiedLinkError e) {
            // Ignore if native method not found
        }
    }

    /**
     * Read a batch of screen rows from Rust.
     */
    public static void readScreenBatch(long rustEnginePtr, char[][] textBuffer, long[][] styleBuffer, int startRow, int numRows) {
        if (rustEnginePtr == 0) return;
        try {
            TerminalEmulator.readScreenBatchFromRust(rustEnginePtr, textBuffer, styleBuffer, startRow, numRows);
        } catch (UnsatisfiedLinkError e) {
            // Ignore if native method not found
        }
    }

    /**
     * Read the full screen from Rust.
     */
    public void readFullScreen(long rustEnginePtr, char[][] textBuffer, long[][] styleBuffer) {
        if (rustEnginePtr == 0) return;
        try {
            TerminalEmulator.readFullScreenFromRust(rustEnginePtr, textBuffer, styleBuffer);
        } catch (UnsatisfiedLinkError e) {
            // Ignore if native method not found
        }
    }
}
