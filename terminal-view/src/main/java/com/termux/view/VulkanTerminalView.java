package com.termux.hg.view;

import android.content.Context;
import android.util.AttributeSet;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;

public class VulkanTerminalView extends SurfaceView implements SurfaceHolder.Callback {

    private RenderThread mRenderThread;

    static {
        System.loadLibrary("termux-vulkan");
    }

    public VulkanTerminalView(Context context, AttributeSet attrs) {
        super(context, attrs);
        getHolder().addCallback(this);
        nativeInit();
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        // Surface is created, but we wait for surfaceChanged to get dimensions
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        if (width == 0 || height == 0) {
            return;
        }

        nativeSetSurface(holder.getSurface());

        if (mRenderThread == null) {
            mRenderThread = new RenderThread();
            mRenderThread.start();
        }
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        if (mRenderThread != null) {
            mRenderThread.interrupt();
            try {
                mRenderThread.join();
            } catch (InterruptedException e) {
                // Ignore
            }
            mRenderThread = null;
        }
        nativeSetSurface(null);
    }

    @Override
    protected void onDetachedFromWindow() {
        super.onDetachedFromWindow();
        nativeDestroy();
    }

    private class RenderThread extends Thread {
        @Override
        public void run() {
            try {
                while (!isInterrupted()) {
                    nativeRender();
                }
            } finally {
                // Final cleanup if needed
            }
        }
    }

    private native void nativeInit();
    private native void nativeDestroy();
    private native void nativeSetSurface(Surface surface);
    private native void nativeRender();
}
