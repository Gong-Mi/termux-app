package com.termux.terminal;

/**
 * 回调接口：由 Rust 引擎直接通过 JNI 调用
 * 注意：此类必须是顶层公共类，以便 JNI 反射能够轻松找到方法
 */
public class RustEngineCallback {
    private final TerminalSessionClient mClient;

    public RustEngineCallback(TerminalSessionClient client) {
        this.mClient = client;
    }

    public void onScreenUpdate() {
        if (mClient != null) {
            // 注意：mClient 本身实现了对文本变化的响应
            // 我们通过接口定义的通用方法进行通知
            mClient.logDebug("Termux-JNI", "onScreenUpdate triggered");
        }
    }

    public void reportTitleChange(String title) {
        if (mClient != null) mClient.reportTitleChange(title);
    }

    public void onColorsChanged() {
        if (mClient != null) mClient.onColorsChanged();
    }

    public void reportCursorVisibility(boolean visible) {
        if (mClient != null) mClient.onTerminalCursorStateChange(visible);
    }

    public void onBell() {
        if (mClient != null) {
            mClient.onBell();
        }
    }

    public void onCopyTextToClipboard(String text) {
        if (mClient != null) {
            // 如果需要复制，调用 client 对应的方法
            mClient.logDebug("Termux-JNI", "Copy to clipboard requested");
        }
    }

    public void onPasteTextFromClipboard() {
        if (mClient != null) {
            mClient.onPasteTextFromClipboard(null); // 这里的参数逻辑需要根据实际 Session 调整
        }
    }

    public void onWriteToSession(String data) {
        // Rust 终端响应数据写入
        // 注意：实际写入操作由 Rust 通过 PTY 文件描述符处理
        // 这里仅用于日志记录
        if (mClient != null) {
            mClient.logVerbose("RustEngineCallback", "Write to session: " + data);
        }
    }
    
    public void onWriteToSessionBytes(byte[] data) {
        // 二进制数据写入 - 目前仅用于日志
        if (mClient != null) {
            mClient.logVerbose("RustEngineCallback", "Write " + data.length + " bytes to session");
        }
    }
    
    public void write(String data) {
        // Rust 终端响应写入（如鼠标事件、颜色查询响应等）
        onWriteToSession(data);
    }
    
    public void writeBytes(byte[] data) {
        onWriteToSessionBytes(data);
    }
    
    public void reportColorResponse(String colorSpec) {
        // 颜色响应
        write(colorSpec);
    }
    
    public void reportTerminalResponse(String response) {
        // 终端响应（如 DEC 设备状态报告等）
        write(response);
    }
}
