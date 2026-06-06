package com.termux.app;

import android.annotation.SuppressLint;
import android.app.AlertDialog;
import android.content.Context;
import android.media.AudioManager;
import android.util.Log;
import android.view.Gravity;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.inputmethod.InputMethodManager;
import android.widget.ListView;

import com.termux.R;
import com.termux.app.extrakeys.SpecialButton;
import com.termux.terminal.TerminalEmulator;
import com.termux.terminal.TerminalSession;
import com.termux.view.TerminalViewClient;

import java.util.Arrays;
import java.util.Collections;
import java.util.LinkedHashSet;

import androidx.annotation.NonNull;
import androidx.drawerlayout.widget.DrawerLayout;

public final class TermuxTerminalViewClient implements TerminalViewClient {

    @NonNull final TermuxActivity mActivity;

    final TermuxTerminalSessionActivityClient mTermuxTerminalSessionActivityClient;

    /** Keeping track of the special keys acting as Ctrl and Fn for the soft keyboard and other hardware keys. */
    boolean mVirtualControlKeyDown, mVirtualFnKeyDown;

    private static final String LOG_TAG = "TermuxTerminalViewClient";

    public TermuxTerminalViewClient(@NonNull TermuxActivity activity, TermuxTerminalSessionActivityClient termuxTerminalSessionActivityClient) {
        this.mActivity = activity;
        this.mTermuxTerminalSessionActivityClient = termuxTerminalSessionActivityClient;
    }

    public TermuxActivity getActivity() {
        return mActivity;
    }

    /**
     * Should be called when mActivity.onStart() is called
     */
    public void onStart() {
        // Piggyback on the terminal view key logging toggle for now, should add a separate toggle in future
    }

    @Override
    public float onScale(float scale) {
        // Keep pinch-to-zoom responsive by letting TerminalView/Rust apply only visual scale
        // while the fingers are moving. Committing font size during each scale event changes
        // cols/rows and forces full scrollback reflow on long terminal histories.
        return scale;
    }

    @Override
    public float onScaleEnd(float scale) {
        if (scale < 0.9f || scale > 1.1f) {
            int currentFontSize = mActivity.mPreferences.getFontSize();
            int newFontSize = Math.round((currentFontSize * scale) / 2.0f) * 2;
            newFontSize = mActivity.mPreferences.setFontSize(newFontSize);
            if (newFontSize != currentFontSize) {
                Log.i(LOG_TAG, "Pinch zoom commit: scale=" + scale + ", fontSize=" + currentFontSize + " -> " + newFontSize);
                mActivity.getTerminalView().setTextSize(newFontSize);
                return 1.0f;
            }
        }
        return scale;
    }

    @Override
    public void onSingleTapUp(MotionEvent e) {
        TerminalSession session = mActivity.getCurrentSession();
        if (session != null) {
            TerminalEmulator term = session.getEmulator();
            if (term != null) {
                if (!term.isMouseTrackingActive() && !e.isFromSource(InputDevice.SOURCE_MOUSE)) {
                    mActivity.getSystemService(InputMethodManager.class).showSoftInput(mActivity.getTerminalView(), 0);
                }
            } else {
                // Fallback: always show keyboard on tap if emulator is not yet initialized
                if (!e.isFromSource(InputDevice.SOURCE_MOUSE)) {
                    mActivity.getSystemService(InputMethodManager.class).showSoftInput(mActivity.getTerminalView(), 0);
                }
            }
        }
    }

    @Override
    public boolean shouldBackButtonBeMappedToEscape() {
        return mActivity.mProperties.isBackKeyTheEscapeKey();
    }

    @Override
    public boolean shouldEnforceCharBasedInput() {
        return mActivity.mProperties.isEnforcingCharBasedInput();
    }

    @Override
    public boolean isTerminalViewSelected() {
        return mActivity.getTerminalToolbarViewPager() == null || mActivity.isTerminalViewSelected() || mActivity.getTerminalView().hasFocus();
    }

    @Override
    public void copyModeChanged(boolean copyMode) {
        // Disable drawer while copying.
        mActivity.getDrawer().setDrawerLockMode(copyMode ? DrawerLayout.LOCK_MODE_LOCKED_CLOSED : DrawerLayout.LOCK_MODE_UNLOCKED);
    }

    @SuppressLint("RtlHardcoded")
    @Override
    public boolean onKeyDown(int keyCode, KeyEvent e, TerminalSession currentSession) {
        if (handleVirtualKeys(keyCode, e, true)) {
            return true;
        }

        if (keyCode == KeyEvent.KEYCODE_ENTER && !currentSession.isRunning()) {
            mTermuxTerminalSessionActivityClient.removeFinishedSession(currentSession);
            return true;
        } else if (!mActivity.mProperties.areHardwareKeyboardShortcutsDisabled() && e.isCtrlPressed() && e.isAltPressed()) {
            // Get the unmodified code point:
            int unicodeChar = e.getUnicodeChar(0);

            if (keyCode == KeyEvent.KEYCODE_DPAD_DOWN || unicodeChar == 'n'/* next */) {
                mTermuxTerminalSessionActivityClient.switchToSession(true);
            } else if (keyCode == KeyEvent.KEYCODE_DPAD_UP || unicodeChar == 'p' /* previous */) {
                mTermuxTerminalSessionActivityClient.switchToSession(false);
            } else if (keyCode == KeyEvent.KEYCODE_DPAD_RIGHT) {
                mActivity.getDrawer().openDrawer(Gravity.LEFT);
            } else if (keyCode == KeyEvent.KEYCODE_DPAD_LEFT) {
                mActivity.getDrawer().closeDrawers();
            } else if (unicodeChar == 'c'/* create */) {
                mTermuxTerminalSessionActivityClient.addNewSession(false, null, null, null);
            } else if (unicodeChar == 'k'/* keyboard */) {
                onToggleSoftKeyboardRequest();
            } else if (unicodeChar == 'm'/* menu */) {
                mActivity.getTerminalView().showContextMenu();
            } else if (unicodeChar == 'r'/* rename */) {
                mTermuxTerminalSessionActivityClient.renameSession(currentSession);
            } else if (unicodeChar == 'u' /* urls */) {
                showUrlSelection();
            } else if (unicodeChar == 'v') {
                doPaste();
            } else if (unicodeChar == 'z' /* Zecret */) {
                mActivity.requestAutoFill();
            } else if (unicodeChar == '+' || e.getUnicodeChar(KeyEvent.META_SHIFT_ON) == '+') {
                // We also check for the shifted char here since shift may be required to produce '+',
                // see https://github.com/termux/termux-api/issues/2
                changeFontSize(true);
            } else if (unicodeChar == '-') {
                changeFontSize(false);
            } else if (unicodeChar == 'f') {
                boolean doFullscreen = mActivity.mPreferences.toggleFullscreen();
                mActivity.applyFullscreenSetting(doFullscreen);
            } else if (unicodeChar == 't') {
                mActivity.toggleTerminalToolbar();
            } else if (unicodeChar >= '1' && unicodeChar <= '9') {
                int index = unicodeChar - '1';
                mTermuxTerminalSessionActivityClient.switchToSession(index);
            }
            return true;
        }

        return false;

    }

    @Override
    public boolean onKeyUp(int keyCode, KeyEvent e) {
        // If emulator is not set, like if bootstrap installation failed and user dismissed the error
        // dialog, then just exit the activity, otherwise they will be stuck in a broken state.
        if (keyCode == KeyEvent.KEYCODE_BACK && mActivity.getTerminalView().mEmulator == null) {
            mActivity.finishActivityIfNotFinishing();
            return true;
        }

        return handleVirtualKeys(keyCode, e, false);
    }

    /** Handle dedicated volume buttons as virtual keys if applicable. */
    private boolean handleVirtualKeys(int keyCode, KeyEvent event, boolean down) {
        /*
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            if (event.getAction() == KeyEvent.ACTION_DOWN) {
                mActivity.onBackPressed();
            }
            return true;
        }
         */

        InputDevice inputDevice = event.getDevice();
        if (mActivity.mProperties.areVirtualVolumeKeysDisabled()) {
            return false;
        } else if (inputDevice != null && inputDevice.getKeyboardType() == InputDevice.KEYBOARD_TYPE_ALPHABETIC) {
            // Do not steal dedicated buttons from a full external keyboard.
            return false;
        } else if (keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) {
            mVirtualControlKeyDown = down;
            return true;
        } else if (keyCode == KeyEvent.KEYCODE_VOLUME_UP) {
            mVirtualFnKeyDown = down;
            return true;
        }
        return false;
    }



    @Override
    public boolean readControlKey() {
        return readExtraKeysSpecialButton(SpecialButton.CTRL) || mVirtualControlKeyDown;
    }

    @Override
    public boolean readAltKey() {
        return readExtraKeysSpecialButton(SpecialButton.ALT);
    }

    @Override
    public boolean readShiftKey() {
        return readExtraKeysSpecialButton(SpecialButton.SHIFT);
    }

    @Override
    public boolean readFnKey() {
        return readExtraKeysSpecialButton(SpecialButton.FN);
    }

    public boolean readExtraKeysSpecialButton(SpecialButton specialButton) {
        if (mActivity.getExtraKeysView() == null) return false;
        Boolean state = mActivity.getExtraKeysView().readSpecialButton(specialButton, true);
        if (state == null) {
            Log.e(LOG_TAG,"Failed to read an unregistered " + specialButton + " special button value from extra keys.");
            return false;
        }
        return state;
    }

    @Override
    public boolean onLongPress(MotionEvent event) {
        return false;
    }

    @Override
    public boolean onCodePoint(final int codePoint, boolean ctrlDown, TerminalSession session) {
        if (mVirtualFnKeyDown || readFnKey()) {
            int resultingKeyCode = -1;
            int resultingCodePoint = -1;
            boolean altDown = false;
            int lowerCase = Character.toLowerCase(codePoint);
            switch (lowerCase) {
                // Arrow keys.
                case 'w':
                    resultingKeyCode = KeyEvent.KEYCODE_DPAD_UP;
                    break;
                case 'a':
                    resultingKeyCode = KeyEvent.KEYCODE_DPAD_LEFT;
                    break;
                case 's':
                    resultingKeyCode = KeyEvent.KEYCODE_DPAD_DOWN;
                    break;
                case 'd':
                    resultingKeyCode = KeyEvent.KEYCODE_DPAD_RIGHT;
                    break;

                // Page up and down.
                case 'p':
                    resultingKeyCode = KeyEvent.KEYCODE_PAGE_UP;
                    break;
                case 'n':
                    resultingKeyCode = KeyEvent.KEYCODE_PAGE_DOWN;
                    break;

                // Some special keys:
                case 't':
                    resultingKeyCode = KeyEvent.KEYCODE_TAB;
                    break;
                case 'i':
                    resultingKeyCode = KeyEvent.KEYCODE_INSERT;
                    break;
                case 'h':
                    resultingCodePoint = '~';
                    break;

                // Special characters to input.
                case 'u':
                    resultingCodePoint = '_';
                    break;
                case 'l':
                    resultingCodePoint = '|';
                    break;

                // Function keys.
                case '1':
                case '2':
                case '3':
                case '4':
                case '5':
                case '6':
                case '7':
                case '8':
                case '9':
                    resultingKeyCode = (codePoint - '1') + KeyEvent.KEYCODE_F1;
                    break;
                case '0':
                    resultingKeyCode = KeyEvent.KEYCODE_F10;
                    break;

                // Other special keys.
                case 'e':
                    resultingCodePoint = /*Escape*/ 27;
                    break;
                case '.':
                    resultingCodePoint = /*^.*/ 28;
                    break;

                case 'b': // alt+b, jumping backward in readline.
                case 'f': // alf+f, jumping forward in readline.
                case 'x': // alt+x, common in emacs.
                    resultingCodePoint = lowerCase;
                    altDown = true;
                    break;

                // Volume control.
                case 'v':
                    resultingCodePoint = -1;
                    AudioManager audio = (AudioManager) mActivity.getSystemService(Context.AUDIO_SERVICE);
                    audio.adjustSuggestedStreamVolume(AudioManager.ADJUST_SAME, AudioManager.USE_DEFAULT_STREAM_TYPE, AudioManager.FLAG_SHOW_UI);
                    break;

                // Writing mode:
                case 'q':
                case 'k':
                    mActivity.toggleTerminalToolbar();
                    mVirtualFnKeyDown=false; // force disable fn key down to restore keyboard input into terminal view, fixes termux/termux-app#1420
                    break;

                case 'z': // Zecret :)
                    mActivity.requestAutoFill();
                    break;
            }

            if (resultingKeyCode != -1) {
                TerminalEmulator term = session.getEmulator();
                String keySequence = term.sendKeyEvent(resultingKeyCode, 0);
                if (keySequence != null) {
                    byte[] bytes = keySequence.getBytes(java.nio.charset.StandardCharsets.UTF_8);
                    session.write(bytes, 0, bytes.length);
                }
            } else if (resultingCodePoint != -1) {
                session.writeCodePoint(altDown, resultingCodePoint);
            }
            return true;
        } else if (ctrlDown) {
            if (codePoint == 106 /* Ctrl+j or \n */ && !session.isRunning()) {
                mTermuxTerminalSessionActivityClient.removeFinishedSession(session);
                return true;
            }
        }

        return false;
    }

    public void changeFontSize(boolean increase) {
        int newFontSize = mActivity.mPreferences.changeFontSize(increase);
        mActivity.getTerminalView().setTextSize(newFontSize);
    }

    /**
     * Called when user requests the soft keyboard to be toggled via "KEYBOARD" toggle button in
     * drawer or extra keys, or with ctrl+alt+k hardware keyboard shortcut.
     */
    public void onToggleSoftKeyboardRequest() {
        mActivity.getSystemService(InputMethodManager.class).toggleSoftInput(0, 0);
        mActivity.mTerminalView.requestFocus();
    }

    public void shareSessionTranscript() {
        TerminalSession session = mActivity.getCurrentSession();
        if (session == null) return;
        TerminalEmulator terminalEmulator = session.getEmulator();
        if (terminalEmulator == null) return;
        String sessionTranscript = terminalEmulator.getTranscriptText().trim();
        TermuxUrlUtils.shareText(mActivity, mActivity.getString(R.string.title_share_transcript),
            sessionTranscript, mActivity.getString(R.string.title_share_transcript_with));
    }

    public void shareSelectedText() {
        String selectedText = mActivity.getTerminalView().getStoredSelectedText();
        if (selectedText != null && !selectedText.isEmpty()) {
            TermuxUrlUtils.shareText(mActivity, mActivity.getString(R.string.title_share_selected_text),
                selectedText, mActivity.getString(R.string.title_share_selected_text_with));
        }
    }

    public void showUrlSelection() {
        TerminalSession session = mActivity.getCurrentSession();
        if (session == null) return;
        TerminalEmulator terminalEmulator = session.getEmulator();
        if (terminalEmulator == null) return;
        String sessionTranscript = terminalEmulator.getTranscriptText().trim();

        LinkedHashSet<CharSequence> urlSet = TermuxUrlUtils.extractUrls(sessionTranscript);
        if (urlSet.isEmpty()) {
            new AlertDialog.Builder(mActivity).setMessage(R.string.title_select_url_none_found).show();
            return;
        }

        final CharSequence[] urls = urlSet.toArray(new CharSequence[0]);
        Collections.reverse(Arrays.asList(urls)); // Latest first.

        // Click to copy url to clipboard:
        final AlertDialog dialog = new AlertDialog.Builder(mActivity).setItems(urls, (di, which) -> {
            String url = (String) urls[which];
            TermuxUrlUtils.copyTextToClipboard(mActivity, url, mActivity.getString(R.string.msg_select_url_copied_to_clipboard));
        }).setTitle(R.string.title_select_url_dialog).create();

        // Long press to open URL:
        dialog.setOnShowListener(di -> {
            ListView lv = dialog.getListView(); // this is a ListView with your "buds" in it
            lv.setOnItemLongClickListener((parent, view, position, id) -> {
                dialog.dismiss();
                String url = (String) urls[position];
                TermuxUrlUtils.openUrl(mActivity, url);
                return true;
            });
        });

        dialog.show();
    }

    public void doPaste() {
        TerminalSession session = mActivity.getCurrentSession();
        if (session == null || !session.isRunning()) {
            return;
        }

        String text = TermuxUrlUtils.getTextStringFromClipboardIfSet(mActivity, true);
        if (text != null) {
            session.getEmulator().paste(text);
        }
    }

    @Override
    public void logError(String tag, String message) {
        android.util.Log.e(tag, message);
    }

    @Override
    public void logWarn(String tag, String message) {
        android.util.Log.w(tag, message);
    }

    @Override
    public void logInfo(String tag, String message) {
        android.util.Log.i(tag, message);
    }

    @Override
    public void logDebug(String tag, String message) {
        android.util.Log.d(tag, message);
    }

    @Override
    public void logVerbose(String tag, String message) {
        android.util.Log.v(tag, message);
    }

    @Override
    public void logStackTraceWithMessage(String tag, String message, Exception e) {
        android.util.Log.e(tag, message, e);
    }

    @Override
    public void logStackTrace(String tag, Exception e) {
        android.util.Log.e(tag, "", e);
    }

    @Override
    public void onEmulatorSet() {
    }

    @Override
    public boolean shouldUseCtrlSpaceWorkaround() {
        return false;
    }

}
