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
        if (mClient != null && mClient instanceof TerminalSession) {
            mClient.onTextChanged((TerminalSession) mClient);
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
            if (mClient instanceof TerminalSession) {
                mClient.onBell((TerminalSession) mClient);
            } else {
                mClient.onBell();
            }
        }
    }

    public void onCopyTextToClipboard(String text) {
        if (mClient != null && mClient instanceof TerminalSession) {
            mClient.onCopyTextToClipboard((TerminalSession) mClient, text);
        }
    }

    public void onPasteTextFromClipboard() {
        if (mClient != null && mClient instanceof TerminalSession) {
            mClient.onPasteTextFromClipboard((TerminalSession) mClient);
        }
    }

    public void onWriteToSession(String data) {
        // Rust 引擎请求向会话写入数据（例如应答）
    }

    public void onWriteToSessionBytes(byte[] data) {
        // Rust 引擎请求向会话写入原始字节
    }

    public void reportColorResponse(String colorSpec) { }
    public void reportTerminalResponse(String response) { }
}
