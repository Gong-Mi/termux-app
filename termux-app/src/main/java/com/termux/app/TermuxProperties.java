package com.termux.app;

import android.util.Log;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.util.Locale;
import java.util.Properties;

public final class TermuxProperties {

    private final Properties properties = new Properties();

    public static final String EXTRA_KEYS_DEFAULT = "[['ESC',{key: 'DRAWER', popup: 'PASTE'},'SCROLL','HOME','UP','END','PGUP'], ['TAB','CTRL','ALT','LEFT','DOWN','RIGHT','PGDN']]";
    public static final String EXTRA_KEYS_STYLE_DEFAULT = "default";

    public TermuxProperties() {
        reloadProperties();
    }

    void reloadProperties(TermuxActivity activity) {
        reloadProperties();
    }

    private void reloadProperties() {
        properties.clear();
        try {
            for (String subPath : new String[]{".termux/termux.properties", ".config/termux/termux.properties"}) {
                File propertiesFile = new File(TermuxConstants.HOME_PATH + '/' + subPath);
                if (propertiesFile.isFile()) {
                    try (FileInputStream in = new FileInputStream(propertiesFile)) {
                        try {
                            properties.load(in);
                        } catch (Exception e) {
                            Log.e(TermuxConstants.LOG_TAG, "Error reading termux properties", e);
                        }
                    }
                }
            }
        } catch (IOException e) {
            Log.e(TermuxConstants.LOG_TAG, "Failed to reload properties", e);
        }
    }

    boolean isBackKeyTheEscapeKey() {
        return properties.getProperty("back-key", "back").equalsIgnoreCase("escape");
    }

    boolean isEnforcingCharBasedInput() {
        return properties.getProperty("enforce-char-based-input", "false").equalsIgnoreCase("true");
    }

    boolean areVirtualVolumeKeysDisabled() {
        return properties.getProperty("volume-keys", "normal").equalsIgnoreCase("volume");
    }

    public String getExtraKeys() {
        return properties.getProperty("extra-keys", EXTRA_KEYS_DEFAULT);
    }

    public String getExtraKeysStyle() {
        return properties.getProperty("extra-keys-style", EXTRA_KEYS_STYLE_DEFAULT);
    }

    public boolean areHardwareKeyboardShortcutsDisabled() {
        return properties.getProperty("disable-hardware-keyboard-shortcuts", "false").equalsIgnoreCase("true");
    }

    public boolean shouldOpenTerminalTranscriptURLOnClick() {
        return properties.getProperty("terminal-transcript-url-click", "true").equalsIgnoreCase("true");
    }

    public boolean shouldAutoCloseSessionOnExit() {
        return properties.getProperty("close-session-on-exit", "false").equalsIgnoreCase("true");
    }

    public boolean isUsingCtrlSpaceWorkaround() {
        return properties.getProperty("ctrl-space-workaround", "false").equalsIgnoreCase("true");
    }

    public enum BellBehaviour {
        VIBRATE, BEEP, IGNORE
    }

    public BellBehaviour getBellBehaviour() {
        var prop = properties.getProperty("bell-character", "vibrate").trim().toLowerCase(Locale.ROOT);
        return switch (prop) {
            case "vibrate" -> BellBehaviour.VIBRATE;
            case "beep" -> BellBehaviour.BEEP;
            case "ignore" -> BellBehaviour.IGNORE;
            default -> {
                Log.w(TermuxConstants.LOG_TAG, "Invalid 'bell-character' value: '" + prop + "'");
                yield BellBehaviour.VIBRATE;
            }
        };
    }

    public int getTerminalTranscriptRows() {
        String value = properties.getProperty("terminal-transcript-rows");
        if (value != null) {
            try {
                int rows = Integer.parseInt(value.trim());
                return Math.max(100, Math.min(rows, 200000));
            } catch (NumberFormatException e) {
                Log.w(TermuxConstants.LOG_TAG, "Invalid 'terminal-transcript-rows' value: '" + value + "', using default");
            }
        }
        return 2000;
    }

}
