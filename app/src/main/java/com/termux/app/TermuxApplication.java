package com.termux.app;

import android.app.Application;
import android.content.Context;

import com.termux.BuildConfig;
import com.termux.shared.errors.Error;
import com.termux.shared.logger.Logger;
import com.termux.shared.termux.TermuxBootstrap;
import com.termux.shared.termux.TermuxConstants;
import com.termux.shared.termux.crash.TermuxCrashUtils;
import com.termux.shared.termux.file.TermuxFileUtils;
import com.termux.shared.termux.settings.preferences.TermuxAppSharedPreferences;
import com.termux.shared.termux.settings.properties.TermuxAppSharedProperties;
import com.termux.shared.termux.shell.command.environment.TermuxShellEnvironment;
import com.termux.shared.termux.shell.am.TermuxAmSocketServer;
import com.termux.shared.termux.shell.TermuxShellManager;
import com.termux.shared.termux.theme.TermuxThemeUtils;
import com.termux.terminal.JNI;

public class TermuxApplication extends Application {

    private static final String LOG_TAG = "TermuxApplication";

    public void onCreate() {
        super.onCreate();

        Context context = getApplicationContext();

        // Set crash handler for the app
        TermuxCrashUtils.setDefaultCrashHandler(this);

        // Set log config for the app
        setLogConfig(context);

        Logger.logDebug("Starting Application");

        // 将应用版本号传给 Rust 层，供 env_builder 注入 TERMUX_VERSION 环境变量
        JNI.setTermuxVersion(BuildConfig.VERSION_NAME);

        // Set TermuxBootstrap.TERMUX_APP_PACKAGE_MANAGER and TermuxBootstrap.TERMUX_APP_PACKAGE_VARIANT
        TermuxBootstrap.setTermuxPackageManagerAndVariant(BuildConfig.TERMUX_PACKAGE_VARIANT);

        // Init app wide SharedProperties loaded from termux.properties
        TermuxAppSharedProperties properties = TermuxAppSharedProperties.init(context);

        // Init app wide shell manager
        TermuxShellManager shellManager = TermuxShellManager.init(context);

        // Set NightMode.APP_NIGHT_MODE
        TermuxThemeUtils.setAppNightMode(properties.getNightMode());

        // Check and create termux files directory. If failed to access it like in case of secondary
        // user or external sd card installation, then don't run files directory related code
        Error error = TermuxFileUtils.isTermuxFilesDirectoryAccessible(this, true, true);
        boolean isTermuxFilesDirectoryAccessible = error == null;
        if (isTermuxFilesDirectoryAccessible) {
            Logger.logInfo(LOG_TAG, "Termux files directory is accessible");

            error = TermuxFileUtils.isAppsTermuxAppDirectoryAccessible(true, true);
            if (error != null) {
                Logger.logErrorExtended(LOG_TAG, "Create apps/termux-app directory failed\n" + error);
                return;
            }

            // Setup termux-am-socket server
            TermuxAmSocketServer.setupTermuxAmSocketServer(context);
        } else {
            Logger.logErrorExtended(LOG_TAG, "Termux files directory is not accessible\n" + error);
        }

        // Init TermuxShellEnvironment constants and caches after everything has been setup including termux-am-socket server
        TermuxShellEnvironment.init(this);

        // 将 TERMUX_APP__* 扩展环境变量批量传给 Rust 层
        passExtendedEnvironmentToRust(context);

        if (isTermuxFilesDirectoryAccessible) {
            TermuxShellEnvironment.writeEnvironmentToFile(this);
        }
    }

    private void passExtendedEnvironmentToRust(Context context) {
        try {
            java.util.ArrayList<String> keys = new java.util.ArrayList<>();
            java.util.ArrayList<String> values = new java.util.ArrayList<>();

            android.content.pm.ApplicationInfo appInfo = getApplicationInfo();

            keys.add("TERMUX_APP__VERSION_NAME");     values.add(BuildConfig.VERSION_NAME);
            keys.add("TERMUX_APP__VERSION_CODE");     values.add(String.valueOf(BuildConfig.VERSION_CODE));
            keys.add("TERMUX_APP__PACKAGE_NAME");     values.add(TermuxConstants.TERMUX_PACKAGE_NAME);
            keys.add("TERMUX_APP__PID");              values.add(String.valueOf(android.os.Process.myPid()));
            keys.add("TERMUX_APP__UID");              values.add(String.valueOf(appInfo.uid));
            keys.add("TERMUX_APP__TARGET_SDK");       values.add(String.valueOf(appInfo.targetSdkVersion));
            keys.add("TERMUX_APP__APK_PATH");         values.add(appInfo.sourceDir);
            keys.add("TERMUX_APP__IS_DEBUGGABLE_BUILD"); values.add((appInfo.flags & android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0 ? "true" : "false");
            keys.add("TERMUX_APP__IS_INSTALLED_ON_EXTERNAL_STORAGE"); values.add((appInfo.flags & android.content.pm.ApplicationInfo.FLAG_EXTERNAL_STORAGE) != 0 ? "true" : "false");
            keys.add("TERMUX_APP__FILES_DIR");        values.add(context.getFilesDir().getAbsolutePath());

            if (TermuxBootstrap.TERMUX_APP_PACKAGE_MANAGER != null)
                { keys.add("TERMUX_APP__PACKAGE_MANAGER"); values.add(TermuxBootstrap.TERMUX_APP_PACKAGE_MANAGER.getName()); }
            if (TermuxBootstrap.TERMUX_APP_PACKAGE_VARIANT != null)
                { keys.add("TERMUX_APP__PACKAGE_VARIANT"); values.add(TermuxBootstrap.TERMUX_APP_PACKAGE_VARIANT.getName()); }

            keys.add("TERMUX_APP__AM_SOCKET_SERVER_ENABLED");
            values.add(String.valueOf(TermuxAmSocketServer.getTermuxAppAMSocketServerEnabled(context)));

            JNI.setExtendedEnvironment(keys.toArray(new String[0]), values.toArray(new String[0]));
            Logger.logDebug(LOG_TAG, "Passed " + keys.size() + " extended environment vars to Rust");
        } catch (Exception e) {
            Logger.logErrorExtended(LOG_TAG, "Failed to pass extended environment to Rust: " + e.getMessage());
        }
    }

    public static void setLogConfig(Context context) {
        Logger.setDefaultLogTag(TermuxConstants.TERMUX_APP_NAME);

        // Load the log level from shared preferences and set it to the {@link Logger.CURRENT_LOG_LEVEL}
        TermuxAppSharedPreferences preferences = TermuxAppSharedPreferences.build(context);
        if (preferences == null) return;
        preferences.setLogLevel(null, preferences.getLogLevel());
    }

}
