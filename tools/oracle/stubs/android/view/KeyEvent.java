package android.view;

/**
 * Compile-only stand-in for the Android framework class, used to compile upstream Termux's
 * `KeyHandler.java` on a plain JVM (ordinary CI and Termux have javac but no Android SDK on
 * the reference build path).
 *
 * Every value below was extracted with `javap -constants` from the platform SDK jar that
 * ships with this SDK installation (`platforms/android-36/android.jar`), so these are the
 * real platform key codes rather than transcribed or invented numbers.
 *
 * Key codes are consulted only on the key-input path; the differential corpus injects byte
 * streams (what a PTY delivers), not key events, so these values do not influence the
 * compared terminal state. They are real values rather than placeholders so that adding
 * key-input corpus entries later does not require re-deriving them.
 *
 * Regenerate with: python3 tools/oracle/gen_keyevent_stub.py
 */
public final class KeyEvent {

    private KeyEvent() {}

    public static final int KEYCODE_BACK = 4;
    public static final int KEYCODE_BREAK = 121;
    public static final int KEYCODE_DEL = 67;
    public static final int KEYCODE_DPAD_CENTER = 23;
    public static final int KEYCODE_DPAD_DOWN = 20;
    public static final int KEYCODE_DPAD_LEFT = 21;
    public static final int KEYCODE_DPAD_RIGHT = 22;
    public static final int KEYCODE_DPAD_UP = 19;
    public static final int KEYCODE_ENTER = 66;
    public static final int KEYCODE_ESCAPE = 111;
    public static final int KEYCODE_F1 = 131;
    public static final int KEYCODE_F10 = 140;
    public static final int KEYCODE_F11 = 141;
    public static final int KEYCODE_F12 = 142;
    public static final int KEYCODE_F2 = 132;
    public static final int KEYCODE_F3 = 133;
    public static final int KEYCODE_F4 = 134;
    public static final int KEYCODE_F5 = 135;
    public static final int KEYCODE_F6 = 136;
    public static final int KEYCODE_F7 = 137;
    public static final int KEYCODE_F8 = 138;
    public static final int KEYCODE_F9 = 139;
    public static final int KEYCODE_FORWARD_DEL = 112;
    public static final int KEYCODE_INSERT = 124;
    public static final int KEYCODE_MOVE_END = 123;
    public static final int KEYCODE_MOVE_HOME = 122;
    public static final int KEYCODE_NUMPAD_0 = 144;
    public static final int KEYCODE_NUMPAD_1 = 145;
    public static final int KEYCODE_NUMPAD_2 = 146;
    public static final int KEYCODE_NUMPAD_3 = 147;
    public static final int KEYCODE_NUMPAD_4 = 148;
    public static final int KEYCODE_NUMPAD_5 = 149;
    public static final int KEYCODE_NUMPAD_6 = 150;
    public static final int KEYCODE_NUMPAD_7 = 151;
    public static final int KEYCODE_NUMPAD_8 = 152;
    public static final int KEYCODE_NUMPAD_9 = 153;
    public static final int KEYCODE_NUMPAD_ADD = 157;
    public static final int KEYCODE_NUMPAD_COMMA = 159;
    public static final int KEYCODE_NUMPAD_DIVIDE = 154;
    public static final int KEYCODE_NUMPAD_DOT = 158;
    public static final int KEYCODE_NUMPAD_ENTER = 160;
    public static final int KEYCODE_NUMPAD_EQUALS = 161;
    public static final int KEYCODE_NUMPAD_MULTIPLY = 155;
    public static final int KEYCODE_NUMPAD_SUBTRACT = 156;
    public static final int KEYCODE_NUM_LOCK = 143;
    public static final int KEYCODE_PAGE_DOWN = 93;
    public static final int KEYCODE_PAGE_UP = 92;
    public static final int KEYCODE_SPACE = 62;
    public static final int KEYCODE_SYSRQ = 120;
    public static final int KEYCODE_TAB = 61;

    public static final int META_SHIFT_ON = 0x1;
    public static final int META_ALT_ON = 0x02;
    public static final int META_CTRL_ON = 0x1000;
    public static final int META_META_ON = 0x10000;
    public static final int META_CAPS_LOCK_ON = 0x100000;
    public static final int META_NUM_LOCK_ON = 0x200000;
    public static final int META_SCROLL_LOCK_ON = 0x400000;
    public static final int META_MODIFIER_MASK = META_SHIFT_ON | META_ALT_ON | META_CTRL_ON
            | META_META_ON | META_CAPS_LOCK_ON | META_NUM_LOCK_ON | META_SCROLL_LOCK_ON;

    public static boolean metaStateHasNoModifiers(int metaState) {
        return (metaState & META_MODIFIER_MASK) == 0;
    }

    public static boolean metaStateHasModifiers(int metaState, int modifiers) {
        if ((modifiers & META_MODIFIER_MASK) == 0) {
            throw new IllegalArgumentException("modifiers must include a modifier");
        }
        return (metaState & modifiers) == modifiers;
    }
}
