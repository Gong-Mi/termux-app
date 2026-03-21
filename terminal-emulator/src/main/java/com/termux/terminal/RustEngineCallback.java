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
        if (mClient != null) {
            mClient.onWriteToSession(data);
        }
    }
    
    public void onWriteToSessionBytes(byte[] data) { }
    
    public void write(String data) {
        // Rust 终端响应写入
        onWriteToSession(data);
    }
    
    public void writeBytes(byte[] data) {
        onWriteToSessionBytes(data);
    }
    
    public void reportColorResponse(String colorSpec) { }
    public void reportTerminalResponse(String response) { }
}
