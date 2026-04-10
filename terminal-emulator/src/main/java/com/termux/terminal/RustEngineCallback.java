package com.termux.terminal;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

/**
 * 回调接口：由 Rust 引擎直接通过 JNI 调用
 * 注意：此类必须是顶层公共类，以便 JNI 反射能够轻松找到方法
 * 
 * 实现 TerminalSessionClient 接口，以便可以直接传给 Rust JNI
 */
public class RustEngineCallback implements TerminalSessionClient {
    private final TerminalSessionClient mClient;
    private TerminalSession mSession;

    public RustEngineCallback(TerminalSessionClient client) {
        this.mClient = client;
    }

    public void setSession(TerminalSession session) {
        this.mSession = session;
    }

    public void onScreenUpdate() {
        // 屏幕更新通知 - 目前不需要特殊处理
    }

    public void onScreenUpdated() {
        if (mSession != null) {
            mSession.onNativeScreenUpdated();
        } else if (mClient != null) {
            mClient.onTextChanged(null);
        }
    }

    /**
     * Called when the Rust engine and PTY are initialized asynchronously.
     */
    public void onEngineInitialized(long enginePtr, int ptyFd, int pid) {
        if (mSession != null) {
            mSession.onEngineInitialized(enginePtr, ptyFd, pid);
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
            mClient.onCopyTextToClipboard(null, text);
        }
    }

    public void onPasteTextFromClipboard() {
        if (mClient != null) {
            mClient.onPasteTextFromClipboard(null);
        }
    }

    public void onWriteToSession(String data) {
        // Rust 终端响应数据写入 - 通过 PTY 文件描述符处理
        if (mClient != null) {
            mClient.logVerbose("RustEngineCallback", "Write to session: " + data);
        }
    }

    public void onWriteToSessionBytes(byte[] data) {
        // 二进制数据写入
        if (mClient != null) {
            mClient.logVerbose("RustEngineCallback", "Write " + data.length + " bytes to session");
        }
    }

    public void write(String data) {
        onWriteToSession(data);
    }

    public void writeBytes(byte[] data) {
        onWriteToSessionBytes(data);
    }

    public void reportColorResponse(String colorSpec) {
        write(colorSpec);
    }

    public void reportTerminalResponse(String response) {
        write(response);
    }

    /**
     * Sixel 图像回调 - 由 Rust 引擎通过 JNI 调用
     * @param rgbaData RGBA 格式的图像数据
     * @param width 图像宽度
     * @param height 图像高度
     * @param startX 起始 X 坐标（字符位置）
     * @param startY 起始 Y 坐标（字符位置）
     */
    public void onSixelImage(byte[] rgbaData, int width, int height, int startX, int startY) {
        if (mClient != null) {
            mClient.logDebug("SixelImage", String.format("Received Sixel image: %dx%d at (%d,%d), data size: %d",
                width, height, startX, startY, rgbaData != null ? rgbaData.length : 0));
            // 将图像数据传递给 TerminalView 进行渲染
            mClient.onSixelImage(rgbaData, width, height, startX, startY);
        }
    }

    /**
     * 清屏回调 - 由 Rust 引擎通过 JNI 调用
     */
    public void onClearScreen() {
        if (mClient != null) {
            mClient.logDebug("SixelImage", "Clear screen event received");
            mClient.onClearScreen();
        }
    }

    // TerminalSessionClient 接口实现 - 委托给 mClient

    @Override
    public void onTextChanged(@NonNull TerminalSession changedSession) {
        if (mClient != null) mClient.onTextChanged(changedSession);
    }

    @Override
    public void onTitleChanged(@NonNull TerminalSession changedSession) {
        if (mClient != null) mClient.onTitleChanged(changedSession);
    }

    @Override
    public void onSessionFinished(@NonNull TerminalSession finishedSession) {
        if (mClient != null) mClient.onSessionFinished(finishedSession);
    }

    @Override
    public void onCopyTextToClipboard(@NonNull TerminalSession session, String text) {
        if (mClient != null) mClient.onCopyTextToClipboard(session, text);
    }

    @Override
    public void onPasteTextFromClipboard(@Nullable TerminalSession session) {
        if (mClient != null) mClient.onPasteTextFromClipboard(session);
    }

    @Override
    public void onBell(@NonNull TerminalSession session) {
        if (mClient != null) mClient.onBell(session);
    }

    @Override
    public void onColorsChanged(@NonNull TerminalSession session) {
        if (mClient != null) mClient.onColorsChanged(session);
    }

    @Override
    public void onTerminalCursorStateChange(boolean state) {
        if (mClient != null) mClient.onTerminalCursorStateChange(state);
    }

    @Override
    public void setTerminalShellPid(@NonNull TerminalSession session, int pid) {
        if (mClient != null) mClient.setTerminalShellPid(session, pid);
    }

    @Override
    public Integer getTerminalCursorStyle() {
        return mClient != null ? mClient.getTerminalCursorStyle() : null;
    }

    @Override
    public void logError(String tag, String message) {
        if (mClient != null) mClient.logError(tag, message);
    }

    @Override
    public void logWarn(String tag, String message) {
        if (mClient != null) mClient.logWarn(tag, message);
    }

    @Override
    public void logInfo(String tag, String message) {
        if (mClient != null) mClient.logInfo(tag, message);
    }

    @Override
    public void logDebug(String tag, String message) {
        if (mClient != null) mClient.logDebug(tag, message);
    }

    @Override
    public void logVerbose(String tag, String message) {
        if (mClient != null) mClient.logVerbose(tag, message);
    }

    @Override
    public void logStackTraceWithMessage(String tag, String message, Exception e) {
        if (mClient != null) mClient.logStackTraceWithMessage(tag, message, e);
    }

    @Override
    public void logStackTrace(String tag, Exception e) {
        if (mClient != null) mClient.logStackTrace(tag, e);
    }
}
