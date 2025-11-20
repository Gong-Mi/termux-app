package com.termux.view;

import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.opengl.GLES20;
import android.opengl.GLSurfaceView;
import android.opengl.GLUtils;
import android.util.Log;

import com.termux.terminal.TerminalBuffer;
import com.termux.terminal.TerminalEmulator;
import com.termux.terminal.TerminalRow;
import com.termux.terminal.TextStyle;
import com.termux.terminal.WcWidth;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.FloatBuffer;
import java.util.HashMap;
import java.util.Map;

import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;

public class TerminalRendererGLES implements GLSurfaceView.Renderer {

    private static final String TAG = TerminalRendererGLES.class.getSimpleName();

    private final String vertexShaderCode =
        "attribute vec4 a_Position;      \n" +
        "attribute vec2 a_TexCoordinate; \n" +
        "attribute vec4 a_Color;         \n" +
        "varying vec2 v_TexCoordinate;   \n" +
        "varying vec4 v_Color;           \n" +
        "void main() {                   \n" +
        "  v_TexCoordinate = a_TexCoordinate; \n" +
        "  v_Color = a_Color;            \n" +
        "  gl_Position = a_Position;     \n" +
        "}                              \n";

    private final String fragmentShaderCode =
        "precision mediump float;        \n" +
        "uniform sampler2D u_Texture;    \n" +
        "varying vec2 v_TexCoordinate;   \n" +
        "varying vec4 v_Color;           \n" +
        "void main() {                   \n" +
        "  float alpha = texture2D(u_Texture, v_TexCoordinate).a;\n" +
        "  gl_FragColor = vec4(v_Color.rgb, alpha); \n" +
        "}                              \n";

    private int mProgram;

    private FloatBuffer mVertexBuffer;
    private FloatBuffer mTextureBuffer;
    private FloatBuffer mColorBuffer;

    int mTextSize;
    Typeface mTypeface;
    float mFontWidth;
    int mFontLineSpacing;
    int mFontLineSpacingAndAscent;

    private TerminalEmulator mEmulator;
    private int mWidth;
    private int mHeight;

    private int mAtlasTextureId;
    private static final int ATLAS_TEXTURE_WIDTH = 1024;
    private static final int ATLAS_TEXTURE_HEIGHT = 1024;
    private int mAtlasNextX = 0;
    private int mAtlasNextY = 0;
    private int mAtlasLineHeight = 0;
    private final Map<Integer, GlyphMetrics> mGlyphCache = new HashMap<>();

    private static class GlyphMetrics {
        public final float width;
        public final android.graphics.RectF texCoords;

        GlyphMetrics(float width, android.graphics.RectF texCoords) {
            this.width = width;
            this.texCoords = texCoords;
        }
    }

    public TerminalRendererGLES(int textSize, Typeface typeface) {
        Log.d("TermuxDebug", "TerminalRendererGLES constructor");
        mTextSize = textSize;
        mTypeface = typeface;

        Paint paint = new Paint();
        paint.setTypeface(typeface);
        paint.setTextSize(textSize);

        mFontLineSpacing = (int) Math.ceil(paint.getFontSpacing());
        int mFontAscent = (int) Math.ceil(paint.ascent());
        mFontLineSpacingAndAscent = mFontLineSpacing + mFontAscent;
        mFontWidth = paint.measureText("X");
    }

    public void updateFont(int textSize, Typeface typeface) {
        Log.d("TermuxDebug", "updateFont - textSize: " + textSize + ", typeface: " + typeface.toString());
        mTextSize = textSize;
        mTypeface = typeface;

        Paint paint = new Paint();
        paint.setTypeface(typeface);
        paint.setTextSize(textSize);

        mFontLineSpacing = (int) Math.ceil(paint.getFontSpacing());
        int mFontAscent = (int) Math.ceil(paint.ascent());
        mFontLineSpacingAndAscent = mFontLineSpacing + mFontAscent;
        mFontWidth = paint.measureText("X");
    }

    public void setEmulator(TerminalEmulator emulator) {
        mEmulator = emulator;
    }

    private void initTextureAtlas() {
        mGlyphCache.clear();
        mAtlasNextX = 0;
        mAtlasNextY = 0;
        mAtlasLineHeight = 0;

        int[] textureIds = new int[1];
        GLES20.glGenTextures(1, textureIds, 0);
        mAtlasTextureId = textureIds[0];

        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, mAtlasTextureId);
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_NEAREST);
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_NEAREST);
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_S, GLES20.GL_CLAMP_TO_EDGE);
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_T, GLES20.GL_CLAMP_TO_EDGE);

        Bitmap bitmap = Bitmap.createBitmap(ATLAS_TEXTURE_WIDTH, ATLAS_TEXTURE_HEIGHT, Bitmap.Config.ALPHA_8);
        GLUtils.texImage2D(GLES20.GL_TEXTURE_2D, 0, bitmap, 0);
        bitmap.recycle();
    }

    private GlyphMetrics getGlyphMetrics(int codePoint) {
        if (mGlyphCache.containsKey(codePoint)) {
            return mGlyphCache.get(codePoint);
        }

        // The character is not in the cache. Render it to the texture atlas.
        Paint paint = new Paint();
        paint.setTypeface(mTypeface);
        paint.setTextSize(mTextSize);
        paint.setAntiAlias(true);
        paint.setColor(0xFFFFFFFF);

        String charString = new String(Character.toChars(codePoint));
        float charWidth = paint.measureText(charString);
        int charWidthInt = (int) Math.ceil(charWidth);
        int charHeightInt = mFontLineSpacing;

        if (mAtlasNextX + charWidthInt > ATLAS_TEXTURE_WIDTH) {
            mAtlasNextX = 0;
            mAtlasNextY += mAtlasLineHeight;
            mAtlasLineHeight = 0;
        }

        // FIXME: Check if we are out of atlas space.
        if (mAtlasLineHeight < charHeightInt) {
            mAtlasLineHeight = charHeightInt;
        }

        Bitmap glyphBitmap = Bitmap.createBitmap(charWidthInt, charHeightInt, Bitmap.Config.ALPHA_8);
        Canvas canvas = new Canvas(glyphBitmap);
        canvas.drawText(charString, 0, mFontLineSpacingAndAscent - mFontLineSpacing, paint);

        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, mAtlasTextureId);
        GLUtils.texSubImage2D(GLES20.GL_TEXTURE_2D, 0, mAtlasNextX, mAtlasNextY, glyphBitmap);
        glyphBitmap.recycle();

        float u1 = mAtlasNextX / (float) ATLAS_TEXTURE_WIDTH;
        float v1 = mAtlasNextY / (float) ATLAS_TEXTURE_HEIGHT;
        float u2 = (mAtlasNextX + charWidth) / (float) ATLAS_TEXTURE_WIDTH;
        float v2 = (mAtlasNextY + charHeightInt) / (float) ATLAS_TEXTURE_HEIGHT;
        android.graphics.RectF texCoords = new android.graphics.RectF(u1, v1, u2, v2);

        GlyphMetrics metrics = new GlyphMetrics(charWidth, texCoords);
        mGlyphCache.put(codePoint, metrics);

        mAtlasNextX += charWidthInt;

        return metrics;
    }

    private void generateMesh() {
        if (mEmulator == null) return;

        synchronized (mEmulator) {
            if (mEmulator.getScreen() == null || mEmulator.mColors == null || mEmulator.mColors.mCurrentColors == null) {
                if (mVertexBuffer != null) mVertexBuffer.clear();
                if (mTextureBuffer != null) mTextureBuffer.clear();
                if (mColorBuffer != null) mColorBuffer.clear();
                return;
            }

            final int columns = mEmulator.mColumns;
            final int rows = mEmulator.mRows;

            if (columns <= 0 || rows <= 0) return;

            int numCharacters = columns * rows;
            int numVertices = numCharacters * 6;

            if (mVertexBuffer == null || mVertexBuffer.capacity() < numVertices * 3) {
                mVertexBuffer = ByteBuffer.allocateDirect(numVertices * 3 * 4)
                    .order(ByteOrder.nativeOrder()).asFloatBuffer();
            }
            if (mTextureBuffer == null || mTextureBuffer.capacity() < numVertices * 2) {
                mTextureBuffer = ByteBuffer.allocateDirect(numVertices * 2 * 4)
                    .order(ByteOrder.nativeOrder()).asFloatBuffer();
            }
            if (mColorBuffer == null || mColorBuffer.capacity() < numVertices * 4) {
                mColorBuffer = ByteBuffer.allocateDirect(numVertices * 4 * 4)
                    .order(ByteOrder.nativeOrder()).asFloatBuffer();
            }

            mVertexBuffer.clear();
            mTextureBuffer.clear();
            mColorBuffer.clear();

            final TerminalBuffer screen = mEmulator.getScreen();
            final int[] palette = mEmulator.mColors.mCurrentColors;
            final float columnWidth = mFontWidth;

            for (int row = 0; row < rows; row++) {
                TerminalRow line = screen.allocateFullLineIfNecessary(screen.externalToInternalRow(row));
                final char[] text = line.mText;

                for (int col = 0; col < columns; ) {
                    char highSurrogate = text[col];
                    int codePoint = highSurrogate;
                    if (Character.isHighSurrogate(highSurrogate) && col + 1 < columns) {
                        char lowSurrogate = text[col + 1];
                        if (Character.isLowSurrogate(lowSurrogate)) {
                            codePoint = Character.toCodePoint(highSurrogate, lowSurrogate);
                        }
                    }
                    
                    int wcwidth = WcWidth.width(codePoint);
                    if (codePoint == 0 || wcwidth == 0) {
                        col++;
                        continue;
                    }

                    if (col + wcwidth > columns) {
                        col++;
                        continue;
                    }

                    GlyphMetrics metrics = getGlyphMetrics(codePoint);
                    if (metrics == null) {
                        col += wcwidth;
                        continue;
                    }

                    long style = line.getStyle(col);
                    int foreColor = TextStyle.decodeForeColor(style);

                    int color;
                    if ((style & TextStyle.CHARACTER_ATTRIBUTE_TRUECOLOR_FOREGROUND) == 0) {
                        if (foreColor >= 0 && foreColor < palette.length) {
                            color = palette[foreColor];
                        } else {
                            color = palette[TextStyle.COLOR_INDEX_FOREGROUND];
                        }
                    } else {
                        color = foreColor;
                    }

                    float x1_norm = (col * columnWidth / (float) mWidth) * 2.0f - 1.0f;
                    float y1_norm = -(((row * mFontLineSpacing) / (float) mHeight) * 2.0f - 1.0f);
                    float x2_norm = ((col + wcwidth) * columnWidth / (float) mWidth) * 2.0f - 1.0f;
                    float y2_norm = y1_norm - (mFontLineSpacing / (float) mHeight) * 2.0f;

                    mVertexBuffer.put(x1_norm); mVertexBuffer.put(y2_norm); mVertexBuffer.put(0.0f);
                    mVertexBuffer.put(x1_norm); mVertexBuffer.put(y1_norm); mVertexBuffer.put(0.0f);
                    mVertexBuffer.put(x2_norm); mVertexBuffer.put(y1_norm); mVertexBuffer.put(0.0f);
                    mVertexBuffer.put(x2_norm); mVertexBuffer.put(y1_norm); mVertexBuffer.put(0.0f);
                    mVertexBuffer.put(x2_norm); mVertexBuffer.put(y2_norm); mVertexBuffer.put(0.0f);
                    mVertexBuffer.put(x1_norm); mVertexBuffer.put(y2_norm); mVertexBuffer.put(0.0f);
                    
                    mTextureBuffer.put(metrics.texCoords.left); mTextureBuffer.put(metrics.texCoords.bottom);
                    mTextureBuffer.put(metrics.texCoords.left); mTextureBuffer.put(metrics.texCoords.top);
                    mTextureBuffer.put(metrics.texCoords.right); mTextureBuffer.put(metrics.texCoords.top);
                    mTextureBuffer.put(metrics.texCoords.right); mTextureBuffer.put(metrics.texCoords.top);
                    mTextureBuffer.put(metrics.texCoords.right); mTextureBuffer.put(metrics.texCoords.bottom);
                    mTextureBuffer.put(metrics.texCoords.left); mTextureBuffer.put(metrics.texCoords.bottom);

                    float red = ((color >> 16) & 0xFF) / 255.0f;
                    float green = ((color >> 8) & 0xFF) / 255.0f;
                    float blue = (color & 0xFF) / 255.0f;

                    for(int i = 0; i < 6; i++) {
                        mColorBuffer.put(red);
                        mColorBuffer.put(green);
                        mColorBuffer.put(blue);
                        mColorBuffer.put(1.0f);
                    }
                    col += wcwidth;
                }
            }

            mVertexBuffer.position(0);
            mTextureBuffer.position(0);
            mColorBuffer.position(0);
        }
    }

    private static int loadShader(int type, String shaderCode){
        int shader = GLES20.glCreateShader(type);
        GLES20.glShaderSource(shader, shaderCode);
        GLES20.glCompileShader(shader);
        return shader;
    }

    @Override
    public void onSurfaceCreated(GL10 unused, EGLConfig config) {
        // Set the background frame color to black.
        GLES20.glClearColor(0.0f, 0.0f, 0.0f, 1.0f);
        checkGlError("glClearColor");

        GLES20.glEnable(GLES20.GL_BLEND);
        checkGlError("glEnable(GL_BLEND)");
        GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA);
        checkGlError("glBlendFunc");

        Log.d(TAG, "onSurfaceCreated");

        int vertexShader = loadShader(GLES20.GL_VERTEX_SHADER, vertexShaderCode);
        checkGlError("loadShader(vertex)");
        int fragmentShader = loadShader(GLES20.GL_FRAGMENT_SHADER, fragmentShaderCode);
        checkGlError("loadShader(fragment)");

        mProgram = GLES20.glCreateProgram();
        checkGlError("glCreateProgram");
        GLES20.glAttachShader(mProgram, vertexShader);
        checkGlError("glAttachShader(vertex)");
        GLES20.glAttachShader(mProgram, fragmentShader);
        checkGlError("glAttachShader(fragment)");
        GLES20.glLinkProgram(mProgram);
        checkGlError("glLinkProgram");

        initTextureAtlas();
        checkGlError("initTextureAtlas");
    }

    @Override
    public void onSurfaceChanged(GL10 unused, int width, int height) {
        mWidth = width;
        mHeight = height;
        GLES20.glViewport(0, 0, width, height);
        checkGlError("glViewport");
        Log.d(TAG, "onSurfaceChanged: " + width + "x" + height);
    }

    @Override
    public void onDrawFrame(GL10 unused) {
        if (mEmulator == null) return;

        generateMesh();

        // Redraw the background color
        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT);
        checkGlError("glClear");
        GLES20.glUseProgram(mProgram);
        checkGlError("glUseProgram");

        GLES20.glActiveTexture(GLES20.GL_TEXTURE0);
        checkGlError("glActiveTexture");
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, mAtlasTextureId);
        checkGlError("glBindTexture");

        int positionHandle = GLES20.glGetAttribLocation(mProgram, "a_Position");
        checkGlError("glGetAttribLocation(a_Position)");
        GLES20.glEnableVertexAttribArray(positionHandle);
        checkGlError("glEnableVertexAttribArray(positionHandle)");
        GLES20.glVertexAttribPointer(positionHandle, 3, GLES20.GL_FLOAT, false, 0, mVertexBuffer);
        checkGlError("glVertexAttribPointer(positionHandle)");

        int texCoordHandle = GLES20.glGetAttribLocation(mProgram, "a_TexCoordinate");
        checkGlError("glGetAttribLocation(a_TexCoordinate)");
        GLES20.glEnableVertexAttribArray(texCoordHandle);
        checkGlError("glEnableVertexAttribArray(texCoordHandle)");
        GLES20.glVertexAttribPointer(texCoordHandle, 2, GLES20.GL_FLOAT, false, 0, mTextureBuffer);
        checkGlError("glVertexAttribPointer(texCoordHandle)");

        int colorHandle = GLES20.glGetAttribLocation(mProgram, "a_Color");
        checkGlError("glGetAttribLocation(a_Color)");
        GLES20.glEnableVertexAttribArray(colorHandle);
        checkGlError("glEnableVertexAttribArray(colorHandle)");
        GLES20.glVertexAttribPointer(colorHandle, 4, GLES20.GL_FLOAT, false, 0, mColorBuffer);
        checkGlError("glVertexAttribPointer(colorHandle)");

        int textureHandle = GLES20.glGetUniformLocation(mProgram, "u_Texture");
        checkGlError("glGetUniformLocation(u_Texture)");
        GLES20.glUniform1i(textureHandle, 0);
        checkGlError("glUniform1i");

        GLES20.glDrawArrays(GLES20.GL_TRIANGLES, 0, mEmulator.mColumns * mEmulator.mRows * 6);
        checkGlError("glDrawArrays");

        GLES20.glDisableVertexAttribArray(positionHandle);
        checkGlError("glDisableVertexAttribArray(positionHandle)");
        GLES20.glDisableVertexAttribArray(texCoordHandle);
        checkGlError("glDisableVertexAttribArray(texCoordHandle)");
        GLES20.glDisableVertexAttribArray(colorHandle);
        checkGlError("glDisableVertexAttribArray(colorHandle)");
    }

    public float getFontWidth() {
        return mFontWidth;
    }

    public int getFontLineSpacing() {
        return mFontLineSpacing;
    }

    private void checkGlError(String glOperation) {
        int error;
        while ((error = GLES20.glGetError()) != GLES20.GL_NO_ERROR) {
            Log.e(TAG, glOperation + ": glError " + error);
            // Optionally, you can throw a RuntimeException here to crash the app
            // and get a stack trace, but for now, just logging is fine.
        }
    }
}
