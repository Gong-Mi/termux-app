package com.termux.terminal;

import junit.framework.TestCase;

public class CrashReproductionTest extends TestCase {
    public void testCrashOnResizeWithWideChars() {
        int columns = 92;
        int rows = 24;
        TerminalBuffer buffer = new TerminalBuffer(columns, 100, rows);
        
        // Simulate sync from Rust with wide characters
        // We fill 92 chars, but they are wide.
        char[] text = new char[columns];
        long[] styles = new long[columns];
        for (int i = 0; i < columns; i++) {
            text[i] = '测'; // A wide character (width 2)
            styles[i] = 0;
        }
        
        // row 0 is already allocated in constructor for rows > 0
        TerminalRow row = buffer.allocateFullLineIfNecessary(buffer.externalToInternalRow(0));
        row.setTextAndStyles(text, styles);
        
        // Now resize. This should trigger the reflow logic in TerminalBuffer.resize
        // which will iterate over mText and call getStyle(currentOldCol).
        // Since all chars are width 2, currentOldCol will reach 92 quickly.
        int[] cursor = {0, 0};
        buffer.resize(columns + 10, rows, 100, cursor, 0, false);
    }
}
