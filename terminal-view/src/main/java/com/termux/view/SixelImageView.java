package com.termux.view;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.util.AttributeSet;
import android.util.Log;
import android.view.View;
import android.view.ViewParent;

/**
 * View for rendering Sixel images in the terminal.
 * Sixel is a graphics format for terminal emulators that supports 256 colors.
 * 
 * Features:
 * - Automatic scaling to fit character cell grid
 * - Font size adaptive scaling
 * - High-quality bitmap filtering
 * - Support for terminal resize events
 */
public class SixelImageView extends View {
    
    private static final String TAG = "SixelImageView";
    
    private Bitmap mBitmap;
    private Bitmap mScaledBitmap;
    private Paint mPaint;
    private Matrix mScaleMatrix;
    
    // Original image dimensions
    private int mOriginalWidth;
    private int mOriginalHeight;
    
    // Character cell position
    private int mStartX;
    private int mStartY;
    private int mEndX;
    private int mEndY;
    
    // Font metrics for scaling
    private float mFontWidth;
    private float mFontLineSpacing;
    private float mFontAscent;
    
    // Target dimensions in pixels
    private int mTargetPixelWidth;
    private int mTargetPixelHeight;
    
    private boolean mVisible;
    private boolean mNeedsRescale;

    public SixelImageView(Context context) {
        super(context);
        init();
    }
    
    public SixelImageView(Context context, AttributeSet attrs) {
        super(context, attrs);
        init();
    }
    
    public SixelImageView(Context context, AttributeSet attrs, int defStyleAttr) {
        super(context, attrs, defStyleAttr);
        init();
    }
    
    private void init() {
        mPaint = new Paint(Paint.FILTER_BITMAP_FLAG);
        mPaint.setAntiAlias(true);
        mPaint.setDither(true);
        mScaleMatrix = new Matrix();
        mVisible = false;
        mNeedsRescale = false;
        
        // Enable hardware acceleration for better performance
        setLayerType(LAYER_TYPE_HARDWARE, null);
    }
    
    /**
     * Set the Sixel image data with automatic scaling to fit character cells
     * @param rgbaData RGBA format pixel data
     * @param width Image width in sixel units
     * @param height Image height in sixel units
     * @param startX Start X position in character cells
     * @param startY Start Y position in character cells
     * @param fontWidth Font width in pixels
     * @param fontLineSpacing Font line spacing in pixels
     * @param fontAscent Font ascent in pixels
     */
    public void setImageData(byte[] rgbaData, int width, int height, 
                            int startX, int startY,
                            float fontWidth, float fontLineSpacing, float fontAscent) {
        if (rgbaData == null || rgbaData.length == 0) {
            clear();
            return;
        }

        // Convert RGBA byte array to Bitmap
        int pixelCount = rgbaData.length / 4;
        if (pixelCount != width * height) {
            Log.e(TAG, "Invalid RGBA data size: " + rgbaData.length + 
                  ", expected: " + (width * height * 4));
            return;
        }

        int[] pixels = new int[pixelCount];
        for (int i = 0; i < pixelCount; i++) {
            int r = rgbaData[i * 4] & 0xFF;
            int g = rgbaData[i * 4 + 1] & 0xFF;
            int b = rgbaData[i * 4 + 2] & 0xFF;
            int a = rgbaData[i * 4 + 3] & 0xFF;
            pixels[i] = (a << 24) | (r << 16) | (g << 8) | b;
        }

        mBitmap = Bitmap.createBitmap(pixels, width, height, Bitmap.Config.ARGB_8888);
        mOriginalWidth = width;
        mOriginalHeight = height;
        mStartX = startX;
        mStartY = startY;
        
        // Set font metrics
        mFontWidth = fontWidth;
        mFontLineSpacing = fontLineSpacing;
        mFontAscent = fontAscent;
        
        // Calculate target size based on character cells
        calculateTargetSize();
        
        // Create scaled bitmap
        createScaledBitmap();
        
        mVisible = true;
        mNeedsRescale = false;
        
        requestLayout();
        invalidate();
        
        Log.d(TAG, String.format("Image set: %dx%d -> %dx%d pixels at (%d,%d)",
                mOriginalWidth, mOriginalHeight,
                mTargetPixelWidth, mTargetPixelHeight,
                mStartX, mStartY));
    }
    
    /**
     * Set image data with default font metrics (will be updated later)
     */
    public void setImageData(byte[] rgbaData, int width, int height, 
                            int startX, int startY) {
        // Use default font metrics (will be scaled when real metrics are set)
        setImageData(rgbaData, width, height, startX, startY, 10f, 20f, 5f);
    }
    
    /**
     * Update font metrics and rescale image if needed
     * @param fontWidth New font width in pixels
     * @param fontLineSpacing New font line spacing in pixels
     * @param fontAscent New font ascent in pixels
     * @return true if image was rescaled
     */
    public boolean updateFontMetrics(float fontWidth, float fontLineSpacing, float fontAscent) {
        boolean changed = (Math.abs(mFontWidth - fontWidth) > 0.5f ||
                          Math.abs(mFontLineSpacing - fontLineSpacing) > 0.5f ||
                          Math.abs(mFontAscent - fontAscent) > 0.5f);
        
        mFontWidth = fontWidth;
        mFontLineSpacing = fontLineSpacing;
        mFontAscent = fontAscent;
        
        if (changed && mBitmap != null) {
            calculateTargetSize();
            createScaledBitmap();
            requestLayout();
            invalidate();
            Log.d(TAG, String.format("Font metrics updated, rescaled to %dx%d",
                    mTargetPixelWidth, mTargetPixelHeight));
            return true;
        }
        
        return false;
    }
    
    /**
     * Calculate target size in pixels based on character cell grid
     */
    private void calculateTargetSize() {
        // Calculate how many character cells the image should occupy
        // Sixel images are typically designed to fit terminal character grids
        // Default: 1 sixel unit ≈ 1 pixel, scale to fit character cells

        // Estimate character cell span from original sixel dimensions
        // Typical sixel aspect ratio: 6 vertical pixels per character row
        int charWidth = Math.max(1, (int) Math.ceil(mOriginalWidth / 6.0f));
        int charHeight = Math.max(1, (int) Math.ceil(mOriginalHeight / 6.0f));

        // Boundary check: Limit to terminal bounds (default 80x24)
        // This prevents images from exceeding the terminal window
        int maxCols = 80;
        int maxRows = 24;
        
        // Try to get actual terminal size from parent if available
        ViewParent parent = getParent();
        if (parent != null && parent instanceof View) {
            // Get terminal dimensions from the view hierarchy
            View rootView = (View) parent;
            int terminalWidth = rootView.getWidth();
            int terminalHeight = rootView.getHeight();
            
            if (terminalWidth > 0 && terminalHeight > 0 && mFontWidth > 0 && mFontLineSpacing > 0) {
                maxCols = (int) (terminalWidth / mFontWidth);
                maxRows = (int) (terminalHeight / mFontLineSpacing);
            }
        }
        
        // Ensure image doesn't exceed terminal bounds
        if (mStartX + charWidth > maxCols) {
            charWidth = Math.max(1, maxCols - mStartX);
            Log.d(TAG, String.format("Image width cropped to %d chars (terminal width: %d, start: %d)",
                    charWidth, maxCols, mStartX));
        }
        if (mStartY + charHeight > maxRows) {
            charHeight = Math.max(1, maxRows - mStartY);
            Log.d(TAG, String.format("Image height cropped to %d chars (terminal height: %d, start: %d)",
                    charHeight, maxRows, mStartY));
        }
        
        // Final safety limit
        charWidth = Math.min(charWidth, maxCols);
        charHeight = Math.min(charHeight, maxRows);

        // Calculate pixel dimensions
        mTargetPixelWidth = (int) (charWidth * mFontWidth);
        mTargetPixelHeight = (int) (charHeight * mFontLineSpacing);

        // Ensure minimum size (don't scale down below original)
        mTargetPixelWidth = Math.max(mTargetPixelWidth, mOriginalWidth);
        mTargetPixelHeight = Math.max(mTargetPixelHeight, mOriginalHeight);

        mEndX = mStartX + charWidth;
        mEndY = mStartY + charHeight;
        
        Log.d(TAG, String.format("Target size calculated: %dx%d chars -> %dx%d pixels",
                charWidth, charHeight, mTargetPixelWidth, mTargetPixelHeight));
    }
    
    /**
     * Create scaled bitmap for display
     */
    private void createScaledBitmap() {
        if (mBitmap == null || mTargetPixelWidth <= 0 || mTargetPixelHeight <= 0) {
            return;
        }
        
        // Recycle old scaled bitmap
        if (mScaledBitmap != null && !mScaledBitmap.isRecycled()) {
            mScaledBitmap.recycle();
        }
        
        // Create scaled bitmap with high quality filtering
        mScaledBitmap = Bitmap.createScaledBitmap(
            mBitmap,
            mTargetPixelWidth,
            mTargetPixelHeight,
            true  // Use filtering for better quality
        );
        
        Log.d(TAG, String.format("Scaled bitmap created: %dx%d -> %dx%d",
                mOriginalWidth, mOriginalHeight,
                mTargetPixelWidth, mTargetPixelHeight));
    }
    
    /**
     * Clear the current image
     */
    public void clear() {
        if (mBitmap != null) {
            mBitmap.recycle();
            mBitmap = null;
        }
        if (mScaledBitmap != null && !mScaledBitmap.isRecycled()) {
            mScaledBitmap.recycle();
            mScaledBitmap = null;
        }
        mVisible = false;
        mNeedsRescale = false;
        invalidate();
    }
    
    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);

        if (mVisible && mScaledBitmap != null && !mScaledBitmap.isRecycled()) {
            // Draw the scaled bitmap at origin (position is handled by view layout)
            canvas.drawBitmap(mScaledBitmap, 0, 0, mPaint);
        }
    }

    @Override
    protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        if (mScaledBitmap != null && !mScaledBitmap.isRecycled()) {
            setMeasuredDimension(mTargetPixelWidth, mTargetPixelHeight);
        } else if (mBitmap != null) {
            setMeasuredDimension(mOriginalWidth, mOriginalHeight);
        } else {
            setMeasuredDimension(0, 0);
        }
    }
    
    /**
     * Update image position based on character cell coordinates
     * @param pixelX X position in pixels
     * @param pixelY Y position in pixels
     */
    public void updatePosition(int pixelX, int pixelY) {
        setX(pixelX);
        setY(pixelY);
    }
    
    /**
     * Get the character cell span
     * @return int[] {startX, startY, endX, endY}
     */
    public int[] getCharacterSpan() {
        return new int[]{mStartX, mStartY, mEndX, mEndY};
    }
    
    public boolean hasImage() {
        return mVisible && mScaledBitmap != null && !mScaledBitmap.isRecycled();
    }

    public int getImageWidth() {
        return mScaledBitmap != null && !mScaledBitmap.isRecycled() 
               ? mScaledBitmap.getWidth() 
               : (mBitmap != null ? mBitmap.getWidth() : 0);
    }

    public int getImageHeight() {
        return mScaledBitmap != null && !mScaledBitmap.isRecycled() 
               ? mScaledBitmap.getHeight() 
               : (mBitmap != null ? mBitmap.getHeight() : 0);
    }
    
    public int getOriginalWidth() {
        return mOriginalWidth;
    }
    
    public int getOriginalHeight() {
        return mOriginalHeight;
    }
    
    /**
     * Get the scale factor applied to the image
     * @return float[] {scaleX, scaleY}
     */
    public float[] getScaleFactors() {
        float scaleX = mTargetPixelWidth > 0 ? (float) mTargetPixelWidth / mOriginalWidth : 1.0f;
        float scaleY = mTargetPixelHeight > 0 ? (float) mTargetPixelHeight / mOriginalHeight : 1.0f;
        return new float[]{scaleX, scaleY};
    }
    
    @Override
    protected void onDetachedFromWindow() {
        super.onDetachedFromWindow();
        // Clean up bitmaps to free memory
        if (mBitmap != null) {
            mBitmap.recycle();
            mBitmap = null;
        }
        if (mScaledBitmap != null && !mScaledBitmap.isRecycled()) {
            mScaledBitmap.recycle();
            mScaledBitmap = null;
        }
    }
}
