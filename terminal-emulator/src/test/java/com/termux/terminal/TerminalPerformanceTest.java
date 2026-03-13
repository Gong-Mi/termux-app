package com.termux.terminal;

import java.nio.charset.StandardCharsets;
import java.util.Random;

public class TerminalPerformanceTest extends TerminalTestCase {

    private static final int COLS = 80;
    private static final int ROWS = 24;
    private static final int DATA_SIZE_MB = 5; // Reduced for CI stability
    private static final int DATA_SIZE_BYTES = DATA_SIZE_MB * 1024 * 1024;

    public void testRawTextPerformance() {
        withTerminalSized(COLS, ROWS);
        byte[] rawData = new byte[DATA_SIZE_BYTES];
        new Random(42).nextBytes(rawData);
        for (int i = 0; i < rawData.length; i++) {
            if (rawData[i] < 32 || rawData[i] > 126) rawData[i] = (byte) 'A';
        }

        long start = System.nanoTime();
        mTerminal.append(rawData, rawData.length);
        long end = System.nanoTime();

        double durationSeconds = (end - start) / 1_000_000_000.0;
        double speedMBps = DATA_SIZE_MB / durationSeconds;

        System.out.printf("Raw Text Performance: %.2f MB/s (Duration: %.2f s)%n", speedMBps, durationSeconds);
    }

    public void testAnsiEscapePerformance() {
        withTerminalSized(COLS, ROWS);
        StringBuilder sb = new StringBuilder();
        Random rand = new Random(42);
        
        int targetSize = 1024 * 1024; // 1MB complex data
        while (sb.length() < targetSize) {
            int type = rand.nextInt(5);
            switch (type) {
                case 0: sb.append("\033[31m"); break; 
                case 1: sb.append("\033[32m"); break; 
                case 2: sb.append("\033[H"); break;    
                case 3: sb.append("\033[2J"); break;   
                default: sb.append("Hello Performance Test "); break;
            }
        }
        
        byte[] ansiData = sb.toString().getBytes(StandardCharsets.UTF_8);
        int iterations = 3;
        
        long start = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            mTerminal.append(ansiData, ansiData.length);
        }
        long end = System.nanoTime();

        double totalProcessedMB = (ansiData.length * iterations) / (1024.0 * 1024.0);
        double durationSeconds = (end - start) / 1_000_000_000.0;
        double speedMBps = totalProcessedMB / durationSeconds;

        System.out.printf("ANSI Escape Performance: %.2f MB/s (Duration: %.2f s)%n", speedMBps, durationSeconds);
    }
}
