# Rust 一致性测试方案

## 当前问题

1. **ConsistencyTest 是 Android Instrumentation Test**
   - 需要 Android 设备/模拟器才能运行
   - 无法在本地 (Termux) 直接运行
   - 依赖 AndroidJUnit4 和 Android 框架

2. **Rust 引擎无法在纯 Java 环境测试**
   - Rust 代码通过 JNI 暴露功能
   - 需要加载 native library
   - 当前架构下 Rust 引擎状态与 Java 不同步

## 解决方案：分离测试与功能

### 方案 A：创建纯 Java 一致性测试 (推荐)

创建一个独立的测试类，不依赖 Android 框架，可以直接在 Termux 中运行。

**文件位置**: `terminal-emulator/src/test/java/com/termux/terminal/RustConsistencyTest.java`

**特点**:
- 使用 JUnit 4 (非 Android)
- 直接比较 Java 和 Rust 的输出
- 可以通过 `./gradlew test` 运行

### 方案 B：创建 Rust 端独立测试

创建一个独立的 Rust 测试程序，直接调用 `TerminalEngine` 并验证输出。

**文件位置**: `terminal-emulator/src/main/rust/tests/consistency_test.rs`

**特点**:
- 纯 Rust 测试
- 使用 Rust 的 `#[test]` 宏
- 可以通过 `cargo test` 运行
- 需要定义预期的终端行为

### 方案 C：创建 Python 验证脚本

创建一个 Python 脚本，同时调用 Java 和 Rust 实现并比较结果。

**文件位置**: `terminal-emulator/tools/consistency_check.py`

**特点**:
- 语言无关
- 可以生成详细报告
- 易于扩展新测试用例

## 实现方案 A (推荐)

### 1. 修改 TerminalEmulator 使其可测试

当前问题：
- `ConsistencyTest` 依赖 `TerminalEmulator.isRustLibLoaded()` 和 `sForceDisableRust`
- Rust 引擎目前被禁用，总是从 Java Buffer 读取

需要修改：
```java
// 添加一个测试模式，强制使用 Rust 引擎
public static boolean sForceUseRustEngine = false;

// 在 getRowContent 中
public void getRowContent(int row, char[] destText, long[] destStyle) {
    if (sForceUseRustEngine && sRustLibLoaded && mRustEnginePtr != 0) {
        readRowFromRust(mRustEnginePtr, row, destText, destStyle);
    } else {
        // 从 Java Buffer 读取
        TerminalRow line = mScreen.allocateFullLineIfNecessary(...);
        // ...
    }
}
```

### 2. 创建纯 Java 测试类

```java
package com.termux.terminal;

import junit.framework.TestCase;
import java.nio.charset.StandardCharsets;

/**
 * 纯 Java 一致性测试 - 比较 Java 和 Rust 引擎的输出
 * 
 * 运行方式：
 * ./gradlew :terminal-emulator:testDebugUnitTest --tests RustConsistencyTest
 */
public class RustConsistencyTest extends TestCase {

    static class MockTerminalOutput extends TerminalOutput {
        @Override public void write(byte[] data, int offset, int count) {}
        @Override public void titleChanged(String oldTitle, String newTitle) {}
        @Override public void onCopyTextToClipboard(String text) {}
        @Override public void onPasteTextFromClipboard() {}
        @Override public void onBell() {}
        @Override public void onColorsChanged() {}
    }

    static class MockTerminalSessionClient implements TerminalSessionClient {
        @Override public void onTextChanged(TerminalSession session) {}
        @Override public void onTitleChanged(TerminalSession session) {}
        @Override public void onSessionFinished(TerminalSession session) {}
        @Override public void onCopyTextToClipboard(TerminalSession session, String text) {}
        @Override public void onPasteTextFromClipboard(TerminalSession session) {}
        @Override public void onBell(TerminalSession session) {}
        @Override public void onColorsChanged(TerminalSession session) {}
        @Override public void onTerminalCursorStateChange(boolean state) {}
        @Override public void setTerminalShellPid(TerminalSession session, int pid) {}
        @Override public Integer getTerminalCursorStyle() { 
            return TerminalEmulator.TERMINAL_CURSOR_STYLE_BLOCK; 
        }
        @Override public void logError(String tag, String message) {}
        @Override public void logWarn(String tag, String message) {}
        @Override public void logInfo(String tag, String message) {}
        @Override public void logDebug(String tag, String message) {}
        @Override public void logVerbose(String tag, String message) {}
        @Override public void logStackTraceWithMessage(String tag, String message, Exception e) {}
        @Override public void logStackTrace(String tag, Exception e) {}
    }

    private void runTest(String name, String input) {
        // 注意：当前 Rust 引擎已禁用，此测试将跳过
        // 当 Rust 引擎完整实现后，可以重新启用
        if (!TerminalEmulator.isRustLibLoaded()) {
            System.out.println("SKIP " + name + ": Rust library not loaded");
            return;
        }

        int cols = 80;
        int rows = 24;
        MockTerminalSessionClient client = new MockTerminalSessionClient();
        MockTerminalOutput output = new MockTerminalOutput();

        // 1. Java Only (Reference)
        TerminalEmulator javaEmulator = new TerminalEmulator(output, cols, rows, 10, 20, 100, client);
        TerminalEmulator.sForceDisableRust = true;
        byte[] bytes = input.getBytes(StandardCharsets.UTF_8);
        javaEmulator.append(bytes, bytes.length);

        // 2. Rust Enabled (Experimental)
        TerminalEmulator.sForceDisableRust = false;
        TerminalEmulator rustEmulator = new TerminalEmulator(output, cols, rows, 10, 20, 100, client);
        rustEmulator.append(bytes, bytes.length);

        // 比对光标
        assertEquals(name + " - Cursor Column", 
            javaEmulator.getCursorCol(), rustEmulator.getCursorCol());
        assertEquals(name + " - Cursor Row", 
            javaEmulator.getCursorRow(), rustEmulator.getCursorRow());

        // 比对缓冲区内容
        char[] javaText = new char[cols * 2];
        long[] javaStyle = new long[cols];
        char[] rustText = new char[cols * 2];
        long[] rustStyle = new long[cols];

        for (int r = 0; r < rows; r++) {
            javaEmulator.getRowContent(r, javaText, javaStyle);
            rustEmulator.getRowContent(r, rustText, rustStyle);

            // 比较文本内容（跳过尾部空格）
            String javaStr = new String(javaText).stripTrailing();
            String rustStr = new String(rustText).stripTrailing();
            assertEquals(name + " - Row " + r + " text", javaStr, rustStr);
        }
        
        System.out.println("PASS " + name);
    }

    public void testBasicText() {
        runTest("basic_hello", "Hello World");
        runTest("basic_newline", "Line 1\r\nLine 2");
    }

    public void testAutoWrap() {
        runTest("autowrap_long_line", 
            "A very long line designed to test the auto-wrapping logic.");
    }

    public void testCursorMovement() {
        runTest("cursor_cup", "\u001B[5;5HAt 5,5");
        runTest("cursor_backspace", "ABC\bDE\r\nFG");
    }

    public void testErase() {
        runTest("erase_ed", "Should be erased\u001B[2JStill here");
        runTest("erase_el", "Erase this line\u001B[2K");
    }

    public void testColors() {
        runTest("color_fg", "\u001B[31mRed\u001B[0m");
        runTest("color_bg", "\u001B[42mGreen BG\u001B[0m");
    }
}
```

### 3. 创建 Rust 端测试

```rust
// terminal-emulator/src/main/rust/tests/consistency.rs

use termux_rust::engine::{TerminalEngine, ScreenState};

#[test]
fn test_basic_text() {
    let mut engine = TerminalEngine::new(80, 24, 100);
    
    let data = b"Hello World";
    engine.process_bytes(data);
    
    assert_eq!(engine.state.cursor_x, 11);
    assert_eq!(engine.state.cursor_y, 0);
}

#[test]
fn test_newline() {
    let mut engine = TerminalEngine::new(80, 24, 100);
    
    let data = b"Line 1\r\nLine 2";
    engine.process_bytes(data);
    
    assert_eq!(engine.state.cursor_x, 5);
    assert_eq!(engine.state.cursor_y, 1);
}

#[test]
fn test_cursor_position() {
    let mut engine = TerminalEngine::new(80, 24, 100);
    
    // CSI 5;5 H - Move to row 5, column 5
    let data = b"\x1b[5;5HAt 5,5";
    engine.process_bytes(data);
    
    assert_eq!(engine.state.cursor_x, 5);
    assert_eq!(engine.state.cursor_y, 4);
}

#[test]
fn test_erase_display() {
    let mut engine = TerminalEngine::new(80, 24, 100);
    
    // Print text then clear screen
    let data = b"Should be erased\x1b[2JStill here";
    engine.process_bytes(data);
    
    // After ED 2, screen should be cleared and cursor at home
    // "Still here" should be at position (0, 10)
    assert_eq!(engine.state.cursor_x, 10);
    assert_eq!(engine.state.cursor_y, 0);
}
```

### 4. 创建 Python 验证脚本

```python
#!/usr/bin/env python3
"""
Rust 一致性验证脚本

运行 Java 和 Rust 实现并比较结果。
需要：
- Java 8+
- Rust 1.85+
- 已编译的 terminal-emulator JAR
"""

import subprocess
import sys
import json

TEST_CASES = [
    {"name": "basic_hello", "input": "Hello World"},
    {"name": "basic_newline", "input": "Line 1\\r\\nLine 2"},
    {"name": "cursor_cup", "input": "\\u001B[5;5HAt 5,5"},
    {"name": "erase_ed", "input": "Should be erased\\u001B[2JStill here"},
]

def run_java_test(test_input):
    """运行 Java 测试并返回结果"""
    # 这里需要调用 Java 测试类
    # 可以使用 jcommander 或直接调用 Java
    pass

def run_rust_test(test_input):
    """运行 Rust 测试并返回结果"""
    # 调用 Rust 测试二进制文件
    pass

def compare_results(java_result, rust_result, test_name):
    """比较 Java 和 Rust 的结果"""
    if java_result == rust_result:
        print(f"✓ PASS {test_name}")
        return True
    else:
        print(f"✗ FAIL {test_name}")
        print(f"  Java: {java_result}")
        print(f"  Rust: {rust_result}")
        return False

def main():
    print("Rust Consistency Check")
    print("=" * 50)
    
    passed = 0
    failed = 0
    
    for test in TEST_CASES:
        name = test["name"]
        input_data = test["input"].encode().decode('unicode_escape')
        
        java_result = run_java_test(input_data)
        rust_result = run_rust_test(input_data)
        
        if compare_results(java_result, rust_result, name):
            passed += 1
        else:
            failed += 1
    
    print("=" * 50)
    print(f"Results: {passed} passed, {failed} failed")
    return 0 if failed == 0 else 1

if __name__ == "__main__":
    sys.exit(main())
```

## 推荐步骤

### 第一阶段：分离测试框架

1. 创建 `RustConsistencyTest.java` (纯 JUnit 4 测试)
2. 修改 `TerminalEmulator` 添加测试模式开关
3. 配置 Gradle 测试任务

### 第二阶段：Rust 端单元测试

1. 创建 `tests/consistency.rs`
2. 添加基本测试用例
3. 配置 `cargo test`

### 第三阶段：集成验证

1. 创建 Python 验证脚本
2. 添加 CI 集成
3. 生成测试报告

## 当前状态

由于 Rust 引擎已禁用（`FULL TAKEOVER` 模式关闭），一致性测试将：
- 总是从 Java Buffer 读取
- 无法验证 Rust 引擎的正确性

**建议**: 先完成方案 A 的测试框架搭建，等 Rust 引擎完整实现后再启用完整测试。
