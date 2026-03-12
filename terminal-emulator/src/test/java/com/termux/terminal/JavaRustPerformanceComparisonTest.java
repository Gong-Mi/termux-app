package com.termux.terminal;

import junit.framework.TestCase;
import java.nio.charset.StandardCharsets;
import java.util.Random;

/**
 * Java vs Rust 性能对比测试
 * 
 * 运行方式:
 * ./gradlew :terminal-emulator:testDebug --tests "JavaRustPerformanceComparisonTest"
 * 
 * 注意：需要 Rust 库已编译并加载才能进行对比测试
 */
public class JavaRustPerformanceComparisonTest extends TestCase {

    private static final int COLS = 80;
    private static final int ROWS = 24;
    private static final int WARMUP_ITERATIONS = 3;
    private static final int TEST_ITERATIONS = 5;

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
     * 生成随机 ASCII 数据
     */
    private byte[] generateRandomAscii(int sizeBytes) {
        byte[] data = new byte[sizeBytes];
        Random rand = new Random(42);
        for (int i = 0; i < sizeBytes; i++) {
            byte b = (byte) rand.nextInt();
            // 确保是可打印 ASCII
            if (b < 32 || b > 126) {
                b = (byte) 'A';
            }
            data[i] = b;
        }
        return data;
    }

    /**
     * 生成 ANSI 转义序列数据
     */
    private byte[] generateAnsiData(int sizeBytes) {
        StringBuilder sb = new StringBuilder();
        Random rand = new Random(42);
        
        while (sb.length() * 3 < sizeBytes) { // 估算
            int type = rand.nextInt(5);
            switch (type) {
                case 0: sb.append("\u001B[31m"); break; // 红色
                case 1: sb.append("\u001B[32m"); break; // 绿色
                case 2: sb.append("\u001B[H"); break;   // 光标归位
                case 3: sb.append("\u001B[2J"); break;  // 清屏
                default: sb.append("Hello Performance Test "); break;
            }
        }
        
        return sb.toString().getBytes(StandardCharsets.UTF_8);
    }

    /**
     * 预热 JVM
     */
    private void warmup(TerminalEmulator terminal, byte[] data) {
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            terminal.append(data, data.length);
        }
    }

    /**
     * 运行性能测试
     */
    private double runPerformanceTest(TerminalEmulator terminal, byte[] data, int iterations) {
        long totalBytes = (long) data.length * iterations;
        
        long start = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            terminal.append(data, data.length);
        }
        long end = System.nanoTime();
        
        double durationSeconds = (end - start) / 1_000_000_000.0;
        double throughputMBps = (totalBytes / (1024.0 * 1024.0)) / durationSeconds;
        
        return throughputMBps;
    }

    /**
     * 测试 1: 原始 ASCII 文本处理性能对比
     */
    public void test01_RawAsciiPerformance() {
        System.out.println("\n=== Test 1: Raw ASCII Text Performance ===");
        
        int dataSizeKB = 500; // 500KB per iteration
        byte[] data = generateRandomAscii(dataSizeKB * 1024);
        
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();
        
        // Java-only 测试
        TerminalEmulator javaTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        warmup(javaTerminal, data);
        double javaSpeed = runPerformanceTest(javaTerminal, data, TEST_ITERATIONS);
        
        // Java + Rust Fast Path 测试
        TerminalEmulator hybridTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = false;
        warmup(hybridTerminal, data);
        double hybridSpeed = runPerformanceTest(hybridTerminal, data, TEST_ITERATIONS);
        
        double speedup = hybridSpeed / javaSpeed;
        double improvement = ((hybridSpeed - javaSpeed) / javaSpeed) * 100;
        
        System.out.printf("Java-only:           %.2f MB/s%n", javaSpeed);
        System.out.printf("Java + Rust FastPath: %.2f MB/s%n", hybridSpeed);
        System.out.printf("Speedup:              %.2fx (%.1f%% improvement)%n", speedup, improvement);
        
        // 验证 Rust 快路径至少不应该更慢
        assertTrue("Rust fast path should not be slower", hybridSpeed >= javaSpeed * 0.95);
    }

    /**
     * 测试 2: ANSI 转义序列性能对比
     */
    public void test02_AnsiEscapePerformance() {
        System.out.println("\n=== Test 2: ANSI Escape Sequence Performance ===");
        
        int dataSizeKB = 100; // 100KB per iteration (ANSI is more complex)
        byte[] data = generateAnsiData(dataSizeKB * 1024);
        
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();
        
        // Java-only 测试
        TerminalEmulator javaTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        warmup(javaTerminal, data);
        double javaSpeed = runPerformanceTest(javaTerminal, data, TEST_ITERATIONS);
        
        // Java + Rust Fast Path 测试
        TerminalEmulator hybridTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = false;
        warmup(hybridTerminal, data);
        double hybridSpeed = runPerformanceTest(hybridTerminal, data, TEST_ITERATIONS);
        
        double speedup = hybridSpeed / javaSpeed;
        double improvement = ((hybridSpeed - javaSpeed) / javaSpeed) * 100;
        
        System.out.printf("Java-only:           %.2f MB/s%n", javaSpeed);
        System.out.printf("Java + Rust FastPath: %.2f MB/s%n", hybridSpeed);
        System.out.printf("Speedup:              %.2fx (%.1f%% improvement)%n", speedup, improvement);
        
        // ANSI 序列复杂，Rust 快路径可能帮助有限
        assertTrue("Hybrid should perform reasonably", hybridSpeed >= javaSpeed * 0.8);
    }

    /**
     * 测试 3: 混合负载性能（真实场景模拟）
     */
    public void test03_MixedWorkloadPerformance() {
        System.out.println("\n=== Test 3: Mixed Workload Performance ===");
        
        // 模拟真实终端会话：80% 文本 + 20% 控制序列
        StringBuilder sb = new StringBuilder();
        Random rand = new Random(42);
        int targetSize = 200 * 1024; // 200KB
        
        while (sb.length() < targetSize) {
            if (rand.nextDouble() < 0.8) {
                // 80% 普通文本
                sb.append("The quick brown fox jumps over the lazy dog. ");
            } else {
                // 20% 控制序列
                int seqType = rand.nextInt(4);
                switch (seqType) {
                    case 0: sb.append("\r\n"); break;
                    case 1: sb.append("\t"); break;
                    case 2: sb.append("\u001B[31m"); break;
                    case 3: sb.append("\u001B[0m"); break;
                }
            }
        }
        
        byte[] data = sb.toString().getBytes(StandardCharsets.UTF_8);
        
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();
        
        // Java-only 测试
        TerminalEmulator javaTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        warmup(javaTerminal, data);
        double javaSpeed = runPerformanceTest(javaTerminal, data, TEST_ITERATIONS);
        
        // Java + Rust Fast Path 测试
        TerminalEmulator hybridTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = false;
        warmup(hybridTerminal, data);
        double hybridSpeed = runPerformanceTest(hybridTerminal, data, TEST_ITERATIONS);
        
        double speedup = hybridSpeed / javaSpeed;
        double improvement = ((hybridSpeed - javaSpeed) / javaSpeed) * 100;
        
        System.out.printf("Java-only:           %.2f MB/s%n", javaSpeed);
        System.out.printf("Java + Rust FastPath: %.2f MB/s%n", hybridSpeed);
        System.out.printf("Speedup:              %.2fx (%.1f%% improvement)%n", speedup, improvement);
        
        assertTrue("Mixed workload should benefit from Rust", hybridSpeed >= javaSpeed * 0.95);
    }

    /**
     * 测试 4: 光标移动操作性能
     */
    public void test04_CursorMovementPerformance() {
        System.out.println("\n=== Test 4: Cursor Movement Performance ===");
        
        // 生成大量光标移动序列
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 1000; i++) {
            sb.append("\u001B[5;10H");
            sb.append("\u001B[10;20H");
            sb.append("\u001B[15;30H");
            sb.append("\u001B[1;1H");
        }
        byte[] data = sb.toString().getBytes(StandardCharsets.UTF_8);
        
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();
        
        // Java-only 测试
        TerminalEmulator javaTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        warmup(javaTerminal, data);
        
        long start = System.nanoTime();
        for (int i = 0; i < TEST_ITERATIONS; i++) {
            javaTerminal.append(data, data.length);
        }
        long javaDuration = System.nanoTime() - start;
        double javaOpsPerSec = (4000.0 * TEST_ITERATIONS) / (javaDuration / 1_000_000_000.0);
        
        // Java + Rust 测试
        TerminalEmulator hybridTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = false;
        warmup(hybridTerminal, data);
        
        start = System.nanoTime();
        for (int i = 0; i < TEST_ITERATIONS; i++) {
            hybridTerminal.append(data, data.length);
        }
        long hybridDuration = System.nanoTime() - start;
        double hybridOpsPerSec = (4000.0 * TEST_ITERATIONS) / (hybridDuration / 1_000_000_000.0);
        
        double speedup = hybridOpsPerSec / javaOpsPerSec;
        
        System.out.printf("Java-only:           %.0f ops/s%n", javaOpsPerSec);
        System.out.printf("Java + Rust FastPath: %.0f ops/s%n", hybridOpsPerSec);
        System.out.printf("Speedup:              %.2fx%n", speedup);
    }

    /**
     * 测试 5: 滚动性能测试
     */
    public void test05_ScrollingPerformance() {
        System.out.println("\n=== Test 5: Scrolling Performance ===");
        
        // 生成超过屏幕行数的文本（触发滚动）
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 1000; i++) {
            sb.append("Line ").append(i).append("\r\n");
        }
        byte[] data = sb.toString().getBytes(StandardCharsets.UTF_8);
        
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();
        
        // Java-only 测试
        TerminalEmulator javaTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        warmup(javaTerminal, data);
        
        long start = System.nanoTime();
        for (int i = 0; i < TEST_ITERATIONS; i++) {
            javaTerminal.append(data, data.length);
        }
        long javaDuration = System.nanoTime() - start;
        double javaLinesPerSec = (1000.0 * TEST_ITERATIONS) / (javaDuration / 1_000_000_000.0);
        
        // Java + Rust 测试
        TerminalEmulator hybridTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = false;
        warmup(hybridTerminal, data);
        
        start = System.nanoTime();
        for (int i = 0; i < TEST_ITERATIONS; i++) {
            hybridTerminal.append(data, data.length);
        }
        long hybridDuration = System.nanoTime() - start;
        double hybridLinesPerSec = (1000.0 * TEST_ITERATIONS) / (hybridDuration / 1_000_000_000.0);
        
        double speedup = hybridLinesPerSec / javaLinesPerSec;
        
        System.out.printf("Java-only:           %.0f lines/s%n", javaLinesPerSec);
        System.out.printf("Java + Rust FastPath: %.0f lines/s%n", hybridLinesPerSec);
        System.out.printf("Speedup:              %.2fx%n", speedup);
    }

    /**
     * 测试 6: 内存分配对比
     */
    public void test06_MemoryAllocationComparison() {
        System.out.println("\n=== Test 6: Memory Allocation (Indirect) ===");
        
        byte[] data = generateRandomAscii(100 * 1024);
        
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();
        
        Runtime runtime = Runtime.getRuntime();
        
        // Java-only 测试
        TerminalEmulator javaTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        
        runtime.gc();
        long javaStartMem = runtime.totalMemory() - runtime.freeMemory();
        
        for (int i = 0; i < 10; i++) {
            javaTerminal.append(data, data.length);
        }
        
        long javaEndMem = runtime.totalMemory() - runtime.freeMemory();
        long javaAllocated = javaEndMem - javaStartMem;
        
        // Java + Rust 测试
        TerminalEmulator hybridTerminal = new TerminalEmulator(output, COLS, ROWS, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = false;
        
        runtime.gc();
        long hybridStartMem = runtime.totalMemory() - runtime.freeMemory();
        
        for (int i = 0; i < 10; i++) {
            hybridTerminal.append(data, data.length);
        }
        
        long hybridEndMem = runtime.totalMemory() - runtime.freeMemory();
        long hybridAllocated = hybridEndMem - hybridStartMem;
        
        System.out.printf("Java-only allocation:    %d KB%n", javaAllocated / 1024);
        System.out.printf("Java + Rust allocation:  %d KB%n", hybridAllocated / 1024);
        
        // Rust 快路径应该减少 Java 侧的分配
        // 但由于 JNI 开销，可能差异不大
        System.out.println("Note: Memory measurement is approximate due to GC");
    }
}
