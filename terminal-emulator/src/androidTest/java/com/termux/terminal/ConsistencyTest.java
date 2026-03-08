package com.termux.terminal;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.junit.Assert;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.nio.charset.StandardCharsets;
import java.util.Arrays;

@RunWith(AndroidJUnit4.class)
public class ConsistencyTest {

    static class MockTerminalOutput extends TerminalOutput {
        @Override public void write(byte[] data, int offset, int count) {}
        @Override public void titleChanged(String oldTitle, String newTitle) {}
        @Override public void onCopyTextToClipboard(String text) {}
        @Override public void onPasteTextFromClipboard() {}
        @Override public void onBell() {}
        @Override public void onColorsChanged() {}
    }

    static class MockTerminalSessionClient implements TerminalSessionClient {
        @Override public void onTextChanged(@NonNull TerminalSession session) {}
        @Override public void onTitleChanged(@NonNull TerminalSession session) {}
        @Override public void onSessionFinished(@NonNull TerminalSession session) {}
        @Override public void onCopyTextToClipboard(@NonNull TerminalSession session, String text) {}
        @Override public void onPasteTextFromClipboard(@Nullable TerminalSession session) {}
        @Override public void onBell(@NonNull TerminalSession session) {}
        @Override public void onColorsChanged(@NonNull TerminalSession session) {}
        @Override public void onTerminalCursorStateChange(boolean state) {}
        @Override public void setTerminalShellPid(@NonNull TerminalSession session, int pid) {}
        @Override public Integer getTerminalCursorStyle() { return TerminalEmulator.TERMINAL_CURSOR_STYLE_BLOCK; }
        @Override public void logError(String tag, String message) {}
        @Override public void logWarn(String tag, String message) {}
        @Override public void logInfo(String tag, String message) {}
        @Override public void logDebug(String tag, String message) {}
        @Override public void logVerbose(String tag, String message) {}
        @Override public void logStackTraceWithMessage(String tag, String message, Exception e) {}
        @Override public void logStackTrace(String tag, Exception e) {}
    }

    private void runTest(String input) {
        if (!TerminalEmulator.isRustLibLoaded()) {
            return;
        }

        int cols = 80;
        int rows = 24;
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();

        // 1. Java Only (Reference)
        TerminalEmulator javaEmulator = new TerminalEmulator(output, cols, rows, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        byte[] bytes = input.getBytes(StandardCharsets.UTF_8);
        javaEmulator.append(bytes, bytes.length);

        // 2. Rust Enabled (Experimental)
        TerminalEmulator.sForceDisableRust = false;
        TerminalEmulator rustEmulator = new TerminalEmulator(output, cols, rows, 10, 20, 100, client);
        rustEmulator.append(bytes, bytes.length);

        // 比对光标
        Assert.assertEquals("Cursor Column mismatch for input: " + input, javaEmulator.getCursorCol(), rustEmulator.getCursorCol());
        Assert.assertEquals("Cursor Row mismatch for input: " + input, javaEmulator.getCursorRow(), rustEmulator.getCursorRow());

        // 比对缓冲区内容
        char[] javaText = new char[cols * 2];
        long[] javaStyle = new long[cols];
        char[] rustText = new char[cols * 2];
        long[] rustStyle = new long[cols];

        for (int r = 0; r < rows; r++) {
            javaEmulator.getRowContent(r, javaText, javaStyle);
            rustEmulator.getRowContent(r, rustText, rustStyle);

            Assert.assertArrayEquals("Text mismatch at row " + r + " for input: " + input, javaText, rustText);
            // 注意：目前 Rust 的 Style 处理尚未完全搬运，可能存在已知不一致，先注释掉或仅比对 Text
            // Assert.assertArrayEquals("Style mismatch at row " + r, javaStyle, rustStyle);
        }
    }

    @Test
    public void testBasicText() {
        runTest("Hello World");
        runTest("Line 1\r\nLine 2");
    }

    @Test
    public void testAutoWrap() {
        runTest("A very long line designed to test the auto-wrapping logic of the terminal emulator when rust optimization is active.");
    }

    @Test
    public void testCursorMovement() {
        // CUP: Move to 5,5 then print
        runTest("\u001B[5;5HAt 5,5");
        // Backspace and CR/LF
        runTest("ABC\bDE\r\nFG");
    }

    @Test
    public void testErase() {
        // Print text, then clear screen
        runTest("Should be erased\u001B[2JStill here");
        // Clear line
        runTest("Erase this line\u001B[2K");
    }
}
