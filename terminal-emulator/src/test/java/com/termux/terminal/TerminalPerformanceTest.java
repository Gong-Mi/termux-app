package com.termux.terminal;

import java.nio.charset.StandardCharsets;
import java.util.Random;

public class TerminalPerformanceTest extends TerminalTestCase {

    private static final int COLS = 80;
    private static final int ROWS = 24;
    private static final int DATA_SIZE_MB = 10; // 10MB test data
    private static final int DATA_SIZE_BYTES = DATA_SIZE_MB * 1024 * 1024;

    /**
     * Test raw text processing speed (no escape sequences).
     */
    public void testRawTextPerformance() {
        withTerminalSized(COLS, ROWS);
        byte[] rawData = new byte[DATA_SIZE_BYTES];
        new Random(42).nextBytes(rawData);
        // Ensure data is mostly printable to avoid random side effects
        for (int i = 0; i < rawData.length; i++) {
            if (rawData[i] < 32 || rawData[i] > 126) rawData[i] = (byte) 'A';
        }

        long start = System.nanoTime();
        mTerminal.append(rawData, rawData.length);
        long end = System.nanoTime();

        double durationSeconds = (end - start) / 1_000_000_000.0;
        double speedMBps = DATA_SIZE_MB / durationSeconds;

        System.out.printf("Raw Text Performance: %.2f MB/s (Duration: %.2f s)%n", speedMBps, durationSeconds);
        
        // Threshold: 20MB/s on CI should be more reliable
        assertTrue("Performance too low: " + speedMBps + " MB/s", speedMBps > 20);
    }

    /**
     * Test performance with heavy ANSI escape sequences (colors, cursor movements).
     */
    public void testAnsiEscapePerformance() {
        withTerminalSized(COLS, ROWS);
        StringBuilder sb = new StringBuilder();
        Random rand = new Random(42);
        
        // Generate ~1MB of complex ANSI data
        int targetSize = 1024 * 1024;
        while (sb.length() < targetSize) {
            int type = rand.nextInt(5);
            switch (type) {
                case 0: sb.append("\033[31m"); break; // Color Red
                case 1: sb.append("\033[32m"); break; // Color Green
                case 2: sb.append("\033[H"); break;    // Cursor Home
                case 3: sb.append("\033[2J"); break;   // Clear Screen
                default: sb.append("Hello Performance Test "); break;
            }
        }
        
        byte[] ansiData = sb.toString().getBytes(StandardCharsets.UTF_8);
        int iterations = 5; // Run 5 times to get 5MB total processed
        
        long start = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            mTerminal.append(ansiData, ansiData.length);
        }
        long end = System.nanoTime();

        double totalProcessedMB = (ansiData.length * iterations) / (1024.0 * 1024.0);
        double durationSeconds = (end - start) / 1_000_000_000.0;
        double speedMBps = totalProcessedMB / durationSeconds;

        System.out.printf("ANSI Escape Performance: %.2f MB/s (Duration: %.2f s)%n", speedMBps, durationSeconds);
        
        // Threshold: 2MB/s minimum for complex sequences on CI
        assertTrue("ANSI Performance too low: " + speedMBps + " MB/s", speedMBps > 2);
    }
}
