package com.termux.terminal;

/**
 * Test class for Rust-based KeyHandler
 */
public class KeyHandlerTest {

    static {
        if (!JNI.sNativeLibrariesLoaded) {
            System.err.println("Native libraries not loaded!");
        }
    }

    public static void main(String[] args) {
        System.out.println("=== KeyHandler Rust Implementation Test ===\n");

        // Test 1: Basic arrow keys
        System.out.println("Test 1: Arrow keys (no modifiers)");
        testKey("UP", KEYCODE_DPAD_UP, 0);
        testKey("DOWN", KEYCODE_DPAD_DOWN, 0);
        testKey("LEFT", KEYCODE_DPAD_LEFT, 0);
        testKey("RIGHT", KEYCODE_DPAD_RIGHT, 0);

        // Test 2: Arrow keys with modifiers
        System.out.println("\nTest 2: Arrow keys with modifiers");
        testKey("UP+Shift", KEYCODE_DPAD_UP, KEYMOD_SHIFT);
        testKey("UP+Ctrl", KEYCODE_DPAD_UP, KEYMOD_CTRL);
        testKey("UP+Alt", KEYCODE_DPAD_UP, KEYMOD_ALT);
        testKey("UP+Ctrl+Shift", KEYCODE_DPAD_UP, KEYMOD_CTRL | KEYMOD_SHIFT);

        // Test 3: Function keys
        System.out.println("\nTest 3: Function keys");
        testKey("F1", KEYCODE_F1, 0);
        testKey("F2", KEYCODE_F2, 0);
        testKey("F12", KEYCODE_F12, 0);

        // Test 4: Special keys
        System.out.println("\nTest 4: Special keys");
        testKey("Delete", KEYCODE_FORWARD_DEL, 0);
        testKey("Insert", KEYCODE_INSERT, 0);
        testKey("PageUp", KEYCODE_PAGE_UP, 0);
        testKey("PageDown", KEYCODE_PAGE_DOWN, 0);

        // Test 5: Termcap
        System.out.println("\nTest 5: Termcap mappings");
        testTermcap("k1 (F1)", "k1");
        testTermcap("kd (down)", "kd");
        testTermcap("kb (backspace)", "kb");

        // Test 6: Cursor application mode
        System.out.println("\nTest 6: Cursor application mode");
        testKey("UP (app mode)", KEYCODE_DPAD_UP, 0, true);

        System.out.println("\n=== All tests completed ===");
    }

    private static final int KEYMOD_SHIFT = 0x20000000;
    private static final int KEYMOD_CTRL = 0x40000000;
    private static final int KEYMOD_ALT = 0x80000000;

    private static final int KEYCODE_DPAD_UP = 19;
    private static final int KEYCODE_DPAD_DOWN = 20;
    private static final int KEYCODE_DPAD_LEFT = 21;
    private static final int KEYCODE_DPAD_RIGHT = 22;
    private static final int KEYCODE_F1 = 131;
    private static final int KEYCODE_F2 = 132;
    private static final int KEYCODE_F12 = 142;
    private static final int KEYCODE_FORWARD_DEL = 112;
    private static final int KEYCODE_INSERT = 124;
    private static final int KEYCODE_PAGE_UP = 92;
    private static final int KEYCODE_PAGE_DOWN = 93;

    private static void testKey(String name, int keyCode, int keyMod) {
        testKey(name, keyCode, keyMod, false);
    }

    private static void testKey(String name, int keyCode, int keyMod, boolean cursorApp) {
        String result = JNI.getKeyCode(keyCode, keyMod, cursorApp, false);
        System.out.printf("  %-20s: %s\n", name, formatEscape(result));
    }

    private static void testTermcap(String name, String termcap) {
        String result = JNI.getKeyCodeFromTermcap(termcap, false, false);
        System.out.printf("  %-20s: %s\n", name, formatEscape(result));
    }

    private static String formatEscape(String s) {
        if (s == null) return "null";
        StringBuilder sb = new StringBuilder();
        for (char c : s.toCharArray()) {
            if (c == 0x1b) {
                sb.append("\\x1b");
            } else if (c == '\r') {
                sb.append("\\r");
            } else if (c == '\n') {
                sb.append("\\n");
            } else if (c == '\t') {
                sb.append("\\t");
            } else if (c < 32) {
                sb.append(String.format("\\x%02x", (int) c));
            } else {
                sb.append(c);
            }
        }
        return sb.toString();
    }
}
