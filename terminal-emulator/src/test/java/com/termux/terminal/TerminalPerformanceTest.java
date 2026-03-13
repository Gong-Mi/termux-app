package com.termux.terminal;

import java.nio.charset.StandardCharsets;
import java.util.Random;

public class TerminalPerformanceTest extends TerminalTestCase {

    private static final int COLS = 80;
    private static final int ROWS = 24;
    private static final int DATA_SIZE_MB = 10;
    private static final int DATA_SIZE_BYTES = DATA_SIZE_MB * 1024 * 1024;

    private void runPerformanceTest(String label, byte[] data, int iterations) {
        // 1. 测试 Rust 引擎性能
        TerminalEmulator.sForceDisableRust = false;
        withTerminalSized(COLS, ROWS);
        
        long startRust = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            mTerminal.append(data, data.length);
        }
        long endRust = System.nanoTime();
        double rustTime = (endRust - startRust) / 1_000_000_000.0;
        double rustSpeed = (data.length * iterations / (1024.0 * 1024.0)) / rustTime;

        // 2. 测试 Java 引擎性能 (对比项)
        TerminalEmulator.sForceDisableRust = true;
        withTerminalSized(COLS, ROWS);
        
        long startJava = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            mTerminal.append(data, data.length);
        }
        long endJava = System.nanoTime();
        double javaTime = (endJava - startJava) / 1_000_000_000.0;
        double javaSpeed = (data.length * iterations / (1024.0 * 1024.0)) / javaTime;

        System.out.printf("ENGINE_COMPARE [%s]: Rust=%.2f MB/s, Java=%.2f MB/s, Ratio=%.2fx (Rust is %s)%n",
                label, rustSpeed, javaSpeed, rustSpeed / javaSpeed,
                rustSpeed > javaSpeed ? "FASTER" : "SLOWER");
        
        // 重置状态
        TerminalEmulator.sForceDisableRust = false;
    }

    public void testRawTextPerformance() {
        byte[] rawData = new byte[DATA_SIZE_BYTES / 2]; // 5MB for faster CI
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

    /**
     * 测试屏幕同步性能 (重点验证 DirectByteBuffer 优化)
     */
    public void testScreenSyncPerformance() {
        withTerminalSized(COLS, ROWS);
        // 先填充屏幕
        byte[] fillData = new byte[COLS * ROWS];
        for(int i=0; i<fillData.length; i++) fillData[i] = 'X';
        mTerminal.append(fillData, fillData.length);

        int iterations = 1000;
        
        // 1. Rust 模式下的同步性能 (现在使用了 DirectByteBuffer)
        TerminalEmulator.sForceDisableRust = false;
        long startRust = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            mTerminal.syncStateFromRustIfRequired();
        }
        long endRust = System.nanoTime();
        double rustSyncTime = (endRust - startRust) / 1_000_000_000.0;
        double rustSyncSpeed = iterations / rustSyncTime;

        // 2. Java 模式下的同步性能 (实际上 Java 模式不需要从 Rust 同步，这里模拟一下开销)
        TerminalEmulator.sForceDisableRust = true;
        long startJava = System.nanoTime();
        for (int i = 0; i < iterations; i++) {
            // Java 模式下这个方法基本是空操作或简单的内存拷贝
            mTerminal.syncStateFromRustIfRequired();
        }
        long endJava = System.nanoTime();
        double javaSyncTime = (endJava - startJava) / 1_000_000_000.0;
        double javaSyncSpeed = iterations / javaSyncTime;

        System.out.printf("ENGINE_COMPARE [SCREEN_SYNC]: Rust=%.0f syncs/s, Java=%.0f syncs/s, Ratio=%.2fx%n",
                rustSyncSpeed, javaSyncSpeed, rustSyncSpeed / javaSyncSpeed);
        
        TerminalEmulator.sForceDisableRust = false;
    }
}
