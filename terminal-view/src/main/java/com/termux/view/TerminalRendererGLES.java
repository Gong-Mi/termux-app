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

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.FloatBuffer;

import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;

public class TerminalRendererGLES implements GLSurfaceView.Renderer {

    private static final String TAG = "TerminalRendererGLES";

    private final String vertexShaderCode =
        "attribute vec4 a_Position;      \n" +
        "attribute vec2 a_TexCoordinate; \n" +
        "varying vec2 v_TexCoordinate;   \n" +
        "void main() {                   \n" +
        "  v_TexCoordinate = a_TexCoordinate; \n" +
        "  gl_Position = a_Position;     \n" +
        "}                              \n";

    private final String fragmentShaderCode =
        "precision mediump float;        \n" +
        "uniform sampler2D u_Texture;    \n" +
        "varying vec2 v_TexCoordinate;   \n" +
        "uniform vec4 a_Color;           \n" +
        "void main() {                   \n" +
        "  gl_FragColor = texture2D(u_Texture, v_TexCoordinate) * a_Color; \n" +
        "}                              \n";

    private int mProgram;
    private int mTextureId;

    private FloatBuffer mVertexBuffer;
    private FloatBuffer mTextureBuffer;
    private FloatBuffer mColorBuffer;

    final int mTextSize;
    final Typeface mTypeface;
    final float mFontWidth;
    final int mFontLineSpacing;
    final int mFontLineSpacingAndAscent;

    private TerminalEmulator mEmulator;
    private int mWidth;
    private int mHeight;

    public TerminalRendererGLES(int textSize, Typeface typeface) {
        mTextSize = textSize;
        mTypeface = typeface;

        Paint paint = new Paint();
        paint.setTypeface(typeface);
        paint.setTextSize(textSize);

        mFontLineSpacing = (int) Math.ceil(paint.getFontSpacing());
        int mFontAscent = (int) Math.ceil(paint.ascent());
        mFontLineSpacingAndAscent = mFontLineSpacing + mFontAscent;
        mFontWidth = paint.measureText("X");
        Log.d(TAG, "mFontWidth: " + mFontWidth + ", mFontLineSpacing: " + mFontLineSpacing);
    }

    public void setEmulator(TerminalEmulator emulator) {
        mEmulator = emulator;
    }

    private void createFontTexture() {
        int[] textureIds = new int[1];
        GLES20.glGenTextures(1, textureIds, 0);
        mTextureId = textureIds[0];

        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, mTextureId);

        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_NEAREST);
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_NEAREST);
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_S, GLES20.GL_CLAMP_TO_EDGE);
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_T, GLES20.GL_CLAMP_TO_EDGE);

        int textureWidth = (int) (mFontWidth * 95); // ASCII 32-126
        int textureHeight = mFontLineSpacing;

        Bitmap bitmap = Bitmap.createBitmap(textureWidth, textureHeight, Bitmap.Config.ALPHA_8);
        Canvas canvas = new Canvas(bitmap);
        canvas.drawColor(0x00000000); // Transparent background

        Paint paint = new Paint();
        paint.setTypeface(mTypeface);
        paint.setTextSize(mTextSize);
        paint.setColor(0xFFFFFFFF); // White text

        for (int i = 32; i < 127; i++) {
            char c = (char) i;
            canvas.drawText(String.valueOf(c), (i - 32) * mFontWidth, mFontLineSpacingAndAscent - mFontLineSpacing, paint);
        }

        GLUtils.texImage2D(GLES20.GL_TEXTURE_2D, 0, bitmap, 0);
        bitmap.recycle();
    }

    private void generateMesh() {
        if (mEmulator == null) return;

        int columns = mEmulator.mColumns;
        int rows = mEmulator.mRows;

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

        TerminalBuffer screen = mEmulator.getScreen();
        int[] palette = mEmulator.mColors.mCurrentColors;

        for (int row = 0; row < rows; row++) {
            TerminalRow line = screen.allocateFullLineIfNecessary(screen.externalToInternalRow(row));
            for (int col = 0; col < columns; col++) {
                char c = line.mText[col];
                if (c == 0) c = ' '; // Replace null characters with spaces

                long style = line.getStyle(col);
                int foreColor = TextStyle.decodeForeColor(style);
                int backColor = TextStyle.decodeBackColor(style);
                int color = palette[foreColor];

                float x1 = (col * mFontWidth / (float) mWidth) * 2.0f - 1.0f;
                float y1 = -(((row * mFontLineSpacing) / (float) mHeight) * 2.0f - 1.0f);
                float x2 = x1 + (mFontWidth / (float) mWidth) * 2.0f;
                float y2 = y1 - (mFontLineSpacing / (float) mHeight) * 2.0f;

                mVertexBuffer.put(x1); mVertexBuffer.put(y2); mVertexBuffer.put(0.0f);
                mVertexBuffer.put(x1); mVertexBuffer.put(y1); mVertexBuffer.put(0.0f);
                mVertexBuffer.put(x2); mVertexBuffer.put(y1); mVertexBuffer.put(0.0f);

                mVertexBuffer.put(x2); mVertexBuffer.put(y1); mVertexBuffer.put(0.0f);
                mVertexBuffer.put(x2); mVertexBuffer.put(y2); mVertexBuffer.put(0.0f);
                mVertexBuffer.put(x1); mVertexBuffer.put(y2); mVertexBuffer.put(0.0f);

                float u1 = ((c - 32) * mFontWidth) / (mFontWidth * 95);
                float v1 = 0.0f;
                float u2 = u1 + (mFontWidth / (mFontWidth * 95));
                float v2 = 1.0f;

                mTextureBuffer.put(u1); mTextureBuffer.put(v1);
                mTextureBuffer.put(u1); mTextureBuffer.put(v2);
                mTextureBuffer.put(u2); mTextureBuffer.put(v2);

                mTextureBuffer.put(u2); mTextureBuffer.put(v2);
                mTextureBuffer.put(u2); mTextureBuffer.put(v1);
                mTextureBuffer.put(u1); mTextureBuffer.put(v1);

                float red = ((color >> 16) & 0xFF) / 255.0f;
                float green = ((color >> 8) & 0xFF) / 255.0f;
                float blue = (color & 0xFF) / 255.0f;

                for(int i = 0; i < 6; i++) {
                    mColorBuffer.put(red);
                    mColorBuffer.put(green);
                    mColorBuffer.put(blue);
                    mColorBuffer.put(1.0f);
                }
            }
        }

        mVertexBuffer.position(0);
        mTextureBuffer.position(0);
        mColorBuffer.position(0);
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
        Log.d(TAG, "onSurfaceCreated");

        int vertexShader = loadShader(GLES20.GL_VERTEX_SHADER, vertexShaderCode);
        int fragmentShader = loadShader(GLES20.GL_FRAGMENT_SHADER, fragmentShaderCode);

        mProgram = GLES20.glCreateProgram();
        GLES20.glAttachShader(mProgram, vertexShader);
        GLES20.glAttachShader(mProgram, fragmentShader);
        GLES20.glLinkProgram(mProgram);

        createFontTexture();
    }

    @Override
    public void onSurfaceChanged(GL10 unused, int width, int height) {
        mWidth = width;
        mHeight = height;
        GLES20.glViewport(0, 0, width, height);
        Log.d(TAG, "onSurfaceChanged: " + width + "x" + height);
    }

    @Override
    public void onDrawFrame(GL10 unused) {
        if (mEmulator == null) return;

        generateMesh();

        // Redraw the background color
        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT);
        GLES20.glUseProgram(mProgram);

        GLES20.glActiveTexture(GLES20.GL_TEXTURE0);
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, mTextureId);

        int positionHandle = GLES20.glGetAttribLocation(mProgram, "a_Position");
        GLES20.glEnableVertexAttribArray(positionHandle);
        GLES20.glVertexAttribPointer(positionHandle, 3, GLES20.GL_FLOAT, false, 0, mVertexBuffer);

        int texCoordHandle = GLES20.glGetAttribLocation(mProgram, "a_TexCoordinate");
        GLES20.glEnableVertexAttribArray(texCoordHandle);
        GLES20.glVertexAttribPointer(texCoordHandle, 2, GLES20.GL_FLOAT, false, 0, mTextureBuffer);

        int colorHandle = GLES20.glGetAttribLocation(mProgram, "a_Color");
        GLES20.glEnableVertexAttribArray(colorHandle);
        GLES20.glVertexAttribPointer(colorHandle, 4, GLES20.GL_FLOAT, false, 0, mColorBuffer);

        int textureHandle = GLES20.glGetUniformLocation(mProgram, "u_Texture");
        GLES20.glUniform1i(textureHandle, 0);

        GLES20.glDrawArrays(GLES20.GL_TRIANGLES, 0, mEmulator.mColumns * mEmulator.mRows * 6);

        GLES20.glDisableVertexAttribArray(positionHandle);
        GLES20.glDisableVertexAttribArray(texCoordHandle);
        GLES20.glDisableVertexAttribArray(colorHandle);
    }

    public float getFontWidth() {
        return mFontWidth;
    }

    public int getFontLineSpacing() {
        return mFontLineSpacing;
    }
}