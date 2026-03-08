package com.termux.terminal;

import java.nio.charset.StandardCharsets;
import java.util.Random;

/**
 * Consistency test to ensure that the Hybrid/Rust engine produces identical 
 * results to the legacy Java implementation.
 */
public class TerminalConsistencyTest extends TerminalTestCase {

    private TerminalEmulator legacyEmulator;
    private TerminalEmulator hybridEmulator;
    private MockTerminalOutput legacyOutput;
    private MockTerminalOutput hybridOutput;

    private static final int COLS = 80;
    private static final int ROWS = 24;

    @Override
    protected void setUp() throws Exception {
        super.setUp();
        legacyOutput = new MockTerminalOutput();
        hybridOutput = new MockTerminalOutput();
        
        // Assume TerminalEmulator has a way to force legacy mode (e.g., a flag or constructor)
        // For now, we simulate by having two instances.
        legacyEmulator = new TerminalEmulator(legacyOutput, COLS, ROWS, 10, 15, ROWS * 2, null);
        hybridEmulator = new TerminalEmulator(hybridOutput, COLS, ROWS, 10, 15, ROWS * 2, null);
    }

    public void testRandomSequencesConsistency() {
        Random rand = new Random(42);
        int iterations = 1000;
        
        for (int i = 0; i < iterations; i++) {
            byte[] chunk = generateRandomAnsiData(rand, 256);
            
            // 1. Process with Legacy (assuming we can bypass Rust for test)
            processLegacy(legacyEmulator, chunk);
            
            // 2. Process with Hybrid (default)
            hybridEmulator.append(chunk, chunk.length);
            
            // 3. Assert Consistency
            assertEmulatorsMatch(i, chunk);
        }
    }

    private void processLegacy(TerminalEmulator emulator, byte[] data) {
        // Enable legacy mode globally for this call
        boolean oldForce = TerminalEmulator.sForceDisableRust;
        TerminalEmulator.sForceDisableRust = true;
        try {
            emulator.append(data, data.length);
        } finally {
            TerminalEmulator.sForceDisableRust = oldForce;
        }
    }

    private void assertEmulatorsMatch(int iteration, byte[] lastChunk) {
        if (legacyEmulator.getCursorRow() != hybridEmulator.getCursorRow() ||
            legacyEmulator.getCursorCol() != hybridEmulator.getCursorCol()) {
            
            System.err.println("!!! CONSISTENCY ERROR at iteration " + iteration);
            System.err.println("Last Bytes processed: " + bytesToHex(lastChunk));
            System.err.println("Legacy Cursor: " + legacyEmulator.getCursorRow() + "," + legacyEmulator.getCursorCol());
            System.err.println("Hybrid Cursor: " + hybridEmulator.getCursorRow() + "," + hybridEmulator.getCursorCol());
            
            assertEquals("Cursor Row Mismatch", legacyEmulator.getCursorRow(), hybridEmulator.getCursorRow());
            assertEquals("Cursor Col Mismatch", legacyEmulator.getCursorCol(), hybridEmulator.getCursorCol());
        }
    }

    private String bytesToHex(byte[] bytes) {
        StringBuilder sb = new StringBuilder();
        for (byte b : bytes) {
            sb.append(String.format("%02X ", b));
        }
        return sb.toString();
    }

    private byte[] generateRandomAnsiData(Random rand, int len) {
        StringBuilder sb = new StringBuilder();
        while (sb.length() < len) {
            int type = rand.nextInt(10);
            if (type < 7) {
                sb.append((char)(rand.nextInt(95) + 32)); // Printable
            } else {
                // Common Escape Sequences
                switch (rand.nextInt(5)) {
                    case 0: sb.append("\033[H"); break;
                    case 1: sb.append("\033[3" + rand.nextInt(8) + "m"); break;
                    case 2: sb.append("\033[J"); break;
                    case 3: sb.append("\r\n"); break;
                    case 4: sb.append("\033[" + rand.nextInt(24) + "A"); break;
                }
            }
        }
        return sb.toString().getBytes(StandardCharsets.UTF_8);
    }
}
