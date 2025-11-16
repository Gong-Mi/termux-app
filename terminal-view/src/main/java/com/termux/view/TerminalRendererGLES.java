package com.termux.view;

import android.opengl.GLES20;
import android.opengl.GLSurfaceView;
import android.util.Log;

import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;

public class TerminalRendererGLES implements GLSurfaceView.Renderer {

    private static final String TAG = "TerminalRendererGLES";

    @Override
    public void onSurfaceCreated(GL10 unused, EGLConfig config) {
        // Set the background frame color to red. This gives us a visual cue
        // that our GLES renderer is active.
        GLES20.glClearColor(1.0f, 0.0f, 0.0f, 1.0f);
        Log.d(TAG, "onSurfaceCreated");
    }

    @Override
    public void onSurfaceChanged(GL10 unused, int width, int height) {
        GLES20.glViewport(0, 0, width, height);
        Log.d(TAG, "onSurfaceChanged: " + width + "x" + height);
    }

    @Override
    public void onDrawFrame(GL10 unused) {
        // Redraw the background color
        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT);
        // Log.d(TAG, "onDrawFrame"); // This is too noisy to keep enabled
    }
}
