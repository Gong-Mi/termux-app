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
            return mEmulator.getActiveTranscriptRowsFromRust();
        }
        return 0;
    }
    
    /**
     * 获取指定行的文本
     * 
     * @param externalRow 外部行坐标（-activeTranscriptRows 到 screenRows-1）
     * @return 行文本
     */
    public String getLine(int externalRow) {
        if (mEmulator == null) return "";
        
        char[] buffer = new char[mColumns];
        mEmulator.readRowFromRust(mEmulator.mEnginePtr, externalRow, buffer);
        return new String(buffer).trim();
    }
    
    /**
     * 获取指定位置的字符
     * 
     * @param col 列坐标
     * @param row 行坐标（外部坐标）
     * @return 字符
     */
    public char getChar(int col, int row) {
        if (mEmulator == null) return ' ';

        char[] buffer = new char[1];
        mEmulator.readRowFromRust(mEmulator.mEnginePtr, row, buffer, col, 1);
        return buffer[0];
    }
    
    /**
     * 获取指定位置的样式
     * 
     * @param col 列坐标
     * @param row 行坐标（外部坐标）
     * @return 样式值
     */
    public long getStyle(int col, int row) {
        if (mEmulator == null) return TextStyle.NORMAL;
        
        long[] buffer = new long[1];
        mEmulator.readRowStyleFromRust(mEmulator.mEnginePtr, row, buffer, col, 1);
        return buffer[0];
    }
    
    /**
     * 获取转义文本（整个缓冲区）
     */
    public String getTranscriptText() {
        if (mEmulator != null) {
            return mEmulator.getTranscriptTextFromRust();
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
            return mEmulator.getSelectedTextFromRust(selX1, selY1, selX2, selY2);
        }
        return "";
    }
    
    /**
     * 设置或清除效果位（DECCARA 支持）
     *
     * @param bits 效果位
     * @param setOrClear true=设置，false=清除
     * @param reverse 是否反转
     * @param rectangular 是否矩形区域
     * @param leftMargin 左边距
     * @param rightMargin 右边距
     * @param top 顶部行
     * @param left 左侧列
     * @param bottom 底部行
     * @param right 右侧列
     */
    public void setOrClearEffect(int bits, boolean setOrClear, boolean reverse, boolean rectangular,
                                 int leftMargin, int rightMargin, int top, int left, int bottom, int right) {
        if (mEmulator != null) {
            mEmulator.setOrClearEffectFromRust(mEmulator.mEnginePtr, bits, setOrClear, reverse, rectangular,
                                               leftMargin, rightMargin, top, left, bottom, right);
        }
    }
    
    /**
     * 清除历史记录
     */
    public void clearTranscript() {
        if (mEmulator != null) {
            mEmulator.clearTranscriptFromRust(mEmulator.mEnginePtr);
        }
    }
    
    /**
     * 检查是否为空行
     *
     * @param row 行坐标（外部坐标）
     * @return true 如果是空行
     */
    public boolean isBlankLine(int row) {
        if (mEmulator == null) return true;
        
        char[] buffer = new char[mColumns];
        mEmulator.readRowFromRust(mEmulator.mEnginePtr, row, buffer);
        
        for (char c : buffer) {
            if (c != ' ') return false;
        }
        return true;
    }
    
    /**
     * 获取行包装状态
     *
     * @param row 行坐标（外部坐标）
     * @return true 如果该行被换行
     */
    public boolean getLineWrap(int row) {
        if (mEmulator != null) {
            return mEmulator.getLineWrapFromRust(mEmulator.mEnginePtr, row);
        }
        return false;
    }
}
