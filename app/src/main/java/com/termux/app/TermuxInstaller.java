package com.termux.app;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.ProgressDialog;
import android.content.Context;
import android.os.Build;
import android.os.Environment;
import android.system.Os;
import android.view.WindowManager;

import com.termux.R;
import com.termux.shared.file.FileUtils;
import com.termux.shared.termux.crash.TermuxCrashUtils;
import com.termux.shared.termux.file.TermuxFileUtils;
import com.termux.shared.interact.MessageDialogUtils;
import com.termux.shared.logger.Logger;
import com.termux.shared.markdown.MarkdownUtils;
import com.termux.shared.errors.Error;
import com.termux.shared.android.PackageUtils;
import com.termux.shared.android.PermissionUtils;
import com.termux.shared.termux.TermuxConstants;
import com.termux.shared.termux.TermuxUtils;
import com.termux.shared.termux.shell.command.environment.TermuxShellEnvironment;

import java.io.File;

import static com.termux.shared.termux.TermuxConstants.TERMUX_PREFIX_DIR;
import static com.termux.shared.termux.TermuxConstants.TERMUX_PREFIX_DIR_PATH;
import static com.termux.shared.termux.TermuxConstants.TERMUX_STAGING_PREFIX_DIR;
import static com.termux.shared.termux.TermuxConstants.TERMUX_STAGING_PREFIX_DIR_PATH;

/**
 * Install the Termux bootstrap packages if necessary by following the below steps:
 * <p/>
 * (1) If $PREFIX already exist, assume that it is correct and be done. Note that this relies on that we do not create a
 * broken $PREFIX directory below.
 * <p/>
 * (2) A progress dialog is shown with "Installing..." message and a spinner.
 * <p/>
 * (3) A staging directory, $STAGING_PREFIX, is cleared if left over from broken installation below.
 * <p/>
 * (4) The zip file is loaded from a shared library.
 * <p/>
 * (5) The zip is extracted using Rust implementation to $STAGING_PREFIX.
 */
final class TermuxInstaller {

    private static final String LOG_TAG = "TermuxInstaller";

    private static boolean sIsBootstrapInstallationRunning = false;

    /**
     * Performs bootstrap setup if necessary.
     * 
     * Bootstrap 触发条件：
     * 1. PREFIX 目录不存在，或
     * 2. PREFIX 目录存在但为空（只包含不重要的文件）
     * 
     * 不触发的情况：
     * 1. PREFIX 目录存在且非空（已有完整的 bootstrap 安装）
     * 2. Bootstrap 安装已经在运行中（防止并发）
     * 3. Files 目录不可访问（权限问题）
     * 4. 不是主用户（多用户场景）
     */
    static synchronized void setupBootstrapIfNeeded(final Activity activity, final Runnable whenDone) {
        Logger.logInfo(LOG_TAG, "========== [Bootstrap Check Start] ==========");
        Logger.logInfo(LOG_TAG, "TERMUX_PREFIX_DIR_PATH: " + TERMUX_PREFIX_DIR_PATH);
        Logger.logInfo(LOG_TAG, "TERMUX_STAGING_PREFIX_DIR_PATH: " + TERMUX_STAGING_PREFIX_DIR_PATH);
        
        if (sIsBootstrapInstallationRunning) {
            Logger.logWarn(LOG_TAG, "[SKIP] Bootstrap installation is already running, skipping.");
            return;
        }

        String bootstrapErrorMessage;
        Error filesDirectoryAccessibleError;

        // Step 1: Check files directory accessibility
        Logger.logInfo(LOG_TAG, "[Step 1] Checking files directory accessibility...");
        filesDirectoryAccessibleError = TermuxFileUtils.isTermuxFilesDirectoryAccessible(activity, true, true);
        boolean isFilesDirectoryAccessible = filesDirectoryAccessibleError == null;
        Logger.logInfo(LOG_TAG, "Files directory accessible: " + isFilesDirectoryAccessible);

        // Step 2: Check if primary user
        Logger.logInfo(LOG_TAG, "[Step 2] Checking if primary user...");
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N && !PackageUtils.isCurrentUserThePrimaryUser(activity)) {
            bootstrapErrorMessage = activity.getString(R.string.bootstrap_error_not_primary_user_message,
                MarkdownUtils.getMarkdownCodeForString(TERMUX_PREFIX_DIR_PATH, false));
            Logger.logError(LOG_TAG, "[BLOCK] Not primary user - " + bootstrapErrorMessage);
            sendBootstrapCrashReportNotification(activity, bootstrapErrorMessage);
            MessageDialogUtils.exitAppWithErrorMessage(activity,
                activity.getString(R.string.bootstrap_error_title),
                bootstrapErrorMessage);
            return;
        }

        // Step 3: Check files directory access result
        Logger.logInfo(LOG_TAG, "[Step 3] Evaluating files directory access...");
        if (!isFilesDirectoryAccessible) {
            bootstrapErrorMessage = Error.getMinimalErrorString(filesDirectoryAccessibleError);
            //noinspection SdCardPath
            if (PackageUtils.isAppInstalledOnExternalStorage(activity) &&
                !TermuxConstants.TERMUX_FILES_DIR_PATH.equals(activity.getFilesDir().getAbsolutePath().replaceAll("^/data/user/0/", "/data/user/0/"))) {
                bootstrapErrorMessage += "\n\n" + activity.getString(R.string.bootstrap_error_installed_on_portable_sd,
                    MarkdownUtils.getMarkdownCodeForString(TERMUX_PREFIX_DIR_PATH, false));
            }

            Logger.logError(LOG_TAG, "[BLOCK] Files directory not accessible - " + bootstrapErrorMessage);
            sendBootstrapCrashReportNotification(activity, bootstrapErrorMessage);
            MessageDialogUtils.showMessage(activity,
                activity.getString(R.string.bootstrap_error_title),
                bootstrapErrorMessage, null);
            return;
        }

        // Step 4: Check PREFIX directory status
        Logger.logInfo(LOG_TAG, "[Step 4] Checking PREFIX directory status...");
        boolean prefixExists = FileUtils.directoryFileExists(TERMUX_PREFIX_DIR_PATH, true);
        Logger.logInfo(LOG_TAG, "PREFIX directory exists: " + prefixExists + " at " + TERMUX_PREFIX_DIR_PATH);
        
        if (prefixExists) {
            Logger.logInfo(LOG_TAG, "[Check] PREFIX directory exists, checking if empty...");
            boolean isPrefixEmpty = TermuxFileUtils.isTermuxPrefixDirectoryEmpty();
            Logger.logInfo(LOG_TAG, "PREFIX directory is empty: " + isPrefixEmpty);
            
            if (isPrefixEmpty) {
                Logger.logInfo(LOG_TAG, "[PROCEED] PREFIX exists but empty, will install bootstrap.");
            } else {
                Logger.logInfo(LOG_TAG, "[SKIP] PREFIX not empty, skipping bootstrap. whenDone.run()");
                whenDone.run();
                return;
            }
        } else {
            boolean fileExistsAtPath = FileUtils.fileExists(TERMUX_PREFIX_DIR_PATH, false);
            Logger.logInfo(LOG_TAG, "[PROCEED] PREFIX does not exist, file at path: " + fileExistsAtPath + ", will install bootstrap.");
        }

        // Step 5: Start bootstrap installation
        Logger.logInfo(LOG_TAG, "[Step 5] Starting bootstrap installation...");
        sIsBootstrapInstallationRunning = true;
        final ProgressDialog progress = ProgressDialog.show(activity, null, activity.getString(R.string.bootstrap_installer_body), true, false);
        new Thread() {
            @Override
            public void run() {
                try {
                    Logger.logInfo(LOG_TAG, "[Step 5.1] Installing bootstrap packages...");

                    Error error;

                    // Step 5.2: Delete staging directory
                    Logger.logInfo(LOG_TAG, "[Step 5.2] Deleting staging directory: " + TERMUX_STAGING_PREFIX_DIR_PATH);
                    error = FileUtils.deleteFile("termux prefix staging directory", TERMUX_STAGING_PREFIX_DIR_PATH, true);
                    if (error != null) {
                        Logger.logError(LOG_TAG, "[ERROR] Failed to delete staging directory: " + error.getMessage());
                        showBootstrapErrorDialog(activity, whenDone, Error.getErrorMarkdownString(error));
                        return;
                    }
                    Logger.logInfo(LOG_TAG, "[OK] Staging directory cleaned/deleted");

                    // Step 5.3: Delete PREFIX directory
                    Logger.logInfo(LOG_TAG, "[Step 5.3] Deleting PREFIX directory: " + TERMUX_PREFIX_DIR_PATH);
                    error = FileUtils.deleteFile("termux prefix directory", TERMUX_PREFIX_DIR_PATH, true);
                    if (error != null) {
                        Logger.logError(LOG_TAG, "[ERROR] Failed to delete PREFIX directory: " + error.getMessage());
                        showBootstrapErrorDialog(activity, whenDone, Error.getErrorMarkdownString(error));
                        return;
                    }
                    Logger.logInfo(LOG_TAG, "[OK] PREFIX directory cleaned/deleted");

                    // Step 5.4: Create staging directory
                    Logger.logInfo(LOG_TAG, "[Step 5.4] Creating staging directory...");
                    error = TermuxFileUtils.isTermuxPrefixStagingDirectoryAccessible(true, true);
                    if (error != null) {
                        Logger.logError(LOG_TAG, "[ERROR] Failed to create staging directory: " + error.getMessage());
                        showBootstrapErrorDialog(activity, whenDone, Error.getErrorMarkdownString(error));
                        return;
                    }
                    Logger.logInfo(LOG_TAG, "[OK] Staging directory created at: " + TERMUX_STAGING_PREFIX_DIR_PATH);

                    // Step 5.5: Create PREFIX directory
                    Logger.logInfo(LOG_TAG, "[Step 5.5] Creating PREFIX directory...");
                    error = TermuxFileUtils.isTermuxPrefixDirectoryAccessible(true, true);
                    if (error != null) {
                        Logger.logError(LOG_TAG, "[ERROR] Failed to create PREFIX directory: " + error.getMessage());
                        showBootstrapErrorDialog(activity, whenDone, Error.getErrorMarkdownString(error));
                        return;
                    }
                    Logger.logInfo(LOG_TAG, "[OK] PREFIX directory created at: " + TERMUX_PREFIX_DIR_PATH);

                    // Step 5.6: Load and extract bootstrap zip
                    Logger.logInfo(LOG_TAG, "[Step 5.6] Loading bootstrap zip bytes...");
                    final byte[] zipBytes = loadZipBytes();
                    Logger.logInfo(LOG_TAG, "[OK] Loaded bootstrap zip, size: " + zipBytes.length + " bytes");

                    Logger.logInfo(LOG_TAG, "[Step 5.7] Extracting bootstrap using Rust to: " + TERMUX_STAGING_PREFIX_DIR_PATH);
                    long startTime = System.currentTimeMillis();
                    
                    // Use Rust to extract bootstrap
                    boolean extractSuccess = BootstrapExtractor.extractBootstrap(zipBytes, TERMUX_STAGING_PREFIX_DIR_PATH);
                    long extractTime = System.currentTimeMillis() - startTime;
                    Logger.logInfo(LOG_TAG, "[Step 5.7 Complete] Extraction took: " + extractTime + "ms, success: " + extractSuccess);
                    
                    if (!extractSuccess) {
                        Logger.logError(LOG_TAG, "[ERROR] Bootstrap extraction failed");
                        showBootstrapErrorDialog(activity, whenDone, "Bootstrap extraction failed with error");
                        return;
                    }

                    // Step 5.8: Verify extraction result
                    Logger.logInfo(LOG_TAG, "[Step 5.8] Verifying extraction result...");
                    boolean stagingExists = FileUtils.directoryFileExists(TERMUX_STAGING_PREFIX_DIR_PATH, true);
                    Logger.logInfo(LOG_TAG, "Staging directory exists after extraction: " + stagingExists);
                    
                    if (stagingExists) {
                        try {
                            String[] files = TERMUX_STAGING_PREFIX_DIR.list();
                            if (files != null) {
                                Logger.logInfo(LOG_TAG, "Staging directory contents: " + java.util.Arrays.toString(files));
                            }
                        } catch (Exception e) {
                            Logger.logError(LOG_TAG, "Failed to list staging directory: " + e.getMessage());
                        }
                    }

                    // Step 5.9: Move staging to PREFIX
                    Logger.logInfo(LOG_TAG, "[Step 5.9] Moving staging to PREFIX...");
                    if (!TERMUX_STAGING_PREFIX_DIR.renameTo(TERMUX_PREFIX_DIR)) {
                        Logger.logError(LOG_TAG, "[ERROR] Failed to move staging to PREFIX");
                        throw new RuntimeException("Moving termux prefix staging to prefix directory failed");
                    }
                    Logger.logInfo(LOG_TAG, "[OK] Staging moved to PREFIX");

                    // Step 5.10: Verify final PREFIX
                    Logger.logInfo(LOG_TAG, "[Step 5.10] Verifying final PREFIX directory...");
                    boolean finalPrefixExists = FileUtils.directoryFileExists(TERMUX_PREFIX_DIR_PATH, true);
                    Logger.logInfo(LOG_TAG, "Final PREFIX directory exists: " + finalPrefixExists);
                    
                    if (finalPrefixExists) {
                        try {
                            String[] files = TERMUX_PREFIX_DIR.list();
                            if (files != null) {
                                Logger.logInfo(LOG_TAG, "Final PREFIX contents: " + java.util.Arrays.toString(files));
                            }
                        } catch (Exception e) {
                            Logger.logError(LOG_TAG, "Failed to list PREFIX directory: " + e.getMessage());
                        }
                    }

                    Logger.logInfo(LOG_TAG, "[Step 5.11] Writing environment file...");
                    TermuxShellEnvironment.writeEnvironmentToFile(activity);
                    Logger.logInfo(LOG_TAG, "[OK] Environment file written");

                    Logger.logInfo(LOG_TAG, "========== [Bootstrap Installation Complete] ==========");
                    activity.runOnUiThread(whenDone);

                } catch (final Exception e) {
                    Logger.logError(LOG_TAG, "[EXCEPTION] Bootstrap installation failed: " + e.getMessage());
                    showBootstrapErrorDialog(activity, whenDone, Logger.getStackTracesMarkdownString(null, Logger.getStackTracesStringArray(e)));

                } finally {
                    synchronized (TermuxInstaller.class) {
                        sIsBootstrapInstallationRunning = false;
                    }
                    Logger.logInfo(LOG_TAG, "[Cleanup] Reset installation running flag");
                    activity.runOnUiThread(() -> {
                        try {
                            progress.dismiss();
                            Logger.logInfo(LOG_TAG, "[Cleanup] Progress dialog dismissed");
                        } catch (RuntimeException e) {
                            Logger.logWarn(LOG_TAG, "[Cleanup] Failed to dismiss progress: " + e.getMessage());
                        }
                    });
                }
            }
        }.start();
    }

    public static void showBootstrapErrorDialog(Activity activity, Runnable whenDone, String message) {
        synchronized (TermuxInstaller.class) {
            sIsBootstrapInstallationRunning = false;
        }
        Logger.logErrorExtended(LOG_TAG, "Bootstrap Error:\n" + message);

        // Send a notification with the exception so that the user knows why bootstrap setup failed
        sendBootstrapCrashReportNotification(activity, message);

        activity.runOnUiThread(() -> {
            try {
                new AlertDialog.Builder(activity).setTitle(R.string.bootstrap_error_title).setMessage(R.string.bootstrap_error_body)
                    .setNegativeButton(R.string.bootstrap_error_abort, (dialog, which) -> {
                        dialog.dismiss();
                        activity.finish();
                    })
                    .setPositiveButton(R.string.bootstrap_error_try_again, (dialog, which) -> {
                        dialog.dismiss();
                        FileUtils.deleteFile("termux prefix directory", TERMUX_PREFIX_DIR_PATH, true);
                        TermuxInstaller.setupBootstrapIfNeeded(activity, whenDone);
                    }).show();
            } catch (WindowManager.BadTokenException e1) {
                // Activity already dismissed - ignore.
            }
        });
    }

    private static void sendBootstrapCrashReportNotification(Activity activity, String message) {
        final String title = TermuxConstants.TERMUX_APP_NAME + " Bootstrap Error";

        // Add info of all install Termux plugin apps as well since their target sdk or installation
        // on external/portable sd card can affect Termux app files directory access or exec.
        TermuxCrashUtils.sendCrashReportNotification(activity, LOG_TAG,
            title, null, "## " + title + "\n\n" + message + "\n\n" +
                TermuxUtils.getTermuxDebugMarkdownString(activity),
            true, false, TermuxUtils.AppInfoMode.TERMUX_AND_PLUGIN_PACKAGES, true);
    }

    static void setupStorageSymlinks(final Context context) {
        final String LOG_TAG = "termux-storage";
        final String title = TermuxConstants.TERMUX_APP_NAME + " Setup Storage Error";

        Logger.logInfo(LOG_TAG, "Setting up storage symlinks.");

        // 在开始前检查权限是否已经获得，尤其针对 Android 11+ (API 30+)
        boolean isLegacy = PermissionUtils.isLegacyExternalStoragePossible(context);
        if (!PermissionUtils.checkStoragePermission(context, isLegacy)) {
            String msg;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                msg = context.getString(R.string.msg_storage_manager_permission_not_granted);
            } else {
                msg = context.getString(R.string.msg_storage_permission_not_granted);
            }
            Logger.logErrorAndShowToast(context, LOG_TAG, msg);
            return;
        }

        new Thread() {
            public void run() {
                try {
                    Error error;
                    File storageDir = TermuxConstants.TERMUX_STORAGE_HOME_DIR;

                    error = FileUtils.clearDirectory("~/storage", storageDir.getAbsolutePath());
                    if (error != null) {
                        Logger.logErrorAndShowToast(context, LOG_TAG, error.getMessage());
                        Logger.logErrorExtended(LOG_TAG, "Setup Storage Error\n" + error.toString());
                        TermuxCrashUtils.sendCrashReportNotification(context, LOG_TAG, title, null,
                            "## " + title + "\n\n" + Error.getErrorMarkdownString(error),
                            true, false, TermuxUtils.AppInfoMode.TERMUX_PACKAGE, true);
                        return;
                    }

                    Logger.logInfo(LOG_TAG, "Setting up storage symlinks at ~/storage/shared, ~/storage/downloads, ~/storage/dcim, ~/storage/pictures, ~/storage/music and ~/storage/movies for directories in \"" + Environment.getExternalStorageDirectory().getAbsolutePath() + "\".");

                    // Get primary storage root "/storage/emulated/0" symlink
                    File sharedDir = Environment.getExternalStorageDirectory();
                    Os.symlink(sharedDir.getAbsolutePath(), new File(storageDir, "shared").getAbsolutePath());

                    File documentsDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS);
                    Os.symlink(documentsDir.getAbsolutePath(), new File(storageDir, "documents").getAbsolutePath());

                    File downloadsDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS);
                    Os.symlink(downloadsDir.getAbsolutePath(), new File(storageDir, "downloads").getAbsolutePath());

                    File dcimDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DCIM);
                    Os.symlink(dcimDir.getAbsolutePath(), new File(storageDir, "dcim").getAbsolutePath());

                    File picturesDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_PICTURES);
                    Os.symlink(picturesDir.getAbsolutePath(), new File(storageDir, "pictures").getAbsolutePath());

                    File musicDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC);
                    Os.symlink(musicDir.getAbsolutePath(), new File(storageDir, "music").getAbsolutePath());

                    File moviesDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MOVIES);
                    Os.symlink(moviesDir.getAbsolutePath(), new File(storageDir, "movies").getAbsolutePath());

                    File podcastsDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_PODCASTS);
                    Os.symlink(podcastsDir.getAbsolutePath(), new File(storageDir, "podcasts").getAbsolutePath());

                    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
                        File audiobooksDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_AUDIOBOOKS);
                        Os.symlink(audiobooksDir.getAbsolutePath(), new File(storageDir, "audiobooks").getAbsolutePath());
                    }

                    // Dir 0 should ideally be for primary storage
                    // https://cs.android.com/android/platform/superproject/+/android-12.0.0_r32:frameworks/base/core/java/android/app/ContextImpl.java;l=818
                    // https://cs.android.com/android/platform/superproject/+/android-12.0.0_r32:frameworks/base/core/java/android/os/Environment.java;l=219
                    // https://cs.android.com/android/platform/superproject/+/android-12.0.0_r32:frameworks/base/core/java/android/os/Environment.java;l=181
                    // https://cs.android.com/android/platform/superproject/+/android-12.0.0_r32:frameworks/base/services/core/java/com/android/server/StorageManagerService.java;l=3796
                    // https://cs.android.com/android/platform/superproject/+/android-7.0.0_r36:frameworks/base/services/core/java/com/android/server/MountService.java;l=3053

                    // Create "Android/data/com.termux" symlinks
                    File[] dirs = context.getExternalFilesDirs(null);
                    if (dirs != null && dirs.length > 0) {
                        for (int i = 0; i < dirs.length; i++) {
                            File dir = dirs[i];
                            if (dir == null) continue;
                            String symlinkName = "external-" + i;
                            Logger.logInfo(LOG_TAG, "Setting up storage symlinks at ~/storage/" + symlinkName + " for \"" + dir.getAbsolutePath() + "\".");
                            Os.symlink(dir.getAbsolutePath(), new File(storageDir, symlinkName).getAbsolutePath());
                        }
                    }

                    // Create "Android/media/com.termux" symlinks
                    dirs = context.getExternalMediaDirs();
                    if (dirs != null && dirs.length > 0) {
                        for (int i = 0; i < dirs.length; i++) {
                            File dir = dirs[i];
                            if (dir == null) continue;
                            String symlinkName = "media-" + i;
                            Logger.logInfo(LOG_TAG, "Setting up storage symlinks at ~/storage/" + symlinkName + " for \"" + dir.getAbsolutePath() + "\".");
                            Os.symlink(dir.getAbsolutePath(), new File(storageDir, symlinkName).getAbsolutePath());
                        }
                    }

                    Logger.logInfo(LOG_TAG, "Storage symlinks created successfully.");
                } catch (Exception e) {
                    Logger.logErrorAndShowToast(context, LOG_TAG, e.getMessage());
                    Logger.logStackTraceWithMessage(LOG_TAG, "Setup Storage Error: Error setting up link", e);
                    TermuxCrashUtils.sendCrashReportNotification(context, LOG_TAG, title, null,
                        "## " + title + "\n\n" + Logger.getStackTracesMarkdownString(null, Logger.getStackTracesStringArray(e)),
                        true, false, TermuxUtils.AppInfoMode.TERMUX_PACKAGE, true);
                }
            }
        }.start();
    }

    private static Error ensureDirectoryExists(File directory) {
        return FileUtils.createDirectoryFile(directory.getAbsolutePath());
    }

    public static byte[] loadZipBytes() {
        // Only load the shared library when necessary to save memory usage.
        System.loadLibrary("termux-bootstrap");
        return getZip();
    }

    public static native byte[] getZip();

}
