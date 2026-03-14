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
    private char[] mText;
    private long[] mStyle;
    
    // 共享内存引用（Rust 化模式 - 只读）
    private ByteBuffer mSharedBuffer;
    private int mRowOffset;  // 该行在共享内存中的偏移（以 cell 为单位）
    
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
     * @param sharedBuffer 共享内存缓冲区
     * @param rowOffset 该行在共享内存中的起始偏移（以 cell 为单位）
     * @param columns 列数
     */
    public TerminalRow(ByteBuffer sharedBuffer, int rowOffset, int columns) {
        mColumns = columns;
        mSharedBuffer = sharedBuffer;
        mRowOffset = rowOffset;
        mText = null;
        mStyle = null;
        mTextCache = new char[columns];
        mStyleCache = new long[columns];
        mCacheValid = false;
        mSpaceUsed = (short) columns;
        mLineWrap = false;
        mHasNonOneWidthOrSurrogateChars = false;
    }

    /**
     * 检查是否使用 Rust 后端（共享内存）
     */
    public boolean isRustBacked() {
        return mSharedBuffer != null;
    }

    /**
     * 从共享内存刷新缓存
     */
    private void refreshCache() {
        if (mSharedBuffer != null && !mCacheValid) {
            for (int col = 0; col < mColumns; col++) {
                mTextCache[col] = getCharUnsafe(col);
                mStyleCache[col] = getStyleUnsafe(col);
            }
            mCacheValid = true;
        }
    }

    /**
     * 获取指定列的字符（Rust 化模式从共享内存读取）
     */
    public char getChar(int column) {
        if (mSharedBuffer != null && column >= 0 && column < mColumns) {
            return getCharUnsafe(column);
        }
        return mText != null && column >= 0 && column < mText.length ? mText[column] : ' ';
    }

    /**
     * 获取字符（不检查边界，内部使用）
     */
    private char getCharUnsafe(int column) {
        int cellIndex = mRowOffset + column;
        int byteOffset = cellIndex * 10;  // u16(2) + u64(8) = 10 bytes per cell
        return mSharedBuffer.getChar(byteOffset);
    }

    /**
     * 获取指定列的样式（Rust 化模式从共享内存读取）
     */
    public long getStyle(int column) {
        if (mSharedBuffer != null && column >= 0 && column < mColumns) {
            return getStyleUnsafe(column);
        }
        return mStyle != null && column >= 0 && column < mStyle.length ? mStyle[column] : TextStyle.NORMAL;
    }

    /**
     * 获取样式（不检查边界，内部使用）
     */
    private long getStyleUnsafe(int column) {
        int cellIndex = mRowOffset + column;
        int byteOffset = cellIndex * 10 + 2;  // 跳过 2 字节的 u16
        return mSharedBuffer.getLong(byteOffset);
    }

    /**
     * 获取文本数组（Rust 化模式返回缓存）
     */
    public char[] getTextArray() {
        if (mSharedBuffer != null) {
            refreshCache();
            return mTextCache;
        }
        return mText;
    }

    /**
     * 获取样式数组（Rust 化模式返回缓存）
     */
    public long[] getStyleArray() {
        if (mSharedBuffer != null) {
            refreshCache();
            return mStyleCache;
        }
        return mStyle;
    }

    /**
     * 获取实际使用的空间
     */
    public int getSpaceUsed() {
        return mSpaceUsed;
    }

    /**
     * 设置共享内存缓冲区版本（用于 resize 后更新）
     */
    public void updateSharedBuffer(ByteBuffer sharedBuffer, int rowOffset) {
        mSharedBuffer = sharedBuffer;
        mRowOffset = rowOffset;
        mCacheValid = false;
    }

    public void copyInterval(TerminalRow line, int sourceX1, int sourceX2, int destinationX) {
        // Rust 化模式下不支持此操作
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
        // Rust 化模式下不支持此操作
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
        
        // Rust 化模式使用缓存
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
        // Rust 化模式下不支持此操作
        if (isRustBacked()) return;
        
        Arrays.fill(mText, ' ');
        Arrays.fill(mStyle, style);
        mSpaceUsed = (short) mColumns;
        mHasNonOneWidthOrSurrogateChars = false;
    }

    public final void setChar(int columnToSet, final int codePoint, final long style) {
        // Rust 化模式下不支持此操作
        if (isRustBacked()) return;
        
        if (columnToSet < 0 || columnToSet >= mColumns) return;

        final int newWidth = WcWidth.width(codePoint);

        // 设置样式：宽字符需要设置两列的样式
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
        if (!mHasNonOneWidthOrSurrogateChars) return mText[column];
        
        int idx = findStartOfColumn(column);
        if (idx >= mSpaceUsed) return ' ';
        char c = mText[idx];
        if (Character.isHighSurrogate(c) && idx + 1 < mSpaceUsed) {
            return Character.toCodePoint(c, mText[idx+1]);
        }
        return c;
    }

    /** 批量设置 ASCII 字符，仅在确定目标区域为单宽度字符且无组合字符时使用 */
    public final void setChars(int columnStart, byte[] buffer, int offset, int length, long style) {
        if (columnStart < 0 || columnStart + length > mColumns) return;
        
        // 批量更新样式
        Arrays.fill(mStyle, columnStart, columnStart + length, style);
        
        if (!mHasNonOneWidthOrSurrogateChars) {
            // 最快路径：直接拷贝字节到字符数组
            for (int i = 0; i < length; i++) {
                mText[columnStart + i] = (char) (buffer[offset + i] & 0xFF);
            }
            return;
        }
        
        // 如果行内已有复杂字符，则回退到逐个处理以保持内部变长存储的正确性
        for (int i = 0; i < length; i++) {
            setChar(columnStart + i, buffer[offset + i] & 0xFF, style);
        }
    }

    private void setCharInternal(int columnToSet, int codePoint, long style, int newWidth) {
        final boolean newIsCombining = newWidth <= 0;
        boolean wasWide = (columnToSet > 0) && wideDisplayCharacterStartingAt(columnToSet - 1);
        if (newIsCombining) { 
            if (wasWide) columnToSet--; 
        }
        else {
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
        if (oldWidth == 2 && newWidth == 1) insertSpaceAt(oldStart + newUsed, style);
        else if (oldWidth == 1 && newWidth == 2) handleWideOverwrite(columnToSet, oldStart + newUsed, style);
    }

    private void insertSpaceAt(int index, long style) {
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
        // 注意：样式数组不需要在这里更新，因为 insertSpaceAt 只在 setCharInternal 中被调用
        // 且调用前已经通过 setChar() 设置了正确的样式
    }

    private void handleWideOverwrite(int col, int idx, long style) {
        if (col >= mColumns - 1) return;
        if (col == mColumns - 2) mSpaceUsed = (short) idx;
        else {
            int nidx = idx;
            // 跳过要被覆盖的列起始字符及其所有组合字符
            nidx += (Character.isHighSurrogate(mText[nidx]) ? 2 : 1);
            while (nidx < mSpaceUsed && WcWidth.width(mText, nidx) <= 0)
                nidx += (Character.isHighSurrogate(mText[nidx]) ? 2 : 1);

            System.arraycopy(mText, nidx, mText, idx, mSpaceUsed - nidx);
            mSpaceUsed -= (nidx - idx);
        }
        // 样式已经在 setChar() 中设置好了
    }

    boolean isBlank() {
        for (int i = 0; i < mSpaceUsed; i++) if (mText[i] != ' ') return false;
        return true;
    }

    public final long getStyle(int column) {
        if (column < 0 || column >= mColumns) return TextStyle.NORMAL;
        if (mStyle == null) return TextStyle.NORMAL;
        return mStyle[column];
    }

    /** 在 Native 批量写入后调用，以同步 Java 层的状态 */
    public final void updateStatusAfterBatchWrite() {
        mSpaceUsed = (short) mColumns;
    }

    /**
     * 批量设置文本和样式（用于 Rust Full Takeover 优化）
     * 直接从 Rust 传输的数组复制数据，避免逐字符操作
     */
    public final void setTextAndStyles(char[] text, long[] styles) {
        if (text.length != mColumns || styles.length != mColumns) {
            throw new IllegalArgumentException("Text and styles length must match mColumns (" + mColumns + ")");
        }
        
        // 直接复制整个数组
        System.arraycopy(text, 0, mText, 0, Math.min(text.length, mText.length));
        System.arraycopy(styles, 0, mStyle, 0, mColumns);
        
        // 更新状态
        mSpaceUsed = (short) mColumns;
        mHasNonOneWidthOrSurrogateChars = false;
        
        // 检查是否有非单宽字符或代理对
        for (int i = 0; i < mColumns && i < text.length; i++) {
            char c = text[i];
            if (c >= 0xD800 && c <= 0xDFFF) {
                // 代理对
                mHasNonOneWidthOrSurrogateChars = true;
                break;
            }
        }
    }
}
