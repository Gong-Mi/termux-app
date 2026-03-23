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

/** Renderer of a {@link TerminalEmulator} into a {@link Canvas}. */
public final class TerminalRenderer {

    public final Paint mTextPaint = new Paint();

    /** The width of a single mono spaced character. */
    public float mFontWidth;
    /** The 128 first characters (ASCII) width. */
    private final float[] asciiMeasures = new float[128];

    /** The font line spacing. */
    public int mFontLineSpacing;
    /** The font line spacing and ascent. */
    public int mFontLineSpacingAndAscent;
    /** The font ascent. */
    public int mFontAscent;

    public final int mTextSize;
    public final Typeface mTypeface;

    public TerminalRenderer(int textSize, Typeface typeface) {
        mTextSize = textSize;
        mTypeface = typeface;
        mTextPaint.setTypeface(typeface);
        mTextPaint.setTextSize(textSize);
        mTextPaint.setAntiAlias(true);

        mFontAscent = (int) Math.ceil(mTextPaint.ascent());
        mFontLineSpacing = (int) Math.ceil(mTextPaint.getFontSpacing());
        mFontLineSpacingAndAscent = mFontLineSpacing + mFontAscent;
        mFontWidth = mTextPaint.measureText("X");

        StringBuilder sb = new StringBuilder("X");
        for (int i = 0; i < asciiMeasures.length; i++) {
            sb.setCharAt(0, (char) i);
            asciiMeasures[i] = mTextPaint.measureText(sb, 0, 1);
        }
    }

    public float getFontWidth() {
        return mFontWidth;
    }

    public int getFontLineSpacing() {
        return mFontLineSpacing;
    }

    private int[] mRowCodePointBuffer;
    private long[] mRowStyleBuffer;

    public final void render(TerminalEmulator mEmulator, Canvas canvas, int topRow,
                             int selectionY1, int selectionY2, int selectionX1, int selectionX2) {
        if (mEmulator == null) return;
        
        final int columns = mEmulator.getCols();
        final int rows = mEmulator.getRows();
        if (columns <= 0 || rows <= 0) return;

        final boolean reverseVideo = mEmulator.isReverseVideo();
        final int endRow = topRow + rows;
        final int cursorCol = mEmulator.getCursorCol();
        final int cursorRow = mEmulator.getCursorRow();
        final boolean cursorVisible = mEmulator.shouldCursorBeVisible();
        final int[] palette = mEmulator.getCurrentColors();
        final int cursorShape = mEmulator.getCursorStyle();

        if (reverseVideo)
            canvas.drawColor(palette[TextStyle.COLOR_INDEX_FOREGROUND], PorterDuff.Mode.SRC);

        float heightOffset = mFontLineSpacingAndAscent;

        for (int row = topRow; row < endRow; row++) {
            heightOffset += mFontLineSpacing;

            final int cursorX = (row == cursorRow && cursorVisible) ? cursorCol : -1;
            int selx1 = -1, selx2 = -1;
            if (row >= selectionY1 && row <= selectionY2) {
                if (row == selectionY1) selx1 = selectionX1;
                selx2 = (row == selectionY2) ? selectionX2 : mEmulator.getCols();
            }

            if (mRowCodePointBuffer == null || mRowCodePointBuffer.length != columns) {
                mRowCodePointBuffer = new int[columns];
                mRowStyleBuffer = new long[columns];
            }
            
            mEmulator.readRow(row, mRowCodePointBuffer, mRowStyleBuffer);
            int[] line = mRowCodePointBuffer;
            long[] styles = mRowStyleBuffer;

            long lastRunStyle = 0;
            boolean lastRunInsideCursor = false;
            boolean lastRunInsideSelection = false;
            int lastRunStartColumn = -1;
            boolean lastRunFontWidthMismatch = false;
            float measuredWidthForRun = 0.f;

            for (int column = 0; column < columns; ) {
                final int codePoint = line[column];
                if (codePoint == 0) { column++; continue; } // 跳过占位符

                final int codePointWcWidth = WcWidth.width(codePoint);
                final boolean insideCursor = (cursorX == column || (codePointWcWidth == 2 && cursorX == column + 1));
                final boolean insideSelection = column >= selx1 && column <= selx2;
                final long style = styles[column];

                final float measuredCodePointWidth = (codePoint < asciiMeasures.length) ? asciiMeasures[codePoint] : 
                    mTextPaint.measureText(new String(new int[]{codePoint}, 0, 1));
                
                final boolean fontWidthMismatch = Math.abs(measuredCodePointWidth / mFontWidth - Math.max(1, codePointWcWidth)) > 0.01;

                if (column == 0 || style != lastRunStyle || insideCursor != lastRunInsideCursor || 
                    insideSelection != lastRunInsideSelection || fontWidthMismatch != lastRunFontWidthMismatch) {
                    
                    if (column != 0) {
                        renderRun(canvas, line, lastRunStartColumn, column - lastRunStartColumn, heightOffset, 
                                  measuredWidthForRun, lastRunStyle, lastRunInsideCursor, 
                                  lastRunInsideSelection, reverseVideo, palette, cursorColorForRun(lastRunInsideCursor, palette), 
                                  cursorShape);
                    }
                    
                    lastRunStyle = style;
                    lastRunInsideCursor = insideCursor;
                    lastRunInsideSelection = insideSelection;
                    lastRunStartColumn = column;
                    lastRunFontWidthMismatch = fontWidthMismatch;
                    measuredWidthForRun = 0.f;
                }
                
                measuredWidthForRun += measuredCodePointWidth;
                column += Math.max(1, codePointWcWidth);
            }
            
            if (columns > lastRunStartColumn && lastRunStartColumn != -1) {
                renderRun(canvas, line, lastRunStartColumn, columns - lastRunStartColumn, heightOffset, 
                          measuredWidthForRun, lastRunStyle, lastRunInsideCursor, 
                          lastRunInsideSelection, reverseVideo, palette, cursorColorForRun(lastRunInsideCursor, palette), 
                          cursorShape);
            }
        }
    }

    private int cursorColorForRun(boolean inside, int[] palette) {
        return inside ? palette[TextStyle.COLOR_INDEX_CURSOR] : 0;
    }

    private void renderRun(Canvas canvas, int[] line, int offset, int count, float y, float measuredWidth, 
                           long style, boolean insideCursor, boolean insideSelection, boolean globalReverse, 
                           int[] palette, int cursorColor, int cursorShape) {
        
        String text = new String(line, offset, count);
        drawTextRun(canvas, text, palette, y, offset, count, measuredWidth, cursorColor, cursorShape, style, 
                    globalReverse || insideSelection);
    }

    private void drawTextRun(Canvas canvas, String text, int[] palette, float y, int startColumn, int runWidthColumns,
                             float mes, int cursor, int cursorStyle, long textStyle, boolean reverseVideo) {
        int foreColor = TextStyle.decodeForeColor(textStyle);
        final int effect = TextStyle.decodeEffect(textStyle);
        int backColor = TextStyle.decodeBackColor(textStyle);
        
        final boolean bold = (effect & (TextStyle.CHARACTER_ATTRIBUTE_BOLD | TextStyle.CHARACTER_ATTRIBUTE_BLINK)) != 0;
        
        if ((foreColor & 0xff000000) != 0xff000000) {
            if (bold && foreColor >= 0 && foreColor < 8) foreColor += 8;
            foreColor = palette[foreColor];
        }
        if ((backColor & 0xff000000) != 0xff000000) {
            backColor = palette[backColor];
        }

        final boolean reverseVideoHere = reverseVideo ^ (effect & (TextStyle.CHARACTER_ATTRIBUTE_INVERSE)) != 0;
        if (reverseVideoHere) {
            int tmp = foreColor; foreColor = backColor; backColor = tmp;
        }

        float left = startColumn * mFontWidth;
        float right = left + runWidthColumns * mFontWidth;

        if (backColor != palette[TextStyle.COLOR_INDEX_BACKGROUND]) {
            mTextPaint.setColor(backColor);
            canvas.drawRect(left, y - mFontLineSpacingAndAscent + mFontAscent, right, y, mTextPaint);
        }

        if (cursor != 0) {
            mTextPaint.setColor(cursor);
            float cursorHeight = mFontLineSpacingAndAscent - mFontAscent;
            if (cursorStyle == TerminalEmulator.TERMINAL_CURSOR_STYLE_UNDERLINE) cursorHeight /= 4.;
            else if (cursorStyle == TerminalEmulator.TERMINAL_CURSOR_STYLE_BAR) right -= ((right - left) * 3) / 4.;
            canvas.drawRect(left, y - cursorHeight, right, y, mTextPaint);
        }

        mTextPaint.setColor(foreColor);
        mTextPaint.setFakeBoldText(bold);
        mTextPaint.setUnderlineText((effect & TextStyle.CHARACTER_ATTRIBUTE_UNDERLINE) != 0);
        mTextPaint.setStrikeThruText((effect & TextStyle.CHARACTER_ATTRIBUTE_STRIKETHROUGH) != 0);
        
        canvas.drawText(text, left, y, mTextPaint);
    }
}
