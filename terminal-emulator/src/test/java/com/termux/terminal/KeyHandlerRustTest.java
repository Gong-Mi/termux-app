package com.termux.terminal;

import org.junit.BeforeClass;
import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

/**
 * JNI preservation tests for the six groups originally printed by this class.
 *
 * Golden strings were checked against KeyHandler.java at df17f03e^ and the
 * current Rust terminal/key_handler.rs. JNI.kt delegates directly to that Rust
 * implementation; expected values must not be calculated through the same JNI.
 */
public class KeyHandlerRustTest {

    // Android key codes and the terminal's modifier ABI, not Android meta-state bits.
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

    @BeforeClass
    public static void requireNativeLibrary() {
        // Missing/wrong-ABI libraries are failures, never assumptions or skips.
        assertTrue("KeyHandler tests require the real termux_rust JNI library",
            JNI.sNativeLibrariesLoaded);
    }

    @Test
    public void arrowKeysWithoutModifiers() {
        assertKey("UP", "\u001b[A", KEYCODE_DPAD_UP, 0);
        assertKey("DOWN", "\u001b[B", KEYCODE_DPAD_DOWN, 0);
        assertKey("LEFT", "\u001b[D", KEYCODE_DPAD_LEFT, 0);
        assertKey("RIGHT", "\u001b[C", KEYCODE_DPAD_RIGHT, 0);
    }

    @Test
    public void arrowKeysWithModifiers() {
        assertKey("UP+Shift", "\u001b[1;2A", KEYCODE_DPAD_UP, KEYMOD_SHIFT);
        assertKey("UP+Ctrl", "\u001b[1;5A", KEYCODE_DPAD_UP, KEYMOD_CTRL);
        assertKey("UP+Alt", "\u001b[1;3A", KEYCODE_DPAD_UP, KEYMOD_ALT);
        assertKey("UP+Ctrl+Shift", "\u001b[1;6A", KEYCODE_DPAD_UP, KEYMOD_CTRL | KEYMOD_SHIFT);
    }

    @Test
    public void functionKeys() {
        assertKey("F1", "\u001bOP", KEYCODE_F1, 0);
        assertKey("F2", "\u001bOQ", KEYCODE_F2, 0);
        assertKey("F12", "\u001b[24~", KEYCODE_F12, 0);
    }

    @Test
    public void specialKeys() {
        assertKey("Delete", "\u001b[3~", KEYCODE_FORWARD_DEL, 0);
        assertKey("Insert", "\u001b[2~", KEYCODE_INSERT, 0);
        assertKey("PageUp", "\u001b[5~", KEYCODE_PAGE_UP, 0);
        assertKey("PageDown", "\u001b[6~", KEYCODE_PAGE_DOWN, 0);
    }

    @Test
    public void termcapMappings() {
        assertEquals("k1 (F1)", "\u001bOP", JNI.getKeyCodeFromTermcap("k1", false, false));
        assertEquals("kd (down)", "\u001b[B", JNI.getKeyCodeFromTermcap("kd", false, false));
        // Backspace is DEL (0x7f), not BS (0x08), without Ctrl.
        assertEquals("kb (backspace)", "\u007f", JNI.getKeyCodeFromTermcap("kb", false, false));
    }

    @Test
    public void cursorApplicationMode() {
        assertEquals("UP (app mode)", "\u001bOA", JNI.getKeyCode(KEYCODE_DPAD_UP, 0, true, false));
    }

    private static void assertKey(String name, String expected, int keyCode, int keyMod) {
        assertEquals(name, expected, JNI.getKeyCode(keyCode, keyMod, false, false));
    }
}
