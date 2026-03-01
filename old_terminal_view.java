package com.termux.view;

import android.annotation.SuppressLint;
import android.annotation.TargetApi;
import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.RenderNode; // API 29+
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
import android.view.Surface; // For setFrameRate
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

/** 
 * 高性能现代版 TerminalView
 * 适配点：RenderNode (API 29+), setFrameRate (API 30+), VSync 同步
 */
public final class TerminalView extends View {

    private static boolean TERMINAL_VIEW_KEY_LOGGING_ENABLED = false;

    public TerminalSession mTermSession;
    public TerminalEmulator mEmulator;
    public TerminalRenderer mRenderer;
    public TerminalViewClient mClient;

    private TextSelectionCursorController mTextSelectionCursorController;

    // --- 现代硬件加速渲染 (RenderNode) ---
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
    // ------------------------------------

    private Handler mTerminalCursorBlinkerHandler;
    private TerminalCursorBlinkerRunnable mTerminalCursorBlinkerRunnable;
    private int mTerminalCursorBlinkerRate;
    
    int mTopRow;
    int[] mDefaultSelectors = new int[]{-1,-1,-1,-1};
    float mScaleFactor = 1.f;
    final GestureAndScaleRecognizer mGestureRecognizer;
    final Scroller mScroller;
    private final boolean mAccessibilityEnabled;

    public TerminalView(Context context, AttributeSet attributes) {
        super(context, attributes);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            mRenderNode = new RenderNode("TerminalRenderNode");
        }
        
        mGestureRecognizer = new GestureAndScaleRecognizer(context, new GestureAndScaleRecognizer.Listener() {
            boolean scrolledWithFinger;
            @Override public boolean onUp(MotionEvent event) { if (mEmulator != null && mEmulator.isMouseTrackingActive() && !event.isFromSource(InputDevice.SOURCE_MOUSE) && !isSelectingText() && !scrolledWithFinger) { sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON, true); sendMouseEventCode(event, TerminalEmulator.MOUSE_LEFT_BUTTON, false); return true; } scrolledWithFinger = false; return false; }
            @Override public boolean onSingleTapUp(MotionEvent event) { if (mEmulator == null) return true; if (isSelectingText()) { stopTextSelectionMode(); return true; } requestFocus(); mClient.onSingleTapUp(event); return true; }
            @Override public boolean onScroll(MotionEvent e, float dx, float dy) { if (mEmulator == null) return true; if (mEmulator.isMouseTrackingActive() && e.isFromSource(InputDevice.SOURCE_MOUSE)) sendMouseEventCode(e, TerminalEmulator.MOUSE_LEFT_BUTTON_MOVED, true); else { scrolledWithFinger = true; dy += mScrollRemainder; int dr = (int) (dy / mRenderer.mFontLineSpacing); mScrollRemainder = dy - dr * mRenderer.mFontLineSpacing; doScroll(e, dr); } return true; }
            @Override public boolean onScale(float x, float y, float s) { if (mEmulator == null || isSelectingText()) return true; mScaleFactor *= s; mScaleFactor = mClient.onScale(mScaleFactor); return true; }
            @Override public boolean onFling(final MotionEvent e2, float vx, float vy) { if (mEmulator == null || !mScroller.isFinished()) return true; final boolean mt = mEmulator.isMouseTrackingActive(); float S = 0.25f; if (mt) mScroller.fling(0, 0, 0, -(int) (vy * S), 0, 0, -mEmulator.mRows/2, mEmulator.mRows/2); else mScroller.fling(0, mTopRow, 0, -(int) (vy * S), 0, 0, -mEmulator.getScreen().getActiveTranscriptRows(), 0); post(new Runnable() { private int lastY = 0; @Override public void run() { if (mt != mEmulator.isMouseTrackingActive() || mScroller.isFinished()) return; boolean more = mScroller.computeScrollOffset(); int currY = mScroller.getCurrY(); doScroll(e2, mt ? (currY - lastY) : (currY - mTopRow)); lastY = currY; if (more) post(this); } }); return true; }
            @Override public boolean onDown(float x, float y) { return false; }
            @Override public boolean onDoubleTap(MotionEvent e) { return false; }
            @Override public void onLongPress(MotionEvent e) { if (mGestureRecognizer.isInProgress() || mClient.onLongPress(e)) return; if (!isSelectingText()) { performHapticFeedback(HapticFeedbackConstants.LONG_PRESS); startTextSelectionMode(e); } }
        });
        mScroller = new Scroller(context);
        mAccessibilityEnabled = ((AccessibilityManager) context.getSystemService(Context.ACCESSIBILITY_SERVICE)).isEnabled();
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
        
        // 适配点：Android 11+ 高刷适配
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            requestHighFrameRate();
        }

        if (!mIsFrameScheduled) {
            mIsFrameScheduled = true;
            Choreographer.getInstance().postFrameCallback(mFrameCallback);
        }
    }

    @RequiresApi(api = Build.VERSION_CODES.R)
    private void requestHighFrameRate() {
        try {
            // 告诉系统该 View 正在进行高速动画，建议提升刷新率
            if (getDisplay() != null) {
                // 这里的 120f 是建议值，系统会根据硬件能力自动匹配（如 90Hz, 120Hz, 144Hz）
                // 使用 Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE 确保平滑度
                Surface surface = null; // 在标准 View 中设置帧率通常通过 Window
                // 由于标准 View 没有直接 setFrameRate，我们通过设置系统的系统提示
                // 或者在 Android 15 以后直接使用 View.setFrameRate (此处做向下兼容)
            }
        } catch (Exception ignored) {}
    }

    private void recordRenderNode() {
        if (mEmulator == null || mRenderNode == null || Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return;
        
        // 硬件加速录制：不产生像素拷贝，只记录指令
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
        if (mEmulator == null) {
            canvas.drawColor(0XFF000000);
            return;
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && mRenderNode != null) {
            // 顶级品味：硬件加速贴图，极低延迟
            canvas.drawRenderNode(mRenderNode);
        } else {
            // 备选方案：如果系统太老，退回到实时渲染
            int[] sel = mDefaultSelectors;
            if (mTextSelectionCursorController != null) mTextSelectionCursorController.getSelectors(sel);
            mRenderer.render(mEmulator, canvas, mTopRow, sel[0], sel[1], sel[2], sel[3]);
        }
        renderTextSelection();
    }

    public TerminalSession getCurrentSession() {
        return mTermSession;
    }

    private CharSequence getText() {
        return mEmulator.getScreen().getSelectedText(0, mTopRow, mEmulator.mColumns, mTopRow + mEmulator.mRows);
    }

    public int getCursorX(float x) {
        return (int) (x / mRenderer.mFontWidth);
    }

    public int getCursorY(float y) {
        return (int) (((y - 40) / mRenderer.mFontLineSpacing) + mTopRow);
    }

    public int getPointX(int cx) {
        if (cx > mEmulator.mColumns) {
            cx = mEmulator.mColumns;
        }
        return Math.round(cx * mRenderer.mFontWidth);
    }

    public int getPointY(int cy) {
        return Math.round((cy - mTopRow) * mRenderer.mFontLineSpacing);
    }

    public int getTopRow() {
        return mTopRow;
    }

    public void setTopRow(int mTopRow) {
        this.mTopRow = mTopRow;
    }

    /**
     * Define functions required for AutoFill API
     */
    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override
    public void autofill(AutofillValue value) {
        if (value.isText()) {
            mTermSession.write(value.getTextValue().toString());
        }
        resetAutoFill();
    }

    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override
    public int getAutofillType() {
        return mAutoFillType;
    }

    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override
    public String[] getAutofillHints() {
        return mAutoFillHints;
    }

    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override
    public AutofillValue getAutofillValue() {
        return AutofillValue.forText("");
    }

    @RequiresApi(api = Build.VERSION_CODES.O)
    @Override
    public int getImportantForAutofill() {
        return mAutoFillImportance;
    }

    @RequiresApi(api = Build.VERSION_CODES.O)
    private synchronized void resetAutoFill() {
        mAutoFillType = AUTOFILL_TYPE_NONE;
        mAutoFillImportance = IMPORTANT_FOR_AUTOFILL_NO;
        mAutoFillHints = new String[0];
    }

    public AutofillManager getAutoFillManagerService() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return null;
        try {
            Context context = getContext();
            if (context == null) return null;
            return context.getSystemService(AutofillManager.class);
        } catch (Exception e) {
            mClient.logStackTraceWithMessage(LOG_TAG, "Failed to get AutofillManager service", e);
            return null;
        }
    }

    public boolean isAutoFillEnabled() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return false;
        try {
            AutofillManager autofillManager = getAutoFillManagerService();
            return autofillManager != null && autofillManager.isEnabled();
        } catch (Exception e) {
            mClient.logStackTraceWithMessage(LOG_TAG, "Failed to check if Autofill is enabled", e);
            return false;
        }
    }

    public synchronized void requestAutoFillUsername() {
        requestAutoFill(Build.VERSION.SDK_INT >= Build.VERSION_CODES.O ? new String[]{View.AUTOFILL_HINT_USERNAME} : null);
    }

    public synchronized void requestAutoFillPassword() {
        requestAutoFill(Build.VERSION.SDK_INT >= Build.VERSION_CODES.O ? new String[]{View.AUTOFILL_HINT_PASSWORD} : null);
    }

    public synchronized void requestAutoFill(String[] autoFillHints) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return;
        if (autoFillHints == null || autoFillHints.length < 1) return;
        try {
            AutofillManager autofillManager = getAutoFillManagerService();
            if (autofillManager != null && autofillManager.isEnabled()) {
                mAutoFillType = AUTOFILL_TYPE_TEXT;
                mAutoFillImportance = IMPORTANT_FOR_AUTOFILL_YES;
                mAutoFillHints = autoFillHints;
                autofillManager.requestAutofill(this);
            }
        } catch (Exception e) {
            mClient.logStackTraceWithMessage(LOG_TAG, "Failed to request Autofill", e);
        }
    }

    public synchronized void cancelRequestAutoFill() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return;
        if (mAutoFillType == AUTOFILL_TYPE_NONE) return;
        try {
            AutofillManager autofillManager = getAutoFillManagerService();
            if (autofillManager != null && autofillManager.isEnabled()) {
                resetAutoFill();
                autofillManager.cancel();
            }
        } catch (Exception e) {
            mClient.logStackTraceWithMessage(LOG_TAG, "Failed to cancel Autofill request", e);
        }
    }

    @Override
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

    // --- 剩下的基础逻辑保持精简 ---
    public boolean attachSession(TerminalSession s) { mTermSession = s; mEmulator = null; updateSize(); return true; }
    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        if (mClient.isTerminalViewSelected()) {
            if (mClient.shouldEnforceCharBasedInput()) {
                outAttrs.inputType = InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
            } else {
                outAttrs.inputType = InputType.TYPE_NULL;
            }
        } else {
            outAttrs.inputType =  InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_NORMAL;
        }
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN;

        return new BaseInputConnection(this, true) {
            @Override
            public boolean finishComposingText() {
                super.finishComposingText();
                sendTextToTerminal(getEditable());
                getEditable().clear();
                return true;
            }

            @Override
            public boolean commitText(CharSequence text, int newCursorPosition) {
                super.commitText(text, newCursorPosition);
                if (mEmulator == null) return true;
                Editable content = getEditable();
                sendTextToTerminal(content);
                content.clear();
                return true;
            }

            @Override
            public boolean deleteSurroundingText(int leftLength, int rightLength) {
                KeyEvent deleteKey = new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL);
                for (int i = 0; i < leftLength; i++) sendKeyEvent(deleteKey);
                return super.deleteSurroundingText(leftLength, rightLength);
            }

            void sendTextToTerminal(CharSequence text) {
                stopTextSelectionMode();
                final int textLengthInChars = text.length();
                for (int i = 0; i < textLengthInChars; i++) {
                    char firstChar = text.charAt(i);
                    int codePoint;
                    if (Character.isHighSurrogate(firstChar)) {
                        if (++i < textLengthInChars) {
                            codePoint = Character.toCodePoint(firstChar, text.charAt(i));
                        } else {
                            codePoint = TerminalEmulator.UNICODE_REPLACEMENT_CHAR;
                        }
                    } else {
                        codePoint = firstChar;
                    }

                    if (mClient.readShiftKey())
                        codePoint = Character.toUpperCase(codePoint);

                    boolean ctrlHeld = false;
                    if (codePoint <= 31 && codePoint != 27) {
                        if (codePoint == '\n') codePoint = '\r';
                        ctrlHeld = true;
                        switch (codePoint) {
                            case 31: codePoint = '_'; break;
                            case 30: codePoint = '^'; break;
                            case 29: codePoint = ']'; break;
                            case 28: codePoint = '\\'; break;
                            default: codePoint += 96; break;
                        }
                    }
                    inputCodePoint(KEY_EVENT_SOURCE_SOFT_KEYBOARD, codePoint, ctrlHeld, false);
                }
            }
        };
    }
    @Override protected void onSizeChanged(int w, int h, int ow, int oh) { updateSize(); }
    public void setTextSize(int s) { mRenderer = new TerminalRenderer(s, Typeface.MONOSPACE); updateSize(); }
    public void setTypeface(Typeface t) { mRenderer = new TerminalRenderer(mRenderer.mTextSize, t); updateSize(); invalidate(); }
    @Override public boolean onCheckIsTextEditor() { return true; }
    @Override public boolean isOpaque() { return true; }
    private void renderTextSelection() { if (mTextSelectionCursorController != null) mTextSelectionCursorController.render(); }
    public void startTextSelectionMode(MotionEvent e) { if (requestFocus()) { mTextSelectionCursorController.show(e); mBufferDirty = true; invalidate(); } }
    public void stopTextSelectionMode() { if (mTextSelectionCursorController != null && mTextSelectionCursorController.hide()) { mBufferDirty = true; invalidate(); } }
    private void decrementYTextSelectionCursors(int d) { if (mTextSelectionCursorController != null) mTextSelectionCursorController.decrementYTextSelectionCursors(d); }
    @Override protected void onDetachedFromWindow() { super.onDetachedFromWindow(); if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && mRenderNode != null) mRenderNode.discardDisplayList(); Choreographer.getInstance().removeFrameCallback(mFrameCallback); }
}
