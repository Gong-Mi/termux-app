package com.termux.view;

import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.PorterDuff;
import android.graphics.Typeface;

import com.termux.terminal.TerminalBuffer;
import com.termux.terminal.TerminalEmulator;
import com.termux.terminal.TerminalRow;
import com.termux.terminal.TextStyle;
import com.termux.terminal.WcWidth;

/**
 * Renderer of a {@link TerminalEmulator} into a {@link Canvas}.
 * <p/>
 * Saves font metrics, so needs to be recreated each time the typeface or font size changes.
 */
public final class TerminalRenderer {

    final int mTextSize;
    final Typeface mTypeface;
    private final Paint mTextPaint = new Paint();

    /** The width of a single mono spaced character obtained by {@link Paint#measureText(String)} on a single 'X'. */
    final float mFontWidth;
    /** The {@link Paint#getFontSpacing()}. See http://www.fampennings.nl/maarten/android/08numgrid/font.png */
    final int mFontLineSpacing;
    /** The {@link Paint#ascent()}. See http://www.fampennings.nl/maarten/android/08numgrid/font.png */
    private final int mFontAscent;
    /** The {@link #mFontLineSpacing} + {@link #mFontAscent}. */
    final int mFontLineSpacingAndAscent;

    private final float[] asciiMeasures = new float[127];
    private static final java.util.HashMap<Integer, Float> mUnicodeMeasureCache = new java.util.HashMap<>();
    private static int sLastCacheTextSize = -1;
    private static Typeface sLastCacheTypeface = null;

    public TerminalRenderer(int textSize, Typeface typeface) {
        mTextSize = textSize;
        mTypeface = typeface;

        if (sLastCacheTextSize != textSize || sLastCacheTypeface != typeface) {
            mUnicodeMeasureCache.clear();
            sLastCacheTextSize = textSize;
            sLastCacheTypeface = typeface;
        }

        mTextPaint.setTypeface(typeface);
        mTextPaint.setTextSize(textSize);
        mTextPaint.setAntiAlias(true);

        mFontLineSpacing = (int) Math.ceil(mTextPaint.getFontSpacing());
        mFontAscent = (int) Math.ceil(mTextPaint.ascent());
        mFontLineSpacingAndAscent = mFontLineSpacing + mFontAscent;
        mFontWidth = mTextPaint.measureText("X");

        for (int i = 0; i < asciiMeasures.length; i++) {
            asciiMeasures[i] = mTextPaint.measureText(new String(new char[]{(char) i}));
        }
    }

    public float getFontWidth() {
        return mFontWidth;
    }

    public int getFontLineSpacing() {
        return mFontLineSpacing;
    }

    /**
     * Alias for {@link #draw} to maintain compatibility with {@link TerminalView}.
     */
    public void render(TerminalEmulator mEmulator, Canvas canvas, int topRow,
                       int selectionY1, int selectionY2, int selectionX1, int selectionX2) {
        draw(mEmulator, canvas, topRow, selectionY1, selectionY2, selectionX1, selectionX2);
    }

    /**
     * Render the terminal to a canvas.
     *
     * @param mEmulator   the emulator to render
     * @param canvas      the canvas to render into
     * @param topRow      the first row to render
     * @param selectionY1 the first row of selection
     * @param selectionY2 the last row of selection
     * @param selectionX1 the first column of selection
     * @param selectionX2 the last column of selection
     */
    public void draw(TerminalEmulator mEmulator, Canvas canvas, int topRow,
                             int selectionY1, int selectionY2, int selectionX1, int selectionX2) {
        final boolean reverseVideo = mEmulator.isReverseVideo();
        final int rows = mEmulator.mRows;
        final int columns = mEmulator.mColumns;
        final int endRow = topRow + rows;
        final int cursorCol = mEmulator.getCursorCol();
        final int cursorRow = mEmulator.getCursorRow();
        final boolean cursorVisible = mEmulator.shouldCursorBeVisible();
        final int[] palette = mEmulator.mColors.mCurrentColors;
        final int cursorShape = mEmulator.getCursorStyle();

        if (reverseVideo)
            canvas.drawColor(palette[TextStyle.COLOR_INDEX_FOREGROUND], PorterDuff.Mode.SRC);

        float heightOffset = mFontLineSpacingAndAscent;

        // 预分配缓冲区，减少行循环内的内存抖动
        // 使用与 TerminalRow 相同的 SPARE_CAPACITY_FACTOR (1.5f) 计算缓冲区大小
        // 参考：TerminalRow.java: mText = new char[(int) (SPARE_CAPACITY_FACTOR * columns)]
        final int spareCapacityFactor = 3 / 2; // 1.5f 的整数表示
        char[] lineText = new char[(columns * spareCapacityFactor + 1) / 2 * 2]; // 确保是偶数，考虑代理对
        long[] lineStyles = new long[columns];

        for (int row = topRow; row < endRow; row++) {
            heightOffset += mFontLineSpacing;

            final int cursorX = (row == cursorRow && cursorVisible) ? cursorCol : -1;
            int selx1 = -1, selx2 = -1;
            if (row >= selectionY1 && row <= selectionY2) {
                if (row == selectionY1) selx1 = selectionX1;
                selx2 = (row == selectionY2) ? selectionX2 : columns;
            }

            // 关键：从 Emulator 拉取内容，获取实际字符数用于边界检查
            int lineTextLength = mEmulator.getRowContentWithLength(row, lineText, lineStyles);

            long lastRunStyle = 0;
            boolean lastRunInsideCursor = false;
            boolean lastRunInsideSelection = false;
            int lastRunStartColumn = -1;
            int lastRunStartIndex = 0;
            boolean lastRunFontWidthMismatch = false;
            int currentCharIndex = 0;
            float measuredWidthForRun = 0.f;

            float lastRunScaleFactor = 1.0f;
            for (int column = 0; column < columns; ) {
                // 边界检查：防止 currentCharIndex 超出实际数据范围
                if (currentCharIndex >= lineTextLength || currentCharIndex >= lineText.length) {
                    // 如果已到达行尾，绘制剩余的 run 并退出
                    if (lastRunStartColumn != -1) {
                        drawTextRun(canvas, lineText, palette, heightOffset, lastRunStartColumn, lastRunStartIndex, currentCharIndex,
                            lastRunInsideCursor, lastRunInsideSelection, lastRunStyle, cursorShape, measuredWidthForRun, lastRunScaleFactor);
                    }
                    break;
                }

                final char charAtIndex = lineText[currentCharIndex];
                final boolean charIsHighsurrogate = Character.isHighSurrogate(charAtIndex);

                // 边界检查：确保不会访问超出数组范围的字符
                if (charIsHighsurrogate && currentCharIndex + 1 >= lineTextLength) {
                    // 不完整的代理对，视为无效字符，跳过
                    if (lastRunStartColumn != -1) {
                        drawTextRun(canvas, lineText, palette, heightOffset, lastRunStartColumn, lastRunStartIndex, currentCharIndex,
                            lastRunInsideCursor, lastRunInsideSelection, lastRunStyle, cursorShape, measuredWidthForRun, lastRunScaleFactor);
                    }
                    break;
                }

                final int charsForCodePoint = charIsHighsurrogate ? 2 : 1;
                final int codePoint = charIsHighsurrogate ? Character.toCodePoint(charAtIndex, lineText[currentCharIndex + 1]) : charAtIndex;
                final int codePointWcWidth = WcWidth.width(codePoint);
                final boolean isWideChar = codePointWcWidth == 2;
                final long style = lineStyles[column];

                final boolean insideCursor = column == cursorX || (isWideChar && (column + 1) == cursorX);
                final boolean insideSelection = column >= selx1 && column <= selx2;
                final boolean fontWidthMismatch = codePoint < asciiMeasures.length && asciiMeasures[codePoint] != mFontWidth;

                if (style != lastRunStyle || insideCursor != lastRunInsideCursor || insideSelection != lastRunInsideSelection || fontWidthMismatch || lastRunFontWidthMismatch) {
                    if (lastRunStartColumn != -1) {
                        drawTextRun(canvas, lineText, palette, heightOffset, lastRunStartColumn, lastRunStartIndex, currentCharIndex,
                            lastRunInsideCursor, lastRunInsideSelection, lastRunStyle, cursorShape, measuredWidthForRun, lastRunScaleFactor);
                    }
                    lastRunStartIndex = currentCharIndex;
                    lastRunStartColumn = column;
                    lastRunStyle = style;
                    lastRunInsideCursor = insideCursor;
                    lastRunInsideSelection = insideSelection;
                    lastRunFontWidthMismatch = fontWidthMismatch;
                    measuredWidthForRun = 0.f;
                    lastRunScaleFactor = 1.0f;
                }

                if (isWideChar) {
                    column += 2;
                } else {
                    column++;
                }

                if (codePoint < asciiMeasures.length) {
                    measuredWidthForRun += asciiMeasures[codePoint];
                } else {
                    Float cachedWidth = mUnicodeMeasureCache.get(codePoint);
                    if (cachedWidth == null) {
                        cachedWidth = mTextPaint.measureText(lineText, currentCharIndex, charsForCodePoint);
                        mUnicodeMeasureCache.put(codePoint, cachedWidth);
                    }
                    measuredWidthForRun += cachedWidth;
                }
                currentCharIndex += charsForCodePoint;
            }
            if (lastRunStartColumn != -1) {
                drawTextRun(canvas, lineText, palette, heightOffset, lastRunStartColumn, lastRunStartIndex, currentCharIndex,
                    lastRunInsideCursor, lastRunInsideSelection, lastRunStyle, cursorShape, measuredWidthForRun, lastRunScaleFactor);
            }
        }
    }

    private void drawTextRun(Canvas canvas, char[] text, int[] palette, float y, int startColumn, int startIndex, int endIndex,
                             boolean insideCursor, boolean insideSelection, long style, int cursorShape, float measuredWidthForRun, float scaleFactor) {
        int foregroundColor = TextStyle.decodeForeColor(style);
        int backgroundColor = TextStyle.decodeBackColor(style);
        final int effect = TextStyle.decodeEffect(style);

        // Reverse video for selection: swap foreground and background colors
        final boolean reverseVideoHere = insideSelection ^ (effect & TextStyle.CHARACTER_ATTRIBUTE_INVERSE) != 0;
        if (reverseVideoHere) {
            int tmp = foregroundColor;
            foregroundColor = backgroundColor;
            backgroundColor = tmp;
        }

        // Map indexed colors to palette (non-truecolor)
        // After reverse video swap, the foreground/background may have swapped, so we need to check
        // which original color (foreground or background) is now in each variable
        boolean foregroundIsIndexedColor, backgroundIsIndexedColor;

        if (reverseVideoHere) {
            // After swap: foreground was original background, background was original foreground
            foregroundIsIndexedColor = (style & TextStyle.CHARACTER_ATTRIBUTE_TRUECOLOR_BACKGROUND) == 0;
            backgroundIsIndexedColor = (style & TextStyle.CHARACTER_ATTRIBUTE_TRUECOLOR_FOREGROUND) == 0;
        } else {
            foregroundIsIndexedColor = (style & TextStyle.CHARACTER_ATTRIBUTE_TRUECOLOR_FOREGROUND) == 0;
            backgroundIsIndexedColor = (style & TextStyle.CHARACTER_ATTRIBUTE_TRUECOLOR_BACKGROUND) == 0;
        }

        // Process foreground color
        if (foregroundIsIndexedColor) {
            if (foregroundColor < 0 || foregroundColor >= palette.length) {
                foregroundColor = TextStyle.COLOR_INDEX_FOREGROUND;
            }
            foregroundColor = palette[foregroundColor];
        }
        // else: truecolor, keep the ARGB value as-is

        // Process background color
        if (backgroundIsIndexedColor) {
            if (backgroundColor < 0 || backgroundColor >= palette.length) {
                backgroundColor = TextStyle.COLOR_INDEX_BACKGROUND;
            }
            backgroundColor = palette[backgroundColor];
        }
        // else: truecolor, keep the ARGB value as-is

        if (insideCursor) {
            backgroundColor = palette[TextStyle.COLOR_INDEX_CURSOR];
            foregroundColor = palette[TextStyle.COLOR_INDEX_FOREGROUND];
        }

        if (backgroundColor != palette[TextStyle.COLOR_INDEX_BACKGROUND]) {
            mTextPaint.setColor(backgroundColor);
            float width = (endIndex - startIndex) * mFontWidth; // Simplified
            canvas.drawRect(startColumn * mFontWidth, y - mFontLineSpacingAndAscent + mFontAscent, (startColumn * mFontWidth) + width, y, mTextPaint);
        }

        mTextPaint.setColor(foregroundColor);
        canvas.drawText(text, startIndex, endIndex - startIndex, startColumn * mFontWidth, y, mTextPaint);
    }
}
