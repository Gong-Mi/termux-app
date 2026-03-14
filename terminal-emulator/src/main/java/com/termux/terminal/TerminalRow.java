package com.termux.terminal;

import java.nio.ByteBuffer;
import java.util.Arrays;

/**
 * A row in a terminal, composed of a fixed number of cells.
 *
 * Rust 化版本：支持直接从 DirectByteBuffer 读取数据（零拷贝）
 * 当 mSharedBuffer 不为 null 时，数据从共享内存读取（只读模式）；否则使用本地数组。
 */
public final class TerminalRow {

    private static final float SPARE_CAPACITY_FACTOR = 1.5f;
    private static final int MAX_COMBINING_CHARACTERS_PER_COLUMN = 15;

    private final int mColumns;

    // 本地数据（非 Rust 化模式）
    public char[] mText;
    public final long[] mStyle;

    // 共享内存引用（Rust 化模式 - 只读）
    private ByteBuffer mSharedBuffer;
    private int mRowOffset;

    // 缓存（用于 Rust 化模式下需要数组访问的场景）
    private char[] mTextCache;
    private long[] mStyleCache;
    private boolean mCacheValid;

    short mSpaceUsed;
    boolean mLineWrap;
    public boolean mHasNonOneWidthOrSurrogateChars;

    /**
     * 构造使用本地数组的 TerminalRow（传统模式）
     */
    public TerminalRow(int columns, long style) {
        mColumns = columns;
        mText = new char[(int) (SPARE_CAPACITY_FACTOR * columns)];
        mStyle = new long[columns];
        mSharedBuffer = null;
        mTextCache = null;
        mStyleCache = null;
        mCacheValid = false;
        clear(style);
    }

    /**
     * 构造使用共享内存的 TerminalRow（Rust 化模式 - 只读）
     * 注意：mTextCache 需要 SPARE_CAPACITY_FACTOR 额外空间，防止渲染宽字符时越界
     */
    public TerminalRow(ByteBuffer sharedBuffer, int rowOffset, int columns) {
        mColumns = columns;
        mSharedBuffer = sharedBuffer;
        mText = null;
        mStyle = null;
        // 使用 SPARE_CAPACITY_FACTOR 确保有足够空间处理宽字符和组合字符
        mTextCache = new char[(int) (SPARE_CAPACITY_FACTOR * columns)];
        mStyleCache = new long[columns];
        mCacheValid = false;
        mSpaceUsed = (short) columns;
        mLineWrap = false;
        mHasNonOneWidthOrSurrogateChars = false;
        
        // 验证 rowOffset 的有效性
        if (sharedBuffer != null && columns > 0) {
            int maxValidRowOffset = sharedBuffer.capacity() / columns - 1;
            if (rowOffset < 0 || rowOffset > maxValidRowOffset) {
                // 无效的 rowOffset，设置为安全值并初始化缓存
                mRowOffset = 0;
                Arrays.fill(mTextCache, ' ');
                Arrays.fill(mStyleCache, TextStyle.NORMAL);
                mCacheValid = true;
            } else {
                mRowOffset = rowOffset;
            }
        } else {
            mRowOffset = 0;
        }
    }

    public boolean isRustBacked() {
        return mSharedBuffer != null;
    }

    private void refreshCache() {
        if (mSharedBuffer != null && !mCacheValid) {
            // 验证 mRowOffset 的有效性，防止脏数据导致崩溃
            int maxValidRowOffset = mSharedBuffer.capacity() / mColumns - 1;
            if (mRowOffset < 0 || mRowOffset > maxValidRowOffset) {
                // mRowOffset 无效，填充默认值并标记缓存有效
                Arrays.fill(mTextCache, ' ');
                Arrays.fill(mStyleCache, TextStyle.NORMAL);
                mCacheValid = true;
                return;
            }
            
            for (int col = 0; col < mColumns; col++) {
                mTextCache[col] = getCharUnsafe(col);
                mStyleCache[col] = getStyleUnsafe(col);
            }
            mCacheValid = true;
        }
    }

    private char getCharUnsafe(int column) {
        int cellIndex = mRowOffset + column;
        // FlatScreenBuffer 布局：[header][text_data: u16 数组][style_data: u64 数组]
        // header: version(1) + padding(3) + cols(4) + rows(4) = 12 bytes
        int textByteOffset = 12 + cellIndex * 2;  // u16 = 2 bytes
        
        // 边界检查，防止 IndexOutOfBoundsException
        if (textByteOffset < 0 || textByteOffset + 1 >= mSharedBuffer.capacity()) {
            return ' ';
        }
        
        int low = mSharedBuffer.get(textByteOffset) & 0xFF;
        int high = mSharedBuffer.get(textByteOffset + 1) & 0xFF;
        return (char) (low | (high << 8));
    }

    private long getStyleUnsafe(int column) {
        int cellIndex = mRowOffset + column;
        // style_data 在 text_data 之后
        // 先计算 text_data 的总大小
        int cols = mColumns;
        int rows = mSharedBuffer.getInt(8);  // rows 在 offset 8
        int textDataSize = cols * rows * 2;  // u16 per cell
        int styleByteOffset = 12 + textDataSize + cellIndex * 8;  // u64 = 8 bytes

        // 边界检查，防止 IndexOutOfBoundsException
        if (styleByteOffset < 0 || styleByteOffset + 8 > mSharedBuffer.capacity()) {
            return TextStyle.NORMAL;
        }

        long result = 0;
        for (int i = 0; i < 8; i++) {
            result |= (mSharedBuffer.get(styleByteOffset + i) & 0xFFL) << (i * 8);
        }
        return result;
    }

    public char[] getTextArray() {
        if (mSharedBuffer != null) {
            refreshCache();
            return mTextCache;
        }
        return mText;
    }

    public long[] getStyleArray() {
        if (mSharedBuffer != null) {
            refreshCache();
            return mStyleCache;
        }
        return mStyle;
    }

    public int getSpaceUsed() {
        return mSpaceUsed;
    }

    public void updateSharedBuffer(ByteBuffer sharedBuffer, int rowOffset) {
        mSharedBuffer = sharedBuffer;
        // 验证 rowOffset 的有效性
        if (sharedBuffer != null && mColumns > 0) {
            int maxValidRowOffset = sharedBuffer.capacity() / mColumns - 1;
            if (rowOffset < 0 || rowOffset > maxValidRowOffset) {
                // 无效的 rowOffset，设置为安全值
                mRowOffset = 0;
                // 使用 SPARE_CAPACITY_FACTOR 确保有足够空间
                mTextCache = new char[(int) (SPARE_CAPACITY_FACTOR * mColumns)];
                mStyleCache = new long[mColumns];
                Arrays.fill(mTextCache, ' ');
                Arrays.fill(mStyleCache, TextStyle.NORMAL);
                mCacheValid = true;
                return;
            }
        }
        mRowOffset = rowOffset;
        mCacheValid = false;
    }

    public void copyInterval(TerminalRow line, int sourceX1, int sourceX2, int destinationX) {
        if (isRustBacked() || line.isRustBacked()) return;

        mHasNonOneWidthOrSurrogateChars |= line.mHasNonOneWidthOrSurrogateChars;
        final int x1 = line.findStartOfColumn(sourceX1);
        final int x2 = line.findStartOfColumn(sourceX2);
        boolean startingFromSecondHalfOfWideChar = (sourceX1 > 0 && line.wideDisplayCharacterStartingAt(sourceX1 - 1));
        final char[] sourceChars = (this == line) ? Arrays.copyOf(line.mText, line.mText.length) : line.mText;
        int latestNonCombiningWidth = 0;
        for (int i = x1; i < x2; i++) {
            char sourceChar = sourceChars[i];
            int codePoint = Character.isHighSurrogate(sourceChar) ? Character.toCodePoint(sourceChar, sourceChars[++i]) : sourceChar;
            if (startingFromSecondHalfOfWideChar) {
                codePoint = ' ';
                startingFromSecondHalfOfWideChar = false;
            }
            int w = WcWidth.width(codePoint);
            if (w > 0) {
                destinationX += latestNonCombiningWidth;
                sourceX1 += latestNonCombiningWidth;
                latestNonCombiningWidth = w;
            }
            setChar(destinationX, codePoint, line.getStyle(sourceX1));
        }
    }

    public void copyRange(TerminalRow dstRow, int sx, int dx, int w) {
        if (isRustBacked() || dstRow.isRustBacked()) return;
        if (w <= 0) return;
        for (int i = 0; i < w; i++) {
            int codePoint = getChar(sx + i);
            long style = getStyle(sx + i);
            dstRow.setChar(dx + i, codePoint, style);
        }
    }

    public final int findStartOfColumn(int column) {
        if (column == mColumns) return mSpaceUsed;
        char[] text = isRustBacked() ? getTextArray() : mText;
        if (text == null) return 0;

        int currentColumn = 0;
        int currentCharIndex = 0;
        while (currentCharIndex < mSpaceUsed) {
            int newCharIndex = currentCharIndex;
            char c = text[newCharIndex++];
            int codePoint = Character.isHighSurrogate(c) ? Character.toCodePoint(c, text[newCharIndex++]) : c;
            int wcwidth = WcWidth.width(codePoint);
            if (wcwidth > 0) {
                currentColumn += wcwidth;
                if (currentColumn == column) {
                    while (newCharIndex < mSpaceUsed) {
                        char nc = text[newCharIndex];
                        int ncp = Character.isHighSurrogate(nc) ? Character.toCodePoint(nc, text[newCharIndex+1]) : nc;
                        if (WcWidth.width(ncp) <= 0) newCharIndex += (ncp > 65535 ? 2 : 1);
                        else break;
                    }
                    return newCharIndex;
                } else if (currentColumn > column) return currentCharIndex;
            }
            currentCharIndex = newCharIndex;
        }
        return currentCharIndex;
    }

    public final boolean wideDisplayCharacterStartingAt(int column) {
        char[] text = isRustBacked() ? getTextArray() : mText;
        if (text == null) return false;

        for (int currentCharIndex = 0, currentColumn = 0; currentCharIndex < mSpaceUsed; ) {
            char c = text[currentCharIndex++];
            int codePoint = Character.isHighSurrogate(c) ? Character.toCodePoint(c, text[currentCharIndex++]) : c;
            int wcwidth = WcWidth.width(codePoint);
            if (wcwidth > 0) {
                if (currentColumn == column && wcwidth == 2) return true;
                currentColumn += wcwidth;
                if (currentColumn > column) return false;
            }
        }
        return false;
    }

    public void clear(long style) {
        if (isRustBacked()) return;
        Arrays.fill(mText, ' ');
        Arrays.fill(mStyle, style);
        mSpaceUsed = (short) mColumns;
        mHasNonOneWidthOrSurrogateChars = false;
    }

    public final void setChar(int columnToSet, final int codePoint, final long style) {
        if (isRustBacked()) return;
        if (columnToSet < 0 || columnToSet >= mColumns) return;

        final int newWidth = WcWidth.width(codePoint);
        mStyle[columnToSet] = style;
        if (newWidth == 2 && columnToSet + 1 < mColumns) {
            mStyle[columnToSet + 1] = style;
        }

        if (!mHasNonOneWidthOrSurrogateChars) {
            if (newWidth == 1 && codePoint < 65536) {
                mText[columnToSet] = (char) codePoint;
                return;
            }
            mHasNonOneWidthOrSurrogateChars = true;
        }
        setCharInternal(columnToSet, codePoint, style, newWidth);
    }

    public int getChar(int column) {
        if (column < 0 || column >= mColumns) return ' ';
        char[] text = isRustBacked() ? getTextArray() : mText;
        if (text == null) return ' ';
        if (!mHasNonOneWidthOrSurrogateChars) return text[column];

        int idx = findStartOfColumn(column);
        if (idx >= mSpaceUsed) return ' ';
        char c = text[idx];
        if (Character.isHighSurrogate(c) && idx + 1 < mSpaceUsed) {
            return Character.toCodePoint(c, text[idx+1]);
        }
        return c;
    }

    public final long getStyle(int column) {
        if (column < 0 || column >= mColumns) return TextStyle.NORMAL;
        long[] style = isRustBacked() ? getStyleArray() : mStyle;
        if (style == null) return TextStyle.NORMAL;
        return style[column];
    }

    public final void setChars(int columnStart, byte[] buffer, int offset, int length, long style) {
        if (columnStart < 0 || columnStart + length > mColumns) return;
        Arrays.fill(mStyle, columnStart, columnStart + length, style);
        if (!mHasNonOneWidthOrSurrogateChars) {
            for (int i = 0; i < length; i++) {
                mText[columnStart + i] = (char) (buffer[offset + i] & 0xFF);
            }
            return;
        }
        for (int i = 0; i < length; i++) {
            setChar(columnStart + i, buffer[offset + i] & 0xFF, style);
        }
    }

    private void setCharInternal(int columnToSet, int codePoint, long style, int newWidth) {
        final boolean newIsCombining = newWidth <= 0;
        boolean wasWide = (columnToSet > 0) && wideDisplayCharacterStartingAt(columnToSet - 1);
        if (newIsCombining) {
            if (wasWide) columnToSet--;
        } else {
            if (wasWide) setChar(columnToSet - 1, ' ', style);
            if (newWidth == 2 && wideDisplayCharacterStartingAt(columnToSet + 1)) setChar(columnToSet + 1, ' ', style);
        }
        final int oldStart = findStartOfColumn(columnToSet);
        final int oldWidth = WcWidth.width(mText, oldStart);
        int oldUsed = (columnToSet + oldWidth < mColumns) ? (findStartOfColumn(columnToSet + oldWidth) - oldStart) : (mSpaceUsed - oldStart);
        if (newIsCombining && WcWidth.zeroWidthCharsCount(mText, oldStart, oldStart + oldUsed) >= MAX_COMBINING_CHARACTERS_PER_COLUMN) return;
        final int newUsed = Character.charCount(codePoint) + (newIsCombining ? oldUsed : 0);
        final int diff = newUsed - oldUsed;
        if (diff > 0) {
            if (mSpaceUsed + diff > mText.length) {
                char[] nt = new char[mText.length + mColumns];
                System.arraycopy(mText, 0, nt, 0, oldStart + oldUsed);
                System.arraycopy(mText, oldStart + oldUsed, nt, oldStart + newUsed, mSpaceUsed - (oldStart + oldUsed));
                mText = nt;
            } else System.arraycopy(mText, oldStart + oldUsed, mText, oldStart + newUsed, mSpaceUsed - (oldStart + oldUsed));
        } else if (diff < 0) System.arraycopy(mText, oldStart + oldUsed, mText, oldStart + newUsed, mSpaceUsed - (oldStart + oldUsed));
        mSpaceUsed += diff;
        Character.toChars(codePoint, mText, oldStart + (newIsCombining ? oldUsed : 0));
        if (oldWidth == 2 && newWidth == 1) insertSpaceAt(oldStart + newUsed);
        else if (oldWidth == 1 && newWidth == 2) handleWideOverwrite(columnToSet, oldStart + newUsed);
    }

    private void insertSpaceAt(int index) {
        if (mSpaceUsed + 1 > mText.length) {
            char[] nt = new char[mText.length + mColumns];
            System.arraycopy(mText, 0, nt, 0, index);
            System.arraycopy(mText, index, nt, index + 1, mSpaceUsed - index);
            mText = nt;
        } else {
            System.arraycopy(mText, index, mText, index + 1, mSpaceUsed - index);
        }
        mText[index] = ' ';
        mSpaceUsed++;
    }

    private void handleWideOverwrite(int col, int idx) {
        if (col >= mColumns - 1) return;
        if (col == mColumns - 2) mSpaceUsed = (short) idx;
        else {
            int nidx = idx;
            nidx += (Character.isHighSurrogate(mText[nidx]) ? 2 : 1);
            while (nidx < mSpaceUsed && WcWidth.width(mText, nidx) <= 0)
                nidx += (Character.isHighSurrogate(mText[nidx]) ? 2 : 1);
            System.arraycopy(mText, nidx, mText, idx, mSpaceUsed - nidx);
            mSpaceUsed -= (nidx - idx);
        }
    }

    boolean isBlank() {
        for (int i = 0; i < mSpaceUsed; i++) if (mText[i] != ' ') return false;
        return true;
    }

    public final void updateStatusAfterBatchWrite() {
        mSpaceUsed = (short) mColumns;
    }
}
