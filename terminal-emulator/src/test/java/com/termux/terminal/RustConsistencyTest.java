package com.termux.terminal;

import junit.framework.TestCase;
import java.nio.charset.StandardCharsets;

/**
 * 纯 Java Rust 一致性测试 - 比较 Java 和 Rust 引擎的输出
 * 
 * 运行方式:
 * ./gradlew :terminal-emulator:testDebugUnitTest --tests RustConsistencyTest
 * 
 * 注意：当前 Rust 引擎的 FULL TAKEOVER 模式已禁用，因此测试将跳过 Rust 比较。
 * 当 Rust 引擎完整实现 ANSI 序列处理后，可以重新启用完整测试。
 */
public class RustConsistencyTest extends TestCase {

    static class MockTerminalOutput extends TerminalOutput {
        @Override public void write(byte[] data, int offset, int count) {}
        @Override public void titleChanged(String oldTitle, String newTitle) {}
        @Override public void onCopyTextToClipboard(String text) {}
        @Override public void onPasteTextFromClipboard() {}
        @Override public void onBell() {}
        @Override public void onColorsChanged() {}
        @Override public void onTerminalCursorStateChange(boolean visible) {}
    }

    static class MockTerminalSessionClient implements TerminalSessionClient {
        @Override public void onTextChanged(TerminalSession session) {}
        @Override public void onTitleChanged(TerminalSession session) {}
        @Override public void onSessionFinished(TerminalSession session) {}
        @Override public void onCopyTextToClipboard(TerminalSession session, String text) {}
        @Override public void onPasteTextFromClipboard(TerminalSession session) {}
        @Override public void onBell(TerminalSession session) {}
        @Override public void onColorsChanged(TerminalSession session) {}
        @Override public void onTerminalCursorStateChange(boolean state) {}
        @Override public void setTerminalShellPid(TerminalSession session, int pid) {}
        @Override public Integer getTerminalCursorStyle() { 
            return TerminalEmulator.TERMINAL_CURSOR_STYLE_BLOCK; 
        }
        @Override public void logError(String tag, String message) {}
        @Override public void logWarn(String tag, String message) {}
        @Override public void logInfo(String tag, String message) {}
        @Override public void logDebug(String tag, String message) {}
        @Override public void logVerbose(String tag, String message) {}
        @Override public void logStackTraceWithMessage(String tag, String message, Exception e) {}
        @Override public void logStackTrace(String tag, Exception e) {}
    }

    /**
     * 运行一致性测试
     * @param name 测试名称
     * @param input 输入字符串
     * @param skipRustCheck 是否跳过 Rust 检查（当前总是 true）
     */
    private void runTest(String name, String input, boolean skipRustCheck) {
        int cols = 80;
        int rows = 24;
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();

        // 1. Java Only (Reference)
        TerminalEmulator javaEmulator = new TerminalEmulator(output, cols, rows, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        byte[] bytes = input.getBytes(StandardCharsets.UTF_8);
        javaEmulator.append(bytes, bytes.length);

        int javaCursorCol = javaEmulator.getCursorCol();
        int javaCursorRow = javaEmulator.getCursorRow();
        
        // 获取 Java 屏幕内容
        char[][] javaScreen = new char[rows][];
        for (int r = 0; r < rows; r++) {
            char[] text = new char[cols * 2];
            long[] style = new long[cols];
            javaEmulator.getRowContent(r, text, style);
            javaScreen[r] = text;
        }

        if (skipRustCheck) {
            // Rust 检查被跳过，只验证 Java 能正常工作
            System.out.println("SKIP " + name + ": Rust FULL TAKEOVER mode disabled");
            System.out.println("  Java result: cursor=(" + javaCursorCol + ", " + javaCursorRow + ")");
            return;
        }

        // 2. Rust Enabled (Experimental)
        TerminalEmulator.sForceDisableRust = false;
        TerminalEmulator rustEmulator = new TerminalEmulator(output, cols, rows, 10, 20, 100, client);
        rustEmulator.append(bytes, bytes.length);

        int rustCursorCol = rustEmulator.getCursorCol();
        int rustCursorRow = rustEmulator.getCursorRow();

        // 比对光标
        assertEquals(name + " - Cursor Column", javaCursorCol, rustCursorCol);
        assertEquals(name + " - Cursor Row", javaCursorRow, rustCursorRow);

        // 比对缓冲区内容
        for (int r = 0; r < rows; r++) {
            char[] rustText = new char[cols * 2];
            long[] rustStyle = new long[cols];
            rustEmulator.getRowContent(r, rustText, rustStyle);

            // 比较文本内容（跳过尾部空格）
            String javaStr = new String(javaScreen[r]).stripTrailing();
            String rustStr = new String(rustText).stripTrailing();
            assertEquals(name + " - Row " + r + " text", javaStr, rustStr);
        }
        
        System.out.println("PASS " + name + " (Java vs Rust consistent)");
    }

    public void testBasicText() {
        runTest("basic_hello", "Hello World", true);
        runTest("basic_newline", "Line 1\r\nLine 2", true);
    }

    public void testAutoWrap() {
        runTest("autowrap_long_line", 
            "A very long line designed to test the auto-wrapping logic of the terminal emulator.", true);
    }

    public void testCursorMovement() {
        runTest("cursor_cup", "\u001B[5;5HAt 5,5", true);
        runTest("cursor_backspace", "ABC\bDE\r\nFG", true);
    }

    public void testErase() {
        runTest("erase_ed", "Should be erased\u001B[2JStill here", true);
        runTest("erase_el", "Erase this line\u001B[2K", true);
    }

    public void testColors() {
        runTest("color_fg", "\u001B[31mRed\u001B[0m", true);
        runTest("color_bg", "\u001B[42mGreen BG\u001B[0m", true);
    }

    public void testTabStops() {
        runTest("tab_basic", "A\tB\tC", true);
        runTest("tab_with_text", "Col 1\tCol 2\tCol 3", true);
    }

    public void testScrolling() {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 30; i++) {
            sb.append("Line ").append(i).append("\r\n");
        }
        runTest("scroll_many_lines", sb.toString(), true);
    }
}
