package com.termux.app;

import android.annotation.SuppressLint;

public class TermuxConstants {

    public static final String LOG_TAG = "termux";

    public static final String PACKAGE_NAME = "com.termux";

    @SuppressLint("SdCardPath")
    public static String FILES_PATH = "/data/data/" + PACKAGE_NAME + "/files";
    public static String PREFIX_PATH = FILES_PATH + "/usr";
    public static String BIN_PATH = PREFIX_PATH + "/bin";
    public static String HOME_PATH = FILES_PATH + "/home";
    public static String APP_LIB_PATH = FILES_PATH + "/applib";
    public static String EXEC_PATH = FILES_PATH + "/exec";

    public static void init(android.content.Context context) {
        FILES_PATH = context.getFilesDir().getAbsolutePath();
        PREFIX_PATH = FILES_PATH + "/usr";
        BIN_PATH = PREFIX_PATH + "/bin";
        HOME_PATH = FILES_PATH + "/home";
        APP_LIB_PATH = FILES_PATH + "/applib";
        EXEC_PATH = FILES_PATH + "/exec";
        FONT_PATH = HOME_PATH + "/.termux/font.ttf";
        COLORS_PATH = HOME_PATH + "/.termux/colors.properties";
    }

    public static String FONT_PATH = TermuxConstants.HOME_PATH + "/.termux/font.ttf";
    public static String COLORS_PATH = TermuxConstants.HOME_PATH + "/.termux/colors.properties";

    public static final int TERMUX_APP_NOTIFICATION_ID = 1337;

    public static final String TERMUX_INTERNAL_ACTIVITY = PACKAGE_NAME + ".app.TermuxActivityInternal";

}
