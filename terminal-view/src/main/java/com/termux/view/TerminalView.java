package com.termux.view;

import android.annotation.SuppressLint;
import android.annotation.TargetApi;
import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.RenderNode;
import android.graphics.Typeface;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.text.Editable;
import android.text.InputType;
import android.text.TextUtils;
import android.util.AttributeSet;
import android.view.ActionMode;
import android.view.Choreographer;
import android.view.HapticFeedbackConstants;
import android.view.InputDevice;
import android.view.KeyCharacterMap;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.ViewTreeObserver;
import android.view.accessibility.AccessibilityManager;
import android.view.autofill.AutofillManager;
import android.view.autofill.AutofillValue;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.widget.Scroller;

import androidx.annotation.Nullable;
import androidx.annotation.RequiresApi;

import com.termux.terminal.KeyHandler;
import com.termux.terminal.TerminalEmulator;
import com.termux.terminal.TerminalSession;
import com.termux.view.textselection.TextSelectionCursorController;

public final class TerminalView extends View {

    private static boolean TERMINAL_VIEW_KEY_LOGGING_ENABLED = false;

    public TerminalSession mTermSession;
    public TerminalEmulator mEmulator;
    public TerminalRenderer mRenderer;
    public TerminalViewClient mClient;

    private TextSelectionCursorController mTextSelectionCursorController;

    // --- Double Buffering & VSync ---
    private RenderNode mRenderNode;
    private boolean mBufferDirty = true;
    private boolean mIsFrameScheduled = false;
    private final Choreographer.FrameCallback mFrameCallback = new Choreographer.FrameCallback() {
        @Override
        public void doFrame(long frameTimeNanos) {
            mIsFrameScheduled = false;
            if (mBufferDirty) {
                recordRenderNode();
                invalidate();
            }
        }
    };

    private Handler mTerminalCursorBlinkerHandler;
    private TerminalCursorBlinkerRunnable mTerminalCursorBlinkerRunnable;
    private int mTerminalCursorBlinkerRate;
    
    int mTopRow;
    int[] mDefaultSelectors = new int[]{-1,-1,-1,-1};
    float mScaleFactor = 1.f;
    final GestureAndScaleRecognizer mGestureRecognizer;
    final Scroller mScroller;
    float mScrollRemainder;
    int mCombiningAccent;
    
    @RequiresApi(api = Build.VERSION_CODES.O)
    private int mAutoFillType = AUTOFILL_TYPE_NONE;
    @RequiresApi(api = Build.VERSION_CODES.O)
    private int mAutoFillImportance = IMPORTANT_FOR_AUTOFILL_NO;
    private String[] mAutoFillHints = new String[0];

    private final boolean mAccessibilityEnabled;
    private static final String LOG_TAG = "TerminalView";
    public final static int KEY_EVENT_SOURCE_SOFT_KEYBOARD = 0;

    public TerminalView(Context context, AttributeSet attributes) {
        super(context, attributes);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            mRenderNode = new RenderNode("TerminalRenderNode");
        }
        
        mGestureRecognizer = new GestureAndScaleRecognizer(context, new GestureAndScaleRecognizer.Listener() {
            boolean scrolledWithFinger;
            @Override public boolean onUp(MotionEvent event) { 
                mScrollRemainder = 0.0f;
                if (mEmulator != null && mEmulator.isMouseTrackingActive() && !event.isFromSource(InputDevice.SOURCE_MOUSE) && !isSelectingText() && !scrolledWithFinger) {
                    sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON, true);
                    sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON, false);
                    return true;
                }
                scrolledWithFinger = false;
                return false;
            }
            @Override public boolean onSingleTapUp(MotionEvent event) { 
                if (mEmulator == null) return true; 
                if (isSelectingText()) { stopTextSelectionMode(); return true; } 
                requestFocus(); 
                mClient.onSingleTapUp(event); 
                return true; 
            }
            @Override public boolean onScroll(MotionEvent e, float dx, float dy) { 
                if (mEmulator == null) return true;
                if (mEmulator.isMouseTrackingActive() && e.isFromSource(InputDevice.SOURCE_MOUSE)) sendMouseEventCode(e, TerminalEmulator.MOUSE_LEFT_BUTTON_MOVED, true);
                else {
                    scrolledWithFinger = true;
                    dy += mScrollRemainder;
                    int deltaRows = (int) (dy / mRenderer.mFontLineSpacing);
                    mScrollRemainder = dy - deltaRows * mRenderer.mFontLineSpacing;
                    doScroll(e, deltaRows);
                }
                return true;
            }
            @Override public boolean onScale(float x, float y, float s) { if (mEmulator == null || isSelectingText()) return true; mScaleFactor *= s; mScaleFactor = mClient.onScale(mScaleFactor); return true; }
            @Override public boolean onFling(final MotionEvent e2, float vx, float vy) { 
                if (mEmulator == null || !mScroller.isFinished()) return true;
                final boolean mt = mEmulator.isMouseTrackingActive();
                float S = 0.25f;
                if (mt) mScroller.fling(0, 0, 0, -(int) (vy * S), 0, 0, -mEmulator.mRows/2, mEmulator.mRows/2);
                else mScroller.fling(0, mTopRow, 0, -(int) (vy * S), 0, 0, -mEmulator.getScreen().getActiveTranscriptRows(), 0);
                post(new Runnable() {
                    private int lastY = 0;
                    @Override public void run() {
                        if (mt != mEmulator.isMouseTrackingActive() || mScroller.isFinished()) return;
                        boolean more = mScroller.computeScrollOffset();
                        int currY = mScroller.getCurrY();
                        doScroll(e2, mt ? (currY - lastY) : (currY - mTopRow));
                        lastY = currY;
                        if (more) post(this);
                    }
                });
                return true;
            }
            @Override public boolean onDown(float x, float y) { return false; }
            @Override public boolean onDoubleTap(MotionEvent e) { return false; }
            @Override public void onLongPress(MotionEvent e) { if (mGestureRecognizer.isInProgress() || mClient.onLongPress(e)) return; if (!isSelectingText()) { performHapticFeedback(HapticFeedbackConstants.LONG_PRESS); startTextSelectionMode(e); } }
        });
        mScroller = new Scroller(context);
        AccessibilityManager am = (AccessibilityManager) context.getSystemService(Context.ACCESSIBILITY_SERVICE);
        mAccessibilityEnabled = (am != null && am.isEnabled());
    }

    public void onScreenUpdated(boolean skipScrolling) {
        if (mEmulator == null) return;
        int hist = mEmulator.getScreen().getActiveTranscriptRows();
        if (mTopRow < -hist) mTopRow = -hist;
        if (isSelectingText() || mEmulator.isAutoScrollDisabled()) {
            int shift = mEmulator.getScrollCounter();
            if (-mTopRow + shift > hist) { if (isSelectingText()) stopTextSelectionMode(); if (mEmulator.isAutoScrollDisabled()) { mTopRow = -hist; skipScrolling = true; } }
            else { skipScrolling = true; mTopRow -= shift; decrementYTextSelectionCursors(shift); }
        }
        if (!skipScrolling && mTopRow != 0) { if (mTopRow < -3) awakenScrollBars(); mTopRow = 0; }
        mEmulator.clearScrollCounter();

        mBufferDirty = true;
        if (!mIsFrameScheduled) {
            mIsFrameScheduled = true;
            Choreographer.getInstance().postFrameCallback(mFrameCallback);
        }
        if (mAccessibilityEnabled) setContentDescription(getText());
    }

    private void recordRenderNode() {
        if (mEmulator == null || mRenderNode == null || Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return;
        Canvas canvas = mRenderNode.beginRecording();
        try {
            int[] sel = mDefaultSelectors;
            if (mTextSelectionCursorController != null) mTextSelectionCursorController.getSelectors(sel);
            canvas.drawColor(0XFF000000);
            mRenderer.render(mEmulator, canvas, mTopRow, sel[0], sel[1], sel[2], sel[3]);
        } finally {
            mRenderNode.endRecording();
        }
        mBufferDirty = false;
    }

    @Override
    protected void onDraw(Canvas canvas) {
        if (mEmulator == null) { canvas.drawColor(0XFF000000); return; }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && mRenderNode != null) {
            canvas.drawRenderNode(mRenderNode);
        } else {
            int[] sel = mDefaultSelectors;
            if (mTextSelectionCursorController != null) mTextSelectionCursorController.getSelectors(sel);
            mRenderer.render(mEmulator, canvas, mTopRow, sel[0], sel[1], sel[2], sel[3]);
        }
        renderTextSelection();
    }

    public TerminalSession getCurrentSession() { return mTermSession; }
    private CharSequence getText() { return mEmulator.getScreen().getSelectedText(0, mTopRow, mEmulator.mColumns, mTopRow + mEmulator.mRows); }
    public int getCursorX(float x) { return (int) (x / mRenderer.mFontWidth); }
    public int getCursorY(float y) { return (int) (((y - 40) / mRenderer.mFontLineSpacing) + mTopRow); }
    public int getPointX(int cx) { return Math.round(Math.min(cx, mEmulator.mColumns) * mRenderer.mFontWidth); }
    public int getPointY(int cy) { return Math.round((cy - mTopRow) * mRenderer.mFontLineSpacing); }
    public int getTopRow() { return mTopRow; }
    public void setTopRow(int mTopRow) { this.mTopRow = mTopRow; }

    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override public void autofill(AutofillValue value) { if (value.isText()) mTermSession.write(value.getTextValue().toString()); resetAutoFill(); }
    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override public int getAutofillType() { return mAutoFillType; }
    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override public String[] getAutofillHints() { return mAutoFillHints; }
    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override public AutofillValue getAutofillValue() { return AutofillValue.forText(""); }
    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override public int getImportantForAutofill() { return mAutoFillImportance; }
    @RequiresApi(api = Build.VERSION_CODES.O)
    private synchronized void resetAutoFill() { mAutoFillType = AUTOFILL_TYPE_NONE; mAutoFillImportance = IMPORTANT_FOR_AUTOFILL_NO; mAutoFillHints = new String[0]; }

    public AutofillManager getAutoFillManagerService() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return null;
        try { return (AutofillManager) getContext().getSystemService("autofill"); } catch (Exception e) { return null; }
    }

    public void updateSize() {
        int w = getWidth(); int h = getHeight();
        if (w == 0 || h == 0 || mTermSession == null) return;
        int cols = Math.max(4, (int) (w / mRenderer.mFontWidth));
        int rows = Math.max(4, (h - mRenderer.mFontLineSpacingAndAscent) / mRenderer.mFontLineSpacing);
        if (mEmulator == null || (cols != mEmulator.mColumns || rows != mEmulator.mRows)) {
            mTermSession.updateSize(cols, rows, (int) mRenderer.getFontWidth(), mRenderer.getFontLineSpacing());
            mEmulator = mTermSession.getEmulator();
            mClient.onEmulatorSet();
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && mRenderNode != null) {
                mRenderNode.setPosition(0, 0, w, h);
            }
            mTopRow = 0; scrollTo(0, 0); mBufferDirty = true; invalidate();
        }
    }

    public boolean attachSession(TerminalSession s) { mTermSession = s; mEmulator = null; updateSize(); return true; }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN;
        return new BaseInputConnection(this, true) {
            @Override public boolean commitText(CharSequence text, int newCursorPosition) {
                super.commitText(text, newCursorPosition);
                if (mEmulator == null) return true;
                Editable content = getEditable();
                sendTextToTerminal(content);
                content.clear();
                return true;
            }
            void sendTextToTerminal(CharSequence text) {
                stopTextSelectionMode();
                for (int i = 0; i < text.length(); i++) {
                    int cp = Character.toCodePoint(text.charAt(i), (i+1 < text.length()) ? text.charAt(i+1) : 0);
                    if (Character.isSupplementaryCodePoint(cp)) i++;
                    mTermSession.writeCodePoint(mClient.readAltKey(), cp);
                }
            }
        };
    }

    void doScroll(MotionEvent event, int rowsDown) {
        boolean up = rowsDown < 0; int amount = Math.abs(rowsDown);
        for (int i = 0; i < amount; i++) {
            if (mEmulator.isMouseTrackingActive()) sendMouseEventCode(event, up ? TerminalEmulator.MOUSE_WHEELUP_BUTTON : TerminalEmulator.MOUSE_WHEELDOWN_BUTTON, true);
            else { mTopRow = Math.min(0, Math.max(-(mEmulator.getScreen().getActiveTranscriptRows()), mTopRow + (up ? -1 : 1))); mBufferDirty = true; invalidate(); }
        }
    }

    void sendMouseEventCode(MotionEvent e, int b, boolean p) {
        int[] pos = getColumnAndRow(e, false);
        mEmulator.sendMouseEvent(b, pos[0] + 1, pos[1] + 1, p);
    }

    public int[] getColumnAndRow(MotionEvent event, boolean relativeToScroll) {
        int column = (int) (event.getX() / mRenderer.mFontWidth);
        int row = (int) ((event.getY() - mRenderer.mFontLineSpacingAndAscent) / mRenderer.mFontLineSpacing);
        if (relativeToScroll) row += mTopRow;
        return new int[] { column, row };
    }

    public void updateFloatingToolbarVisibility(MotionEvent event) {
        // Placeholder to satisfy dependencies, logic can be implemented if floating toolbar is restored
    }

    @Override protected void onSizeChanged(int w, int h, int ow, int oh) { updateSize(); }
    public void setTypeface(Typeface t) { mRenderer = new TerminalRenderer(mRenderer.mTextSize, t); updateSize(); invalidate(); }
    @Override public boolean onCheckIsTextEditor() { return true; }
    @Override public boolean isOpaque() { return true; }
    
    private void renderTextSelection() { if (mTextSelectionCursorController != null) mTextSelectionCursorController.render(); }
    public boolean isSelectingText() { return mTextSelectionCursorController != null && mTextSelectionCursorController.isActive(); }
    public void startTextSelectionMode(MotionEvent e) { if (requestFocus()) { mTextSelectionCursorController.show(e); mBufferDirty = true; invalidate(); } }
    public void stopTextSelectionMode() { if (mTextSelectionCursorController != null && mTextSelectionCursorController.hide()) { mBufferDirty = true; invalidate(); } }
    private void decrementYTextSelectionCursors(int d) { if (mTextSelectionCursorController != null) mTextSelectionCursorController.decrementYTextSelectionCursors(d); }

    private class TerminalCursorBlinkerRunnable implements Runnable {
        private TerminalEmulator mEmulator;
        public void setEmulator(TerminalEmulator emulator) { mEmulator = emulator; }
        @Override public void run() { /* Logic simplified for now */ }
    }

    @Override
    protected void onDetachedFromWindow() { 
        super.onDetachedFromWindow(); 
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && mRenderNode != null) mRenderNode.discardDisplayList(); 
        Choreographer.getInstance().removeFrameCallback(mFrameCallback); 
    }
}
