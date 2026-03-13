package com.termux.terminal;

import java.nio.charset.StandardCharsets;
import java.util.Random;

/**
 * Java vs Rust 性能对比测试
 * 
 * 输出格式设计为可被脚本解析，便于 CI 自动生成对比报告
 * 
 * 运行方式：
 * ./gradlew :terminal-emulator:test --tests com.termux.terminal.JavaRustPerformanceComparisonTest --info
 */
public class JavaRustPerformanceComparisonTest extends TerminalTestCase {

    private static final int COLS = 80;
    private static final int ROWS = 24;
    
    // 测试数据大小（MB）
    private static final int RAW_TEXT_SIZE_MB = 5;
    private static final int ANSI_TEXT_SIZE_MB = 1;
    
    // 测试迭代次数
    private static final int ANSI_ITERATIONS = 3;
    private static final int CURSOR_ITERATIONS = 20000;
    private static final int SCROLL_LINES = 10000;

    /**
     * 生成与 Rust 测试相同的随机数据（使用相同 seed）
     * 使用位运算模拟 u64 乘法，与 Rust 的 wrapping_mul 行为一致
     */
    private byte[] generateRandomAscii(int size) {
        byte[] data = new byte[size];
        long seed = 42L;
        
        for (int i = 0; i < size; i++) {
            // 模拟 u64 wrapping_mul: (seed * 6364136223846793005) + 1
            seed = multiplyUnsigned(seed, 6364136223846793005L) + 1L;
            byte b = (byte) (seed & 0xFF);
            // 确保是可打印 ASCII
            if (b >= 32 && b <= 126) {
                data[i] = b;
            } else {
                data[i] = (byte) 'A';
            }
        }
        return data;
    }

    /**
     * 生成与 Rust 测试相同的 ANSI 数据（使用相同 seed）
     */
    private byte[] generateAnsiData(int size) {
        StringBuilder sb = new StringBuilder();
        long seed = 42L;
        
        while (sb.length() < size) {
            seed = multiplyUnsigned(seed, 6364136223846793005L) + 1L;
            int seqType = (int) (seed % 5);
            
            switch (seqType) {
                case 0: sb.append("\u001b[31m"); break; // 红色
                case 1: sb.append("\u001b[32m"); break; // 绿色
                case 2: sb.append("\u001b[H"); break;   // 光标归位
                case 3: sb.append("\u001b[2J"); break;  // 清屏
                default: sb.append("Hello Performance Test "); break;
            }
        }
        
        byte[] result = sb.toString().getBytes(StandardCharsets.UTF_8);
        if (result.length > size) {
            byte[] truncated = new byte[size];
            System.arraycopy(result, 0, truncated, 0, size);
            return truncated;
        }
        return result;
    }

    /**
     * 输出性能结果（统一格式）
     */
    private void printResult(String testName, double value, String unit, double javaValue) {
        String marker = "JAVA";
        String speedup = "";
        
        if (javaValue > 0) {
            double ratio = value / javaValue;
            speedup = String.format(" (Rust speedup: %.2fx)", ratio);
            marker = "RUST";
        }
        
        System.out.printf("[%s] %s: %.2f %s%s%n", marker, testName, value, unit, speedup);
    }

    // =========================================================================
    // Raw Text 性能测试
    // =========================================================================

    public void testRawTextPerformance() {
        withTerminalSized(COLS, ROWS);
        byte[] rawData = generateRandomAscii(RAW_TEXT_SIZE_MB * 1024 * 1024);

        long start = System.nanoTime();
        mTerminal.append(rawData, rawData.length);
        long end = System.nanoTime();

        double durationSeconds = (end - start) / 1_000_000_000.0;
        double speedMBps = RAW_TEXT_SIZE_MB / durationSeconds;

        printResult("Raw Text Throughput", speedMBps, "MB/s", 0);
        System.out.printf("JAVA_RAW_TEXT_MBPS=%.2f%n", speedMBps);
    }

    // =========================================================================
    // ANSI Escape 性能测试
    // =========================================================================

    public void testAnsiEscapePerformance() {
        withTerminalSized(COLS, ROWS);
        byte[] ansiData = generateAnsiData(ANSI_TEXT_SIZE_MB * 1024 * 1024);

        long start = System.nanoTime();
        for (int i = 0; i < ANSI_ITERATIONS; i++) {
            mTerminal.append(ansiData, ansiData.length);
        }
        long end = System.nanoTime();

        double totalProcessedMB = (ansiData.length * ANSI_ITERATIONS) / (1024.0 * 1024.0);
        double durationSeconds = (end - start) / 1_000_000_000.0;
        double speedMBps = totalProcessedMB / durationSeconds;

        printResult("ANSI Escape Throughput", speedMBps, "MB/s", 0);
        System.out.printf("JAVA_ANSI_MBPS=%.2f%n", speedMBps);
    }

    // =========================================================================
    // 光标移动性能测试
    // =========================================================================

    public void testCursorPositionPerformance() {
        withTerminalSized(COLS, ROWS);
        
        // 光标移动序列
        byte[] movements = "\u001b[5;10H\u001b[10;20H\u001b[15;30H\u001b[20;40H\u001b[1;1H".getBytes(StandardCharsets.UTF_8);

        long start = System.nanoTime();
        for (int i = 0; i < CURSOR_ITERATIONS; i++) {
            mTerminal.append(movements, movements.length);
        }
        long end = System.nanoTime();

        double durationSeconds = (end - start) / 1_000_000_000.0;
        double opsPerSec = CURSOR_ITERATIONS / durationSeconds;

        printResult("Cursor Movement", opsPerSec, "ops/s", 0);
        System.out.printf("JAVA_CURSOR_OPS=%.0f%n", opsPerSec);
    }

    // =========================================================================
    // 滚动性能测试
    // =========================================================================

    public void testScrollingPerformance() {
        withTerminalSized(COLS, ROWS);

        // 生成 SCROLL_LINES 行文本
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < SCROLL_LINES; i++) {
            sb.append("Line ").append(i).append("\r\n");
        }
        byte[] scrollData = sb.toString().getBytes(StandardCharsets.UTF_8);

        long start = System.nanoTime();
        mTerminal.append(scrollData, scrollData.length);
        long end = System.nanoTime();

        double durationSeconds = (end - start) / 1_000_000_000.0;
        double linesPerSec = SCROLL_LINES / durationSeconds;

        printResult("Scrolling", linesPerSec, "lines/s", 0);
        System.out.printf("JAVA_SCROLL_LINES=%.0f%n", linesPerSec);
    }

    // =========================================================================
    // 宽字符（中文）性能测试
    // =========================================================================

    public void testWideCharPerformance() {
        withTerminalSized(COLS, ROWS);

        // 中文字符串（每个字符占 2 列）
        String chineseText = "你好世界 ".repeat(100000); // 500,000 字符
        byte[] data = chineseText.getBytes(StandardCharsets.UTF_8);

        long start = System.nanoTime();
        mTerminal.append(data, data.length);
        long end = System.nanoTime();

        double durationSeconds = (end - start) / 1_000_000_000.0;
        double charsPerSec = 500000.0 / durationSeconds;

        printResult("Wide Char Processing", charsPerSec, "chars/s", 0);
        System.out.printf("JAVA_WIDECHAR_OPS=%.0f%n", charsPerSec);
    }

    // =========================================================================
    // 小批量高频调用性能测试
    // =========================================================================

    public void testSmallBatchPerformance() {
        withTerminalSized(COLS, ROWS);
        
        byte[] smallBatch = "Hello World\r\n".getBytes(StandardCharsets.UTF_8);
        int iterations = 100000;

        long start = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            mTerminal.append(smallBatch, smallBatch.length);
        }
        long end = System.nanoTime();

        double durationSeconds = (end - start) / 1_000_000_000.0;
        double callsPerSec = iterations / durationSeconds;

        printResult("Small Batch Calls", callsPerSec, "calls/s", 0);
        System.out.printf("JAVA_SMALLBATCH_OPS=%.0f%n", callsPerSec);
    }
}
