package com.termux.terminal;

import org.junit.Test;
import static org.junit.Assert.*;

public class CrashReproductionTest {

    @Test
    public void testResizeCrash() {
        int columns = 80;
        int rows = 24;
        int totalRows = 100;
        TerminalBuffer buffer = new TerminalBuffer(columns, totalRows, rows);
        
        int[] cursor = {0, 0};
        
        // 模拟触发之前发现的潜在崩溃路径：
        // 1. 调整大小
        // 2. 确保索引计算在边界情况下不会溢出
        
        // 修正：TerminalBuffer.resize 现在的签名是 (int newColumns, int newTotalRows, int newScreenRows, int[] cursor, long style)
        buffer.resize(columns + 10, rows, rows, cursor, 0);
        
        assertEquals(columns + 10, buffer.mColumns);
        assertEquals(rows, buffer.mScreenRows);
    }
}
