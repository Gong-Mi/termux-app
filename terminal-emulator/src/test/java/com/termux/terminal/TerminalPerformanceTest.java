package com.termux.terminal;

import java.nio.charset.StandardCharsets;
import java.util.Random;

public class TerminalPerformanceTest extends TerminalTestCase {

    private static final int COLS = 80;
    private static final int ROWS = 24;
    private static final int DATA_SIZE_MB = 10;
    private static final int DATA_SIZE_BYTES = DATA_SIZE_MB * 1024 * 1024;

    private void runPerformanceTest(String label, byte[] data, int iterations) {
        MockTerminalOutput output = new MockTerminalOutput();
        
        // 1. Rust 引擎 (当前 feature 分支)
        TerminalEmulator rustTerminal = new TerminalEmulator(output, COLS, ROWS, 13, 15, ROWS * 2, null);
        
        long startRust = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            rustTerminal.append(data, data.length);
        }
        long endRust = System.nanoTime();
        double rustTime = (endRust - startRust) / 1_000_000_000.0;
        double rustSpeed = (data.length * iterations / (1024.0 * 1024.0)) / rustTime;

        // 2. Java 引擎 (主线 master 版本, 完全隔离)
        JavaTerminalEmulator javaTerminal = new JavaTerminalEmulator(output, COLS, ROWS, 13, 15, ROWS * 2, null);
        
        long startJava = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            javaTerminal.append(data, data.length);
        }
        long endJava = System.nanoTime();
        double javaTime = (endJava - startJava) / 1_000_000_000.0;
        double javaSpeed = (data.length * iterations / (1024.0 * 1024.0)) / javaTime;

        System.out.printf("ENGINE_COMPARE [%s]: Rust=%.2f MB/s, Java=%.2f MB/s, Ratio=%.2fx (Rust is %s)%n",
                label, rustSpeed, javaSpeed, rustSpeed / javaSpeed,
                rustSpeed > javaSpeed ? "FASTER" : "SLOWER");
    }

    public void testRawTextPerformance() {
        byte[] rawData = new byte[DATA_SIZE_BYTES / 2]; // 5MB
        new Random(42).nextBytes(rawData);
        for (int i = 0; i < rawData.length; i++) {
            if (rawData[i] < 32 || rawData[i] > 126) rawData[i] = (byte) 'A';
        }
        runPerformanceTest("RAW_TEXT", rawData, 2);
    }

    public void testAnsiEscapePerformance() {
        StringBuilder sb = new StringBuilder();
        Random rand = new Random(42);
        int targetSize = 512 * 1024; // 0.5MB
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
        runPerformanceTest("ANSI_ESCAPES", ansiData, 4);
    }

    public void testScreenSyncPerformance() {
        MockTerminalOutput output = new MockTerminalOutput();
        TerminalEmulator rustTerminal = new TerminalEmulator(output, COLS, ROWS, 13, 15, ROWS * 2, null);
        
        byte[] fillData = new byte[COLS * ROWS];
        for(int i=0; i<fillData.length; i++) fillData[i] = 'X';
        rustTerminal.append(fillData, fillData.length);

        int iterations = 10000;
        
        long startRust = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            rustTerminal.syncStateFromRustIfRequired();
        }
        long endRust = System.nanoTime();
        double rustSyncTime = (endRust - startRust) / 1_000_000_000.0;
        double rustSyncSpeed = iterations / rustSyncTime;

        System.out.printf("ENGINE_COMPARE [SCREEN_SYNC]: Rust=%.0f syncs/s (DirectByteBuffer zero-copy)%n",
                rustSyncSpeed);
    }
}
