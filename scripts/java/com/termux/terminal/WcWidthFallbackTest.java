package com.termux.terminal;

import junit.framework.TestCase;

/** Characterizes the limited Kotlin heuristic, NOT the Unicode/JNI contract.
 * Run in its own JVM without the native library; see verify-wcwidth-jni.py.
 */
public class WcWidthFallbackTest extends TestCase {

    @Override
    protected void setUp() {
        assertFalse("WcWidth fallback tests require an isolated JVM without JNI",
            JNI.sNativeLibrariesLoaded);
        try {
            WcWidth.widthRust('A');
            fail("Native widthRust unexpectedly resolved in the fallback JVM");
        } catch (UnsatisfiedLinkError expected) {
            // Prove absence, rather than setting or mocking the loaded flag.
        }
    }

    public void testControlsAndAscii() {
        assertEquals(0, WcWidth.width(0));
        assertEquals(0, WcWidth.width(31));
        assertEquals(0, WcWidth.width(0x7F));
        assertEquals(0, WcWidth.width(0x9F));
        for (int i = 0x20; i <= 0x7E; i++) assertEquals(1, WcWidth.width(i));
    }

    public void testBasicWideRanges() {
        assertEquals(2, WcWidth.width('中'));
        assertEquals(2, WcWidth.width('Ａ'));
        assertEquals(2, WcWidth.width(0x3000));
        assertEquals(2, WcWidth.width(0x3400));
        assertEquals(2, WcWidth.width(0xF900));
    }

    public void testHeuristicLimitationsAreNotUnicodeExpectations() {
        // These values deliberately differ from the untouched nine JNI contracts.
        // They document fallback behavior only, not correct Unicode widths.
        assertEquals(1, WcWidth.width(0x0302));
        assertEquals(1, WcWidth.width(0xFE0F));
        assertEquals(1, WcWidth.width(0x2060));
        assertEquals(1, WcWidth.width(0x2070E));
        assertEquals(1, WcWidth.width(0x1F428));
    }
}
