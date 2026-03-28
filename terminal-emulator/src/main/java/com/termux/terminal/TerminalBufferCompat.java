package com.termux.terminal;

/**
 * TerminalBuffer 兼容层 - 为依赖 getScreen() 的旧应用提供兼容性支持
 * 
 * Rust 版本使用共享内存直接访问屏幕数据，此包装类提供与传统 Java 版本兼容的 API。
 * 
 * @deprecated 新代码应直接使用 readRow() 等 JNI 方法访问屏幕数据
 */
@Deprecated
public final class TerminalBufferCompat {
    
    private final TerminalEmulator mEmulator;
    private final int mColumns;
    private final int mScreenRows;
    private final int mTotalRows;
    
    /**
     * 创建 TerminalBuffer 兼容包装
     * 
     * @param emulator TerminalEmulator 实例
     * @param columns 列数
     * @param screenRows 屏幕行数
     * @param totalRows 总行数（包括历史）
     */
    public TerminalBufferCompat(TerminalEmulator emulator, int columns, int screenRows, int totalRows) {
        this.mEmulator = emulator;
        this.mColumns = columns;
        this.mScreenRows = screenRows;
        this.mTotalRows = totalRows;
    }
    
    /**
     * 获取屏幕宽度（列数）
     */
    public int getColumns() {
        return mColumns;
    }
    
    /**
     * 获取屏幕高度（行数）
     */
    public int getScreenRows() {
        return mScreenRows;
    }
    
    /**
     * 获取总行数（包括历史）
     */
    public int getTotalRows() {
        return mTotalRows;
    }
    
    /**
     * 获取活动历史行数
     */
    public int getActiveTranscriptRows() {
        if (mEmulator != null) {
            return mEmulator.getActiveTranscriptRows();
        }
        return 0;
    }
    
    /**
     * 获取转义文本（整个缓冲区）
     */
    public String getTranscriptText() {
        if (mEmulator != null) {
            return mEmulator.getTranscriptText();
        }
        return "";
    }
    
    /**
     * 获取选中区域的文本
     * 
     * @param selX1 起始列
     * @param selY1 起始行（外部坐标）
     * @param selX2 结束列
     * @param selY2 结束行（外部坐标）
     * @return 选中的文本
     */
    public String getSelectedText(int selX1, int selY1, int selX2, int selY2) {
        if (mEmulator != null) {
            return mEmulator.getSelectedText(selX1, selY1, selX2, selY2);
        }
        return "";
    }
}
