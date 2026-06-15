package com.termux.app;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.pm.ActivityInfo;
import android.os.Handler;
import android.os.Looper;
import android.system.Os;
import android.content.ActivityNotFoundException;
import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.ServiceConnection;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.IBinder;
import android.util.Log;
import android.view.ContextMenu;
import android.view.ContextMenu.ContextMenuInfo;
import android.view.Gravity;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.hardware.display.DisplayManager;
import android.view.Display;
import android.view.autofill.AutofillManager;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;
import android.widget.ListView;
import android.widget.Toast;

import java.io.BufferedInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.security.MessageDigest;

import com.termux.R;
import com.termux.BuildConfig;
import com.termux.app.extrakeys.ExtraKeysView;
import com.termux.app.extrakeys.TermuxTerminalExtraKeys;
import com.termux.app.extrakeys.TerminalToolbarViewPager;
import com.termux.terminal.TerminalSession;
import com.termux.terminal.TerminalSessionClient;
import com.termux.view.TerminalView;
import com.termux.view.TerminalViewClient;

import androidx.activity.OnBackPressedCallback;
import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;
import androidx.drawerlayout.widget.DrawerLayout;
import androidx.viewpager.widget.ViewPager;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;

/**
 * A terminal emulator activity.
 * <p/>
 * See
 * <ul>
 * <li>http://www.mongrel-phones.com.au/default/how_to_make_a_local_service_and_bind_to_it_in_android</li>
 * <li>https://code.google.com/p/android/issues/detail?id=6426</li>
 * </ul>
 * about memory leaks.
 */
public final class TermuxActivity extends AppCompatActivity implements ServiceConnection {

    public final Handler mMainThreadHandler = new Handler(Looper.getMainLooper());

    public static final String ACTION_RELOAD_STYLE = "com.termux.app.reload_style";
    public static final String ACTION_REQUEST_PERMISSIONS = "com.termux.app.request_storage_permissions";
    public static final String EXTRA_FAILSAFE_SESSION = "com.termux.app.failsafe_session";

    private static final int CONTEXT_MENU_SELECT_URL_ID = 0;
    private static final int CONTEXT_MENU_SHARE_TRANSCRIPT_ID = 1;
    private static final int CONTEXT_MENU_SHARE_SELECTED_TEXT = 2;
    private static final int CONTEXT_MENU_AUTOFILL_USERNAME = 3;
    private static final int CONTEXT_MENU_AUTOFILL_PASSWORD = 4;
    private static final int CONTEXT_MENU_RESET_TERMINAL_ID = 5;
    private static final int CONTEXT_MENU_KILL_PROCESS_ID = 6;
    private static final int CONTEXT_MENU_STYLING_ID = 7;
    private static final int CONTEXT_MENU_TOGGLE_KEEP_SCREEN_ON = 8;
    private static final int CONTEXT_MENU_FULLSCREEN_ID = 9;
    private static final int CONTEXT_MENU_EXPORT_LOGS_ID = 10;
    private static final int CONTEXT_MENU_EXPORT_ENV_CONFIG_ID = 11;
    private static final int CONTEXT_MENU_EXPORT_CMD_AVAILABILITY_ID = 12;

    private static final String ARG_TERMINAL_TOOLBAR_TEXT_INPUT = "terminal_toolbar_text_input";
    private static final String ARG_ACTIVITY_RECREATED = "activity_recreated";

    private static final int REQUEST_CODE_TERMUX_STYLING = 1;

    /**
     * The connection to the {@link TermuxService}. Requested in {@link #onCreate(Bundle)} with a call to
     * {@link #bindService(Intent, ServiceConnection, int)}, and obtained and stored in
     * {@link #onServiceConnected(ComponentName, IBinder)}.
     */
    TermuxService mTermuxService;

    /**
     * The {@link TerminalView} shown in  {@link TermuxActivity} that displays the terminal.
     */
    TerminalView mTerminalView;
    DisplayManager.DisplayListener mDisplayRotationListener;

    /**
     * The {@link TerminalViewClient} interface implementation to allow for communication between
     * {@link TerminalView} and {@link TermuxActivity}.
     */
    TermuxTerminalViewClient mTermuxTerminalViewClient;

    /**
     * The {@link TerminalSessionClient} interface implementation to allow for communication between
     * {@link TerminalSession} and {@link TermuxActivity}.
     */
    final TermuxTerminalSessionActivityClient mTermuxTerminalSessionActivityClient = new TermuxTerminalSessionActivityClient(this);

    /**
     * The terminal extra keys view.
     */
    ExtraKeysView mExtraKeysView;

    /**
     * The client for the {@link #mExtraKeysView}.
     */
    TermuxTerminalExtraKeys mTermuxTerminalExtraKeys;

    /**
     * The termux sessions list controller.
     */
    TermuxSessionsListViewController mTermuxSessionListViewController;

    /**
     * The {@link TermuxActivity} broadcast receiver for various things like terminal style configuration changes.
     */
    private final BroadcastReceiver mTermuxActivityBroadcastReceiver = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            if (mIsVisible) {
                if (ACTION_RELOAD_STYLE.equals(intent.getAction())) {
                    if ("storage".equals(intent.getStringExtra(ACTION_RELOAD_STYLE))) {
                        TermuxInstaller.setupStorageSymlinks(TermuxActivity.this);
                    } else {
                        reloadActivityStyling();
                    }
                }
            }
        }
    };

    /**
     * The last toast shown, used cancel current toast before showing new in {@link #showTransientMessage(String, boolean)}}.
     */
    Toast mLastToast;

    /**
     * If between onStart() and onStop(). Note that only one session is in the foreground of the terminal view at the
     * time, so if the session causing a change is not in the foreground it should probably be treated as background.
     */
    private boolean mIsVisible;

    private float mTerminalToolbarDefaultHeight;

    public final TermuxProperties mProperties = new TermuxProperties();

    public TermuxPreferences mPreferences;

    @Override
    public void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        TermuxConstants.init(this);
        ensureExecLib(this);

        // 启用广色域 (Wide Color Gamut) 模式。
        // 这允许 Hardware Composer (HWC) 接受 10-bit 格式（如 0x38），并利用 OLED 屏幕的 HDR 特性。
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getWindow().setColorMode(ActivityInfo.COLOR_MODE_HDR);
        }

        // 关键修复：禁用窗口动画，避免 SurfaceView 在 MIUI/HyperOS 的 transition 动画中
        // 被创建后销毁但不重建。
        getWindow().setWindowAnimations(0);

        if (savedInstanceState != null) {
            // mIsActivityRecreated = savedInstanceState.getBoolean(ARG_ACTIVITY_RECREATED, false);
        }

        mProperties.reloadProperties(this);
        mPreferences = new TermuxPreferences(this);

        // Ensure Rust engine knows our version for environment variable injection
        com.termux.terminal.JNI.setTermuxVersion(BuildConfig.VERSION_NAME);

        // 使用动态 Prefix 路径。
        // 之前的固定 /data/data 路径会导致多用户模式下 login 脚本失效。
        com.termux.terminal.JNI.setTermuxPrefix(TermuxConstants.PREFIX_PATH);

        setContentView(R.layout.activity_termux);

        mTermuxTerminalViewClient = new TermuxTerminalViewClient(this, mTermuxTerminalSessionActivityClient);

        mTerminalView = findViewById(R.id.terminal_view);
        mTerminalView.setTerminalViewClient(mTermuxTerminalViewClient);
        mTerminalView.setTextSize(mPreferences.getFontSize());

        mTermuxTerminalSessionActivityClient.onCreate();

        // MIUI/HyperOS workaround: onConfigurationChanged is NOT called during rotation
        // due to MIUI's "Allow fixed rotation for not collecting" mechanism.
        // Use DisplayListener to detect rotation directly.
        DisplayManager dm = (DisplayManager) getSystemService(DISPLAY_SERVICE);
        if (dm != null) {
            mDisplayRotationListener = new DisplayManager.DisplayListener() {
                private int mLastRotation = getDisplay().getRotation();

                @Override
                public void onDisplayChanged(int displayId) {
                    if (displayId != Display.DEFAULT_DISPLAY) return;
                    int newRotation = TermuxActivity.this.getDisplay().getRotation();
                    if (newRotation != mLastRotation) {
                        mLastRotation = newRotation;
                        Log.i(TermuxConstants.LOG_TAG, "Display rotation changed: " + mLastRotation + " -> " + newRotation);
                        // Directly signal TerminalView to update its layout, swapchain, and
                        // terminal dimensions. We cannot rely solely on requestLayout() because
                        // the View dimensions may not change (MIUI applies surface transforms),
                        // and onSizeChanged would not fire.
                        if (mTerminalView != null) {
                            mTerminalView.requestLayout();
                            mTerminalView.notifyConfigurationChanged();
                        }
                    }
                }
                @Override public void onDisplayAdded(int id) {}
                @Override public void onDisplayRemoved(int id) {}
            };
            dm.registerDisplayListener(mDisplayRotationListener, null);
        }

        setTerminalToolbarView(savedInstanceState);

        View newSessionButton = findViewById(R.id.new_session_button);
        newSessionButton.setOnClickListener(v -> {
            Log.d(TermuxConstants.LOG_TAG, "New Session button clicked");
            mTermuxTerminalSessionActivityClient.addNewSession(false, null, null, null);
        });
        newSessionButton.setOnLongClickListener(v -> {
            TermuxMessageDialogUtils.textInput(TermuxActivity.this,
                R.string.title_create_named_session,
                R.string.hint_session_name,
                null,
                R.string.action_create_named_session_confirm, sessionName -> mTermuxTerminalSessionActivityClient.addNewSession(false, sessionName, null, null),
                R.string.action_new_session_failsafe, sessionName -> mTermuxTerminalSessionActivityClient.addNewSession(true, sessionName, null, null),
                -1, null, null);
            return true;
        });
        View toggleKeyboardButton = findViewById(R.id.toggle_keyboard_button);
        toggleKeyboardButton.setOnClickListener(item -> {
            mTermuxTerminalViewClient.onToggleSoftKeyboardRequest();
            getDrawer().closeDrawers();
        });
        toggleKeyboardButton.setOnLongClickListener(v -> {
            toggleTerminalToolbar();
            return true;
        });

        registerForContextMenu(mTerminalView);

        // Start the {@link TermuxService} and make it run regardless of who is bound to it
        var serviceIntent = new Intent(this, TermuxService.class);
        startForegroundService(serviceIntent);

        // Attempt to bind to the service, this will call the {@link #onServiceConnected(ComponentName, IBinder)}
        // callback if it succeeds.
        if (!bindService(serviceIntent, this, 0)) {
            throw new RuntimeException("bindService() failed");
        }

        getOnBackPressedDispatcher().addCallback(this, new OnBackPressedCallback(true) {
            @Override
            public void handleOnBackPressed() {
                if (mTerminalView.isSelectingText()) {
                    mTerminalView.stopTextSelectionMode();
                } else if (getDrawer().isDrawerOpen(Gravity.LEFT)) {
                    getDrawer().closeDrawers();
                } else {
                    setEnabled(false);
                    getOnBackPressedDispatcher().onBackPressed();
                    setEnabled(true);
                }
            }
        });

        getWindow().getDecorView().setOnApplyWindowInsetsListener((view, insets) -> {
            // Note: Do NOT toggle terminalToolbarViewPager visibility based on IME state.
            // Extra keys (ESC/ALT/CTRL) should remain visible regardless of IME visibility
            // so that physical keyboard users can still access them.
            // Upstream behavior: visibility is controlled solely by user preference.
            TermuxFullscreen.updatePadding(this, insets);
            return insets;
        });

        getDrawer().addDrawerListener(new DrawerLayout.SimpleDrawerListener() {
            @Override
            public void onDrawerOpened(@NonNull View drawerView) {
                findViewById(R.id.terminal_sessions_list).requestFocus();
            }

            @Override
            public void onDrawerClosed(@NonNull View drawerView) {
                mTerminalView.requestFocus();
            }
        });
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, @Nullable Intent data) {
        super.onActivityResult(requestCode, resultCode, data);

        Log.i(TermuxConstants.LOG_TAG, "onActivityResult(request=" + requestCode + ", result=" + resultCode + ")");

        if (resultCode != Activity.RESULT_OK) {
            Log.e(TermuxConstants.LOG_TAG, "Failed activity result - request=" + requestCode + ", result=" + resultCode);
            return;
        }

        if (requestCode == REQUEST_CODE_TERMUX_STYLING) {
            try {
                var clipData = data.getClipData();
                if (clipData != null && clipData.getItemCount() == 1) {
                    var styleFileUri = clipData.getItemAt(0).getUri();
                    try (var in = getContentResolver().openInputStream(styleFileUri)) {
                        var out = new ByteArrayOutputStream();
                        var buffer = new byte[8196];
                        if (in != null) {
                            // Null input stream means default style.
                            int read;
                            while ((read = in.read(buffer)) != -1) {
                                out.write(buffer, 0, read);
                            }
                        }
                        var bytesReceived = out.toByteArray();
                        var isColors = styleFileUri.getPath().startsWith("/colors/");
                        var fileToWrite = new File(isColors ? TermuxConstants.COLORS_PATH : TermuxConstants.FONT_PATH);
                        var parentDir = fileToWrite.getParentFile();
                        if (parentDir == null || (!parentDir.isDirectory() && !parentDir.mkdirs())) {
                            showTransientMessage("Cannot create ~/.termux/ directory - check permissions in $HOME", true);
                            return;
                        }
                        if (bytesReceived.length == 0) {
                            if (!fileToWrite.delete()) {
                                Log.e(TermuxConstants.LOG_TAG, "Unable to delete file: " + fileToWrite.getAbsolutePath());
                            }
                        } else {
                            try (var fos = new FileOutputStream(fileToWrite)) {
                                fos.write(bytesReceived);
                            }
                        }
                        mTermuxTerminalSessionActivityClient.onReloadActivityStyling();
                    }
                }
            } catch (IOException e) {
                Log.e(TermuxConstants.LOG_TAG, "Error updating files", e);
                showTransientMessage("Error updating files - check file permissions in $HOME", true);
            }
        }
    }

    @Override
    public void onStart() {
        super.onStart();
        mIsVisible = true;

        mTermuxTerminalSessionActivityClient.onStart();
        if (mTermuxTerminalViewClient != null) {
            mTermuxTerminalViewClient.onStart();
        }
        registerTermuxActivityBroadcastReceiver();

        if (Build.VERSION.SDK_INT >= 33) {
            TermuxPermissionUtils.requestPermissions(this,
                TermuxPermissionUtils.REQUEST_POST_NOTIFICATIONS,
                Manifest.permission.POST_NOTIFICATIONS
            );
        }
    }

    @Override
    public void onResume() {
        super.onResume();
        mTermuxTerminalSessionActivityClient.onResume();
        if (!mPreferences.isFullscreen()) {
            mTerminalView.requestFocus();
        }
        applyFullscreenSetting(mPreferences.isFullscreen());
    }

    @Override
    protected void onStop() {
        super.onStop();
        mIsVisible = false;
        mTermuxTerminalSessionActivityClient.onStop();
        unregisterReceiver(mTermuxActivityBroadcastReceiver);
        getDrawer().closeDrawers();
    }

    @Override
    public void onDestroy() {
        super.onDestroy();

        if (mDisplayRotationListener != null) {
            DisplayManager dm = (DisplayManager) getSystemService(DISPLAY_SERVICE);
            if (dm != null) dm.unregisterDisplayListener(mDisplayRotationListener);
            mDisplayRotationListener = null;
        }

        if (mTermuxService != null) {
            // Do not leave service and session clients with references to activity.
            mTermuxService.unsetTermuxTerminalSessionClient(mTermuxTerminalSessionActivityClient);
            mTermuxService = null;
        }

        try {
            unbindService(this);
        } catch (Exception e) {
            // ignore.
        }
    }

    @Override
    public void onSaveInstanceState(@NonNull Bundle savedInstanceState) {
        super.onSaveInstanceState(savedInstanceState);
        saveTerminalToolbarTextInput(savedInstanceState);
        savedInstanceState.putBoolean(ARG_ACTIVITY_RECREATED, true);
    }

    private static class ExecuteIntentInfo {
        public final Intent intent;

        public ExecuteIntentInfo(Intent intent) {
            this.intent = intent;
        }

        public File executable() {
            var path = intent.getData() == null ? null : intent.getData().getPath();
            return path == null ? null : new File(path);
        }

        @Nullable public String sessionName() {
            var executable = executable();
            return executable == null ? null : executable.getName();
        }
    }

    ExecuteIntentInfo executableFromIntent(Intent intent) {
        if (intent == null) {
            return null;
        }
        if (intent.getComponent() != null &&
            TermuxConstants.TERMUX_INTERNAL_ACTIVITY.equals(intent.getComponent().getClassName()) &&
            Intent.ACTION_RUN.equals(intent.getAction())) {
            return new ExecuteIntentInfo(intent);
        }
        return null;
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        if (mTermuxService != null) {
            var executeIntentInfo = executableFromIntent(intent);
            if (mTermuxService != null && executeIntentInfo != null) {
                // A connection to the termux service has already been established in
                // onServiceConnected(), so handle the execute intent here now.
                mTermuxTerminalSessionActivityClient.addNewSession(false, executeIntentInfo.sessionName(), executeIntentInfo.executable(), executeIntentInfo.intent);
            }
        }
    }

    /**
     * Part of the {@link ServiceConnection} interface. The service is bound with
     * {@link #bindService(Intent, ServiceConnection, int)} in {@link #onCreate(Bundle)} which will cause a call to this
     * callback method.
     */
    @Override
    public void onServiceConnected(ComponentName componentName, IBinder service) {
        Log.i(TermuxConstants.LOG_TAG, "onServiceConnected: Activity connected to service");
        mTermuxService = ((TermuxService.LocalBinder) service).service;

        ListView termuxSessionsListView = findViewById(R.id.terminal_sessions_list);
        mTermuxSessionListViewController = new TermuxSessionsListViewController(this, mTermuxService.getTermuxSessions());
        termuxSessionsListView.setAdapter(mTermuxSessionListViewController);
        termuxSessionsListView.setOnItemClickListener(mTermuxSessionListViewController);
        termuxSessionsListView.setOnItemLongClickListener(mTermuxSessionListViewController);

        final Intent intent = getIntent();
        setIntent(null);

        var executeIntentInfo = executableFromIntent(intent);
        var sessionName = executeIntentInfo == null ? null : executeIntentInfo.sessionName();
        var executable = executeIntentInfo == null ? null : executeIntentInfo.executable();
        var executableIntent = executeIntentInfo == null ? null : executeIntentInfo.intent;
        boolean isFailSafe = intent.getBooleanExtra(EXTRA_FAILSAFE_SESSION, false);

        if (mTermuxService.isTermuxSessionsEmpty()) {
            TermuxInstaller.setupBootstrapIfNeeded(TermuxActivity.this, () -> {
                if (mTermuxService == null) {
                    // Activity might have been destroyed.
                    return;
                }
                try {
                    mTermuxTerminalSessionActivityClient.addNewSession(isFailSafe, sessionName, executable, executableIntent);
                } catch (WindowManager.BadTokenException e) {
                    // Activity finished - ignore.
                }
            });
        } else {
            // If termux was started from launcher "New session" shortcut and activity is recreated,
            // then the original intent will be re-delivered, resulting in a new session being re-added
            // each time.
            if (Intent.ACTION_RUN.equals(intent.getAction())) {
                // Android 7.1 app shortcut from res/xml/shortcuts.xml.
                mTermuxTerminalSessionActivityClient.addNewSession(isFailSafe, sessionName, executable, executableIntent);
            } else {
                mTermuxTerminalSessionActivityClient.setCurrentSession(mTermuxTerminalSessionActivityClient.getCurrentStoredSessionOrLast());
            }
        }

        // Update the {@link TerminalSession} and {@link TerminalEmulator} clients.
        mTermuxService.setTermuxTerminalSessionClient(mTermuxTerminalSessionActivityClient);

        // TODO: mTermuxSessionListViewController.notifyDataSetChanged();
    }

    @Override
    public void onServiceDisconnected(ComponentName name) {
        // Respect being stopped from the {@link TermuxService} notification action.
        finishActivityIfNotFinishing();
    }

    private void setTerminalToolbarView(Bundle savedInstanceState) {
        mTermuxTerminalExtraKeys = new TermuxTerminalExtraKeys(this, mTerminalView,
            mTermuxTerminalViewClient, mTermuxTerminalSessionActivityClient);

        final ViewPager terminalToolbarViewPager = getTerminalToolbarViewPager();
        if (mPreferences.isShowTerminalToolbar()) {
            terminalToolbarViewPager.setVisibility(View.VISIBLE);
        }

        ViewGroup.LayoutParams layoutParams = terminalToolbarViewPager.getLayoutParams();
        mTerminalToolbarDefaultHeight = layoutParams.height;

        setTerminalToolbarHeight();

        String savedTextInput = savedInstanceState == null ? null :
            savedInstanceState.getString(ARG_TERMINAL_TOOLBAR_TEXT_INPUT);

        terminalToolbarViewPager.setAdapter(new TerminalToolbarViewPager.PageAdapter(this, savedTextInput));
        terminalToolbarViewPager.addOnPageChangeListener(new TerminalToolbarViewPager.OnPageChangeListener(this, terminalToolbarViewPager));
    }

    private void setTerminalToolbarHeight() {
        final ViewPager terminalToolbarViewPager = getTerminalToolbarViewPager();
        if (terminalToolbarViewPager == null) return;

        ViewGroup.LayoutParams layoutParams = terminalToolbarViewPager.getLayoutParams();
        layoutParams.height = Math.round(mTerminalToolbarDefaultHeight *
            (mTermuxTerminalExtraKeys.getExtraKeysInfo() == null ? 0 : mTermuxTerminalExtraKeys.getExtraKeysInfo().getMatrix().length) *
            1
        );
        terminalToolbarViewPager.setLayoutParams(layoutParams);
    }

    public void toggleTerminalToolbar() {
        var terminalToolbarViewPager = getTerminalToolbarViewPager();
        if (terminalToolbarViewPager == null) return;

        final boolean showNow = mPreferences.toggleShowTerminalToolbar();
        showTransientMessage((showNow ? getString(R.string.msg_enabling_terminal_toolbar) : getString(R.string.msg_disabling_terminal_toolbar)), false);
        terminalToolbarViewPager.setVisibility(showNow ? View.VISIBLE : View.GONE);
        if (showNow && isTerminalToolbarTextInputViewSelected()) {
            // Focus the text input view if just revealed.
            findViewById(R.id.terminal_toolbar_text_input).requestFocus();
        }
    }

    private void saveTerminalToolbarTextInput(Bundle savedInstanceState) {
        if (savedInstanceState == null) return;

        final EditText textInputView = findViewById(R.id.terminal_toolbar_text_input);
        if (textInputView != null) {
            String textInput = textInputView.getText().toString();
            if (!textInput.isEmpty())
                savedInstanceState.putString(ARG_TERMINAL_TOOLBAR_TEXT_INPUT, textInput);
        }
    }

    public void finishActivityIfNotFinishing() {
        // prevent duplicate calls to finish() if called from multiple places
        if (!isFinishing()) {
            finish();
        }
    }

    /**
     * Show a transient message and dismiss the last one if still visible.
     */
    public void showTransientMessage(String text, boolean longDuration) {
        if (text == null || text.isEmpty()) return;
        if (mLastToast != null) mLastToast.cancel();
        mLastToast = Toast.makeText(this, text, longDuration ? Toast.LENGTH_LONG : Toast.LENGTH_SHORT);
        mLastToast.show();
    }

    @Override
    public void onCreateContextMenu(ContextMenu menu, View v, ContextMenuInfo menuInfo) {
        TerminalSession currentSession = getCurrentSession();
        if (currentSession == null) return;

        var autofillManager = getSystemService(AutofillManager.class);
        boolean addAutoFillMenu = (autofillManager != null && autofillManager.isEnabled());

        menu.add(Menu.NONE, CONTEXT_MENU_SELECT_URL_ID, Menu.NONE, R.string.action_select_url);
        menu.add(Menu.NONE, CONTEXT_MENU_SHARE_TRANSCRIPT_ID, Menu.NONE, R.string.action_share_transcript);

        if (mTerminalView.getStoredSelectedText() != null) {
            menu.add(Menu.NONE, CONTEXT_MENU_SHARE_SELECTED_TEXT, Menu.NONE, R.string.action_share_selected_text);
        }
        if (addAutoFillMenu) {
            menu.add(Menu.NONE, CONTEXT_MENU_AUTOFILL_USERNAME, Menu.NONE, R.string.action_autofill_username);
            menu.add(Menu.NONE, CONTEXT_MENU_AUTOFILL_PASSWORD, Menu.NONE, R.string.action_autofill_password);
        }
        menu.add(Menu.NONE, CONTEXT_MENU_RESET_TERMINAL_ID, Menu.NONE, R.string.action_reset_terminal);
        menu.add(Menu.NONE, CONTEXT_MENU_KILL_PROCESS_ID, Menu.NONE, getResources().getString(R.string.action_kill_process, getCurrentSession().getPid())).setEnabled(currentSession.isRunning());
        menu.add(Menu.NONE, CONTEXT_MENU_STYLING_ID, Menu.NONE, R.string.action_style_terminal);
        menu.add(Menu.NONE, CONTEXT_MENU_TOGGLE_KEEP_SCREEN_ON, Menu.NONE, R.string.action_toggle_keep_screen_on).setCheckable(true).setChecked(mTerminalView.getKeepScreenOn());
        menu.add(Menu.NONE, CONTEXT_MENU_FULLSCREEN_ID, Menu.NONE, R.string.action_fullscreen).setCheckable(true).setChecked(mPreferences.isFullscreen());
        menu.add(Menu.NONE, CONTEXT_MENU_EXPORT_LOGS_ID, Menu.NONE, R.string.action_export_logs);
        menu.add(Menu.NONE, CONTEXT_MENU_EXPORT_ENV_CONFIG_ID, Menu.NONE, R.string.action_export_env_config);
        menu.add(Menu.NONE, CONTEXT_MENU_EXPORT_CMD_AVAILABILITY_ID, Menu.NONE, R.string.action_export_cmd_availability);
    }

    /**
     * Hook system menu to show context menu instead.
     */
    @Override
    public boolean onCreateOptionsMenu(Menu menu) {
        mTerminalView.showContextMenu();
        return false;
    }

    @Override
    public boolean onContextItemSelected(MenuItem item) {
        TerminalSession session = getCurrentSession();

        switch (item.getItemId()) {
            case CONTEXT_MENU_SELECT_URL_ID:
                mTermuxTerminalViewClient.showUrlSelection();
                return true;
            case CONTEXT_MENU_SHARE_TRANSCRIPT_ID:
                mTermuxTerminalViewClient.shareSessionTranscript();
                return true;
            case CONTEXT_MENU_SHARE_SELECTED_TEXT:
                mTermuxTerminalViewClient.shareSelectedText();
                return true;
            case CONTEXT_MENU_AUTOFILL_USERNAME:
                mTerminalView.requestAutoFillUsername();
                return true;
            case CONTEXT_MENU_AUTOFILL_PASSWORD:
                mTerminalView.requestAutoFillPassword();
                return true;
            case CONTEXT_MENU_RESET_TERMINAL_ID:
                if (session != null) {
                    session.reset();
                    showTransientMessage(getResources().getString(R.string.msg_terminal_reset), true);
                }
                return true;
            case CONTEXT_MENU_KILL_PROCESS_ID:
                showKillSessionDialog(session);
                return true;
            case CONTEXT_MENU_STYLING_ID:
                showStylingDialog();
                return true;
            case CONTEXT_MENU_TOGGLE_KEEP_SCREEN_ON:
                toggleKeepScreenOn();
                return true;
            case CONTEXT_MENU_FULLSCREEN_ID:
                mPreferences.toggleFullscreen();
                applyFullscreenSetting(mPreferences.isFullscreen());
                return true;
            case CONTEXT_MENU_EXPORT_LOGS_ID:
                exportDebugLogs();
                return true;
            case CONTEXT_MENU_EXPORT_ENV_CONFIG_ID:
                exportEnvConfig();
                return true;
            case CONTEXT_MENU_EXPORT_CMD_AVAILABILITY_ID:
                exportCommandAvailability();
                return true;
            default:
                return super.onContextItemSelected(item);
        }
    }

    @Override
    public void onContextMenuClosed(@NonNull Menu menu) {
        super.onContextMenuClosed(menu);
        // onContextMenuClosed() is triggered twice if back button is pressed to dismiss instead of tap for some reason
        mTerminalView.onContextMenuClosed(menu);
    }

    private void showKillSessionDialog(TerminalSession session) {
        if (session == null) return;

        final AlertDialog.Builder b = new AlertDialog.Builder(this);
        b.setIcon(android.R.drawable.ic_dialog_alert);
        b.setMessage(R.string.title_confirm_kill_process);
        b.setPositiveButton(android.R.string.ok, (dialog, id) -> {
            dialog.dismiss();
            session.finishIfRunning();
        });
        b.setNegativeButton(android.R.string.cancel, null);
        b.show();
    }

    private void showStylingDialog() {
        try {
            //noinspection deprecation
            startActivityForResult(new Intent().setClassName("com.termux.styling", "com.termux.styling.TermuxStyleActivity"), REQUEST_CODE_TERMUX_STYLING);
        } catch (ActivityNotFoundException | IllegalArgumentException | SecurityException e) {
            // The startActivity() call is not documented to throw IllegalArgumentException.
            // However, crash reporting shows that it sometimes does, so catch it here.
            // The SecurityException may happen if app is not allowed to start TermuxStyleActivity (old installation or non-google play build).
            Log.i(TermuxConstants.LOG_TAG, "Error starting Termux:Style - app needs to be installed", e);

            var installationUrl = "https://play.google.com/store/apps/details?id=com.termux.styling";
            new AlertDialog.Builder(this).setMessage(getString(R.string.error_styling_not_installed))
                .setPositiveButton(R.string.action_styling_install,
                    (dialog, which) -> startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(installationUrl))))
                .setNegativeButton(R.string.cancel, null).show();
        }
    }

    private void toggleKeepScreenOn() {
        boolean newValue = !mTerminalView.getKeepScreenOn();
        mTerminalView.setKeepScreenOn(newValue);
        mPreferences.setKeepScreenOn(newValue);
    }

    private void exportDebugLogs() {
        try {
            String logs = TermuxLogCollector.collect(this);
            android.content.Intent shareIntent = new android.content.Intent(android.content.Intent.ACTION_SEND);
            shareIntent.setType("text/plain");
            shareIntent.putExtra(android.content.Intent.EXTRA_SUBJECT, getString(R.string.title_export_logs));
            shareIntent.putExtra(android.content.Intent.EXTRA_TEXT, logs);
            startActivity(android.content.Intent.createChooser(shareIntent, getString(R.string.title_export_logs)));
        } catch (Exception e) {
            TermuxLogger.e("App", "Failed to export logs", e);
            showTransientMessage("Failed to export logs: " + e.getMessage(), true);
        }
    }

    private void exportEnvConfig() {
        try {
            String logs = TermuxLogCollector.collectEnvConfig(this);
            android.content.Intent shareIntent = new android.content.Intent(android.content.Intent.ACTION_SEND);
            shareIntent.setType("text/plain");
            shareIntent.putExtra(android.content.Intent.EXTRA_SUBJECT, getString(R.string.title_export_env_config));
            shareIntent.putExtra(android.content.Intent.EXTRA_TEXT, logs);
            startActivity(android.content.Intent.createChooser(shareIntent, getString(R.string.title_export_env_config)));
        } catch (Exception e) {
            TermuxLogger.e("App", "Failed to export env config", e);
            showTransientMessage("Failed to export env config: " + e.getMessage(), true);
        }
    }

    private void exportCommandAvailability() {
        try {
            String logs = TermuxLogCollector.collectCommandAvailability(this);
            android.content.Intent shareIntent = new android.content.Intent(android.content.Intent.ACTION_SEND);
            shareIntent.setType("text/plain");
            shareIntent.putExtra(android.content.Intent.EXTRA_SUBJECT, getString(R.string.title_export_cmd_availability));
            shareIntent.putExtra(android.content.Intent.EXTRA_TEXT, logs);
            startActivity(android.content.Intent.createChooser(shareIntent, getString(R.string.title_export_cmd_availability)));
        } catch (Exception e) {
            TermuxLogger.e("App", "Failed to export command availability", e);
            showTransientMessage("Failed to export command availability: " + e.getMessage(), true);
        }
    }

    public void requestAutoFill() {
        var autofillManager = getSystemService(AutofillManager.class);
        if (autofillManager != null && autofillManager.isEnabled()) {
            autofillManager.requestAutofill(mTerminalView);
        }
    }

    public ExtraKeysView getExtraKeysView() {
        return mExtraKeysView;
    }

    public TermuxTerminalExtraKeys getTermuxTerminalExtraKeys() {
        return mTermuxTerminalExtraKeys;
    }

    public void setExtraKeysView(ExtraKeysView extraKeysView) {
        mExtraKeysView = extraKeysView;
    }

    public DrawerLayout getDrawer() {
        return findViewById(R.id.drawer_layout);
    }


    public ViewPager getTerminalToolbarViewPager() {
        return findViewById(R.id.terminal_toolbar_view_pager);
    }

    public float getTerminalToolbarDefaultHeight() {
        return mTerminalToolbarDefaultHeight;
    }

    public boolean isTerminalViewSelected() {
        return getTerminalToolbarViewPager().getCurrentItem() == 0;
    }

    public boolean isTerminalToolbarTextInputViewSelected() {
        return getTerminalToolbarViewPager().getCurrentItem() == 1;
    }

    public boolean isVisible() {
        return mIsVisible;
    }

    public TermuxService getTermuxService() {
        return mTermuxService;
    }

    public TerminalView getTerminalView() {
        return mTerminalView;
    }

    public TermuxTerminalSessionActivityClient getTermuxTerminalSessionClient() {
        return mTermuxTerminalSessionActivityClient;
    }

    @Nullable
    public TerminalSession getCurrentSession() {
        return mTerminalView == null ? null : mTerminalView.getCurrentSession();
    }

    private void registerTermuxActivityBroadcastReceiver() {
        var intentFilter = new IntentFilter();
        intentFilter.addAction(ACTION_RELOAD_STYLE);
        intentFilter.addAction(ACTION_REQUEST_PERMISSIONS);

        var flag = Build.VERSION.SDK_INT >= 33 ? Context.RECEIVER_NOT_EXPORTED : 0;
        registerReceiver(mTermuxActivityBroadcastReceiver, intentFilter, flag);
    }

    private void reloadActivityStyling() {
        mProperties.reloadProperties(this);

        if (mExtraKeysView != null) {
            //mExtraKeysView.setButtonTextAllCaps(mProperties.shouldExtraKeysTextBeAllCaps());
            mTermuxTerminalExtraKeys.loadExtraKeysFromProperties();
            mExtraKeysView.reload(mTermuxTerminalExtraKeys.getExtraKeysInfo(), mTerminalToolbarDefaultHeight);
        }

        setTerminalToolbarHeight();

        mTermuxTerminalSessionActivityClient.onReloadActivityStyling();
    }

    void applyFullscreenSetting(boolean doFullscreen) {
        var rootView = findViewById(R.id.activity_termux_root_relative_layout);
        var windowInsetsController = WindowCompat.getInsetsController(getWindow(), rootView);

        if (doFullscreen) {
            var imm = getSystemService(InputMethodManager.class);
            imm.hideSoftInputFromWindow(rootView.getWindowToken(), 0);

            // Modern edge-to-edge: no FLAG_LAYOUT_NO_LIMITS.
            // WindowInsets drive correct padding via TermuxFullscreen.updatePadding(),
            // which now accounts for statusBar top in fullscreen mode.
            rootView.setFitsSystemWindows(false);

            windowInsetsController.hide(WindowInsetsCompat.Type.systemBars());
            windowInsetsController.setSystemBarsBehavior(WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
        } else {
            // Do not let ViewRoot apply implicit system-window padding to the
            // root hierarchy. TerminalView is a SurfaceView backed by a
            // separate SurfaceFlinger layer; mixing fitsSystemWindows with the
            // translucent/edge-to-edge window makes the Surface layer start at
            // the status-bar inset (for example y=150) while the Java overlays
            // and IME still use window coordinates. Keep the root in one
            // explicit coordinate space and let WindowInsets drive only the
            // fullscreen rounded-corner/IME padding path in TermuxFullscreen.
            rootView.setFitsSystemWindows(false);
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS);
            windowInsetsController.show(WindowInsetsCompat.Type.systemBars());
        }
        rootView.requestApplyInsets();
    }

    /**
     * Ensure the rust-exec preload library is copied to a fixed path
     * (/data/data/com.termux/files/exec/libtermux-exec.so) so that
     * LD_PRELOAD remains stable across APK updates.
     */
    private static void ensureExecLib(Context context) {
        File execDir = new File(TermuxConstants.EXEC_PATH);
        if (!execDir.exists() && !execDir.mkdirs()) {
            Log.e(TermuxConstants.LOG_TAG, "Failed to create exec directory");
            return;
        }

        File apkLib = new File(context.getApplicationInfo().nativeLibraryDir, "libtermux-exec.so");
        File execLib = new File(TermuxConstants.EXEC_PATH, "libtermux-exec.so");

        if (!apkLib.exists()) {
            Log.w(TermuxConstants.LOG_TAG, "APK libtermux-exec.so not found");
            return;
        }

        try {
            String apkHash = sha256(apkLib);
            String execHash = execLib.exists() ? sha256(execLib) : "";
            if (apkHash.equals(execHash)) {
                return;
            }
        } catch (Exception e) {
            Log.w(TermuxConstants.LOG_TAG, "Hash comparison failed, forcing copy", e);
        }

        try (FileInputStream in = new FileInputStream(apkLib);
             FileOutputStream out = new FileOutputStream(execLib)) {
            byte[] buffer = new byte[8192];
            int read;
            while ((read = in.read(buffer)) != -1) {
                out.write(buffer, 0, read);
            }
            //noinspection OctalInteger
            android.system.Os.chmod(execLib.getAbsolutePath(), 0700);
            Log.i(TermuxConstants.LOG_TAG, "Copied libtermux-exec.so to " + execLib.getAbsolutePath());
        } catch (Exception e) {
            Log.e(TermuxConstants.LOG_TAG, "Failed to copy libtermux-exec.so", e);
        }
    }

    private static String sha256(File file) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (BufferedInputStream in = new BufferedInputStream(new FileInputStream(file))) {
            byte[] buffer = new byte[8192];
            int read;
            while ((read = in.read(buffer)) != -1) {
                digest.update(buffer, 0, read);
            }
        }
        StringBuilder sb = new StringBuilder();
        for (byte b : digest.digest()) {
            sb.append(String.format("%02x", b));
        }
        return sb.toString();
    }

    @Override
    public void onConfigurationChanged(@NonNull android.content.res.Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        var root = findViewById(R.id.activity_termux_root_relative_layout);
        if (root != null) {
            root.requestLayout();
        }
        // SurfaceView 需要额外触发 updateSurface() 来适配新尺寸
        if (mTerminalView != null) {
            mTerminalView.requestLayout();
            mTerminalView.notifyConfigurationChanged();
        }
        getWindow().getDecorView().requestApplyInsets();
    }

}
