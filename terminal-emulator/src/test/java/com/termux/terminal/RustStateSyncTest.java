package com.termux.terminal;

import org.junit.Test;
import static org.junit.Assert.*;

/**
 * Test for Rust state synchronization - verifies that Java/Rust state
 * inconsistency does not cause crashes.
 */
public class RustStateSyncTest {

    @Test
    public void testResizeWithRustStateSync() {
        // Test that resize operations don't cause state inconsistency
        int columns = 80;
        int rows = 24;
        int totalRows = 100;
        
        TerminalBuffer buffer = new TerminalBuffer(columns, totalRows, rows);
        TerminalEmulator emulator = createEmulator(buffer, columns, rows);
        
        // Simulate rapid resize operations
        for (int i = 0; i < 10; i++) {
            int newCols = 80 + (i % 20);  // Vary between 80-99 columns
            int newRows = 24 + (i % 5);   // Vary between 24-28 rows
            emulator.resize(newCols, newRows, 10, 20);
            
            // Verify state is consistent
            assertEquals(newCols, emulator.getCols());
            assertEquals(newRows, emulator.getRows());
        }
    }

    @Test
    public void testSyncScreenBatchFromRustWithVaryingSizes() {
        // Test syncScreenBatchFromRust with various screen sizes
        int[] testCols = {80, 85, 90, 100, 120};
        int[] testRows = {24, 30, 40, 50};
        
        for (int cols : testCols) {
            for (int rows : testRows) {
                TerminalBuffer buffer = new TerminalBuffer(cols, 100, rows);
                TerminalEmulator emulator = createEmulator(buffer, cols, rows);
                
                // Fill screen with text
                StringBuilder sb = new StringBuilder();
                for (int c = 0; c < cols; c++) {
                    sb.append('A');
                }
                for (int r = 0; r < rows; r++) {
                    emulator.append(sb.toString().getBytes(), cols);
                    emulator.append(new byte[]{'\r', '\n'}, 2);
                }
                
                // Sync from Rust
                emulator.syncScreenBatchFromRust(0, rows);
                
                // Verify no crash and state is consistent
                assertEquals(cols, emulator.getCols());
                assertEquals(rows, emulator.getRows());
            }
        }
    }

    @Test
    public void testConcurrentResizeAndSync() {
        // Test that resize and sync operations can interleave safely
        int columns = 80;
        int rows = 24;
        
        TerminalBuffer buffer = new TerminalBuffer(columns, 100, rows);
        TerminalEmulator emulator = createEmulator(buffer, columns, rows);
        
        // Fill screen
        for (int r = 0; r < rows; r++) {
            StringBuilder sb = new StringBuilder();
            for (int c = 0; c < columns; c++) {
                sb.append((char)('A' + (r % 26)));
            }
            emulator.append(sb.toString().getBytes(), columns);
            emulator.append(new byte[]{'\r', '\n'}, 2);
        }
        
        // Interleave resize and sync
        for (int i = 0; i < 5; i++) {
            // Resize
            emulator.resize(columns + 5, rows + 2, 10, 20);
            columns += 5;
            rows += 2;
            
            // Sync
            emulator.syncScreenBatchFromRust(0, rows);
            
            // Verify
            assertEquals(columns, emulator.getCols());
            assertEquals(rows, emulator.getRows());
        }
    }

    @Test
    public void testBoundaryConditions() {
        // Test minimum and maximum sizes
        int[] minCols = {1, 2, 4};
        int[] minRows = {1, 2, 4};
        int[] maxCols = {500, 1000};
        int[] maxRows = {100, 200};
        
        for (int cols : minCols) {
            for (int rows : minRows) {
                TerminalBuffer buffer = new TerminalBuffer(cols, 10, rows);
                TerminalEmulator emulator = createEmulator(buffer, cols, rows);
                emulator.syncScreenBatchFromRust(0, rows);
                assertEquals(cols, emulator.getCols());
                assertEquals(rows, emulator.getRows());
            }
        }
        
        for (int cols : maxCols) {
            for (int rows : maxRows) {
                TerminalBuffer buffer = new TerminalBuffer(cols, rows, rows);
                TerminalEmulator emulator = createEmulator(buffer, cols, rows);
                emulator.syncScreenBatchFromRust(0, rows);
                assertEquals(cols, emulator.getCols());
                assertEquals(rows, emulator.getRows());
            }
        }
    }

    @Test
    public void testAlternateBufferResize() {
        // Test resize with alternate buffer active
        int columns = 80;
        int rows = 24;
        
        TerminalBuffer buffer = new TerminalBuffer(columns, 100, rows);
        TerminalEmulator emulator = createEmulator(buffer, columns, rows);
        
        // Switch to alternate buffer
        emulator.append("\033[?1049h".getBytes(), 7);
        
        // Resize while in alternate buffer
        for (int i = 0; i < 5; i++) {
            emulator.resize(columns + 5, rows, 10, 20);
            columns += 5;
            emulator.syncScreenBatchFromRust(0, rows);
        }
        
        // Switch back to main buffer
        emulator.append("\033[?1049l".getBytes(), 7);
        emulator.syncScreenBatchFromRust(0, rows);
        
        // Verify
        assertEquals(columns, emulator.getCols());
        assertEquals(rows, emulator.getRows());
    }

    @Test
    public void testWideCharacterResize() {
        // Test resize with wide characters (CJK)
        int columns = 80;
        int rows = 24;
        
        TerminalBuffer buffer = new TerminalBuffer(columns, 100, rows);
        TerminalEmulator emulator = createEmulator(buffer, columns, rows);
        
        // Write wide characters
        String wideChars = "你好世界こんにちは안녕하세요";
        for (int r = 0; r < rows; r++) {
            emulator.append(wideChars.getBytes(), wideChars.length());
            emulator.append(new byte[]{'\r', '\n'}, 2);
        }
        
        // Resize
        emulator.resize(100, 30, 10, 20);
        emulator.syncScreenBatchFromRust(0, 30);
        
        // Verify no crash
        assertEquals(100, emulator.getCols());
        assertEquals(30, emulator.getRows());
    }

    @Test
    public void testEmojiResize() {
        // Test resize with emoji (may be wide or combining)
        int columns = 80;
        int rows = 24;
        
        TerminalBuffer buffer = new TerminalBuffer(columns, 100, rows);
        TerminalEmulator emulator = createEmulator(buffer, columns, rows);
        
        // Write emoji
        String emoji = "😀😃😄😁😆😅😂🤣🥲☺️😊";
        for (int r = 0; r < rows; r++) {
            emulator.append(emoji.getBytes(), emoji.length());
            emulator.append(new byte[]{'\r', '\n'}, 2);
        }
        
        // Resize
        emulator.resize(100, 30, 10, 20);
        emulator.syncScreenBatchFromRust(0, 30);
        
        // Verify no crash
        assertEquals(100, emulator.getCols());
        assertEquals(30, emulator.getRows());
    }

    @Test
    public void testRapidResizeStress() {
        // Stress test with rapid resize operations
        int columns = 80;
        int rows = 24;
        
        TerminalBuffer buffer = new TerminalBuffer(columns, 100, rows);
        TerminalEmulator emulator = createEmulator(buffer, columns, rows);
        
        // Rapid resize cycle
        for (int i = 0; i < 100; i++) {
            int newCols = 70 + (i % 40);  // 70-109
            int newRows = 20 + (i % 15);  // 20-34
            emulator.resize(newCols, newRows, 10, 20);
            
            if (i % 10 == 0) {
                emulator.syncScreenBatchFromRust(0, newRows);
            }
        }
        
        // Final verification
        assertTrue(emulator.getCols() >= 70 && emulator.getCols() <= 110);
        assertTrue(emulator.getRows() >= 20 && emulator.getRows() <= 35);
    }

    // Helper method to create emulator
    private TerminalEmulator createEmulator(TerminalBuffer buffer, int cols, int rows) {
        // Create a minimal emulator for testing
        TerminalOutput output = new TerminalOutput() {
            @Override
            public void write(byte[] data, int offset, int count) {}
            @Override
            public void titleChanged(String oldTitle, String newTitle) {}
            @Override
            public void onCopyTextToClipboard(String text) {}
            @Override
            public void onPasteTextFromClipboard() {}
            @Override
            public void onBell() {}
            @Override
            public void onColorsChanged() {}
        };
        
        TerminalSessionClient client = new TerminalSessionClient() {
            @Override
            public void onTextChanged(TerminalSession session) {}
            @Override
            public void onTitleChanged(TerminalSession session) {}
            @Override
            public void onSessionFinished(TerminalSession session) {}
            @Override
            public void onCopyTextToClipboard(TerminalSession session, String text) {}
            @Override
            public void onPasteTextFromClipboard(TerminalSession session) {}
            @Override
            public void onBell(TerminalSession session) {}
            @Override
            public void onColorsChanged(TerminalSession session) {}
            @Override
            public void onTerminalCursorStateChange(boolean state) {}
            @Override
            public void setTerminalShellPid(TerminalSession session, int pid) {}
            @Override
            public Integer getTerminalCursorStyle() { return TerminalEmulator.TERMINAL_CURSOR_STYLE_BLOCK; }
            @Override
            public void logError(String tag, String message) {}
            @Override
            public void logWarn(String tag, String message) {}
            @Override
            public void logInfo(String tag, String message) {}
            @Override
            public void logDebug(String tag, String message) {}
            @Override
            public void logVerbose(String tag, String message) {}
            @Override
            public void logStackTraceWithMessage(String tag, String message, Exception e) {}
            @Override
            public void logStackTrace(String tag, Exception e) {}
        };
        
        return new TerminalEmulator(output, cols, rows, 10, 20, 100, client);
    }
}
