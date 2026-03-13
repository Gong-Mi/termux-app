#!/bin/bash
# Java vs Rust 性能对比测试脚本
# 使用方法：./scripts/compare-performance.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RUST_DIR="$PROJECT_ROOT/terminal-emulator/src/main/rust"
JAVA_TEST_DIR="$PROJECT_ROOT/terminal-emulator/src/test/java/com/termux/terminal"

echo "========================================"
echo "⚡ Java vs Rust Performance Comparison"
echo "========================================"
echo ""
echo "📁 Project Root: $PROJECT_ROOT"
echo "🦀 Rust Dir: $RUST_DIR"
echo "☕ Java Test: $JAVA_TEST_DIR"
echo ""

# 创建日志目录
LOG_DIR="$PROJECT_ROOT/build/performance-logs"
mkdir -p "$LOG_DIR"

JAVA_LOG="$LOG_DIR/java_performance.log"
RUST_LOG="$LOG_DIR/rust_performance.log"
REPORT_FILE="$LOG_DIR/comparison_report.md"

# =============================================================================
# Java 性能测试
# =============================================================================
echo "☕ Running Java Performance Tests..."
echo "----------------------------------------"

cd "$PROJECT_ROOT"
./gradlew :terminal-emulator:test --tests com.termux.terminal.JavaRustPerformanceComparisonTest --info 2>&1 | tee "$JAVA_LOG" || true

echo ""
echo "✅ Java tests completed. Log saved to: $JAVA_LOG"
echo ""

# =============================================================================
# Rust 性能测试
# =============================================================================
echo "🦀 Running Rust Performance Tests..."
echo "----------------------------------------"

cd "$RUST_DIR"
cargo test --test performance --release -- --nocapture 2>&1 | tee "$RUST_LOG" || true

echo ""
echo "✅ Rust tests completed. Log saved to: $RUST_LOG"
echo ""

# =============================================================================
# 生成对比报告
# =============================================================================
echo "📊 Generating Comparison Report..."
echo "----------------------------------------"

# 提取 Java 指标
extract_java_metric() {
    grep "$1" "$JAVA_LOG" | tail -n 1 | sed -r 's/.*=([0-9.]+).*/\1/' 2>/dev/null || echo "N/A"
}

# 提取 Rust 指标
extract_rust_metric() {
    grep "$1" "$RUST_LOG" | tail -n 1 | sed -r 's/.*=([0-9.]+).*/\1/' 2>/dev/null || echo "N/A"
}

# 提取指标
J_RAW=$(extract_java_metric "JAVA_RAW_TEXT_MBPS")
J_ANSI=$(extract_java_metric "JAVA_ANSI_MBPS")
J_CURSOR=$(extract_java_metric "JAVA_CURSOR_OPS")
J_SCROLL=$(extract_java_metric "JAVA_SCROLL_LINES")
J_WIDECHAR=$(extract_java_metric "JAVA_WIDECHAR_OPS")
J_SMALLBATCH=$(extract_java_metric "JAVA_SMALLBATCH_OPS")

R_RAW=$(extract_rust_metric "RUST_RAW_TEXT_MBPS")
R_ANSI=$(extract_rust_metric "RUST_ANSI_MBPS")
R_CURSOR=$(extract_rust_metric "RUST_CURSOR_OPS")
R_SCROLL=$(extract_rust_metric "RUST_SCROLL_LINES")
R_WIDECHAR=$(extract_rust_metric "RUST_WIDECHAR_OPS")
R_SMALLBATCH=$(extract_rust_metric "RUST_SMALLBATCH_OPS")

# 计算 speedup
calc_speedup() {
    local java="$1"
    local rust="$2"
    if [ "$java" != "N/A" ] && [ "$rust" != "N/A" ] && [ "$java" != "0" ]; then
        echo "scale=2; $rust / $java" | bc 2>/dev/null || echo "N/A"
    else
        echo "N/A"
    fi
}

RAW_SPEEDUP=$(calc_speedup "$J_RAW" "$R_RAW")
ANSI_SPEEDUP=$(calc_speedup "$J_ANSI" "$R_ANSI")
CURSOR_SPEEDUP=$(calc_speedup "$J_CURSOR" "$R_CURSOR")
SCROLL_SPEEDUP=$(calc_speedup "$J_SCROLL" "$R_SCROLL")
WIDECHAR_SPEEDUP=$(calc_speedup "$J_WIDECHAR" "$R_WIDECHAR")
SMALLBATCH_SPEEDUP=$(calc_speedup "$J_SMALLBATCH" "$R_SMALLBATCH")

# 生成报告
cat > "$REPORT_FILE" << EOF
# ⚡ Java vs Rust Performance Comparison Report

**Generated:** $(date -u '+%Y-%m-%d %H:%M:%S UTC')

## 📊 Throughput Comparison

| Metric | Java | Rust | Speedup (Rust/Java) |
|--------|------|------|---------------------|
| Raw Text | ${J_RAW} MB/s | ${R_RAW} MB/s | ${RAW_SPEEDUP}x |
| ANSI Escape | ${J_ANSI} MB/s | ${R_ANSI} MB/s | ${ANSI_SPEEDUP}x |
| Cursor Movement | ${J_CURSOR} K ops/s | ${R_CURSOR} K ops/s | ${CURSOR_SPEEDUP}x |
| Scrolling | ${J_SCROLL} K lines/s | ${R_SCROLL} K lines/s | ${SCROLL_SPEEDUP}x |
| Wide Char | ${J_WIDECHAR} K chars/s | ${R_WIDECHAR} K chars/s | ${WIDECHAR_SPEEDUP}x |
| Small Batch | ${J_SMALLBATCH} K calls/s | ${R_SMALLBATCH} K calls/s | ${SMALLBATCH_SPEEDUP}x |

## 📝 Analysis

### Raw Text Processing
- Java: ${J_RAW} MB/s
- Rust: ${R_RAW} MB/s
- **Speedup: ${RAW_SPEEDUP}x**

### ANSI Escape Sequence Processing
- Java: ${J_ANSI} MB/s
- Rust: ${R_ANSI} MB/s
- **Speedup: ${ANSI_SPEEDUP}x**

### Cursor Movement
- Java: ${J_CURSOR} K ops/s
- Rust: ${R_CURSOR} K ops/s
- **Speedup: ${CURSOR_SPEEDUP}x**

### Scrolling
- Java: ${J_SCROLL} K lines/s
- Rust: ${R_SCROLL} K lines/s
- **Speedup: ${SCROLL_SPEEDUP}x**

### Wide Character (Chinese) Processing
- Java: ${J_WIDECHAR} K chars/s
- Rust: ${R_WIDECHAR} K chars/s
- **Speedup: ${WIDECHAR_SPEEDUP}x**

### Small Batch Calls
- Java: ${J_SMALLBATCH} K calls/s
- Rust: ${R_SMALLBATCH} K calls/s
- **Speedup: ${SMALLBATCH_SPEEDUP}x**

## 📋 Notes

- All tests use identical input data (same seed: 42) for fair comparison
- Speedup > 1.0x indicates Rust is faster
- Speedup < 1.0x indicates Java is faster
- Tests are run locally, results may vary from CI environment

## 📁 Log Files

- Java Log: \`$JAVA_LOG\`
- Rust Log: \`$RUST_LOG\`
EOF

echo "✅ Report generated: $REPORT_FILE"
echo ""

# 显示报告摘要
echo "========================================"
echo "📊 Quick Summary"
echo "========================================"
echo ""
printf "%-20s | %-15s | %-15s | %-10s\n" "Metric" "Java" "Rust" "Speedup"
echo "---------------------|-----------------|-----------------|------------"
printf "%-20s | %-15s | %-15s | %-10s\n" "Raw Text" "${J_RAW} MB/s" "${R_RAW} MB/s" "${RAW_SPEEDUP}x"
printf "%-20s | %-15s | %-15s | %-10s\n" "ANSI Escape" "${J_ANSI} MB/s" "${R_ANSI} MB/s" "${ANSI_SPEEDUP}x"
printf "%-20s | %-15s | %-15s | %-10s\n" "Cursor Movement" "${J_CURSOR} K/s" "${R_CURSOR} K/s" "${CURSOR_SPEEDUP}x"
printf "%-20s | %-15s | %-15s | %-10s\n" "Scrolling" "${J_SCROLL} K/s" "${R_SCROLL} K/s" "${SCROLL_SPEEDUP}x"
printf "%-20s | %-15s | %-15s | %-10s\n" "Wide Char" "${J_WIDECHAR} K/s" "${R_WIDECHAR} K/s" "${WIDECHAR_SPEEDUP}x"
printf "%-20s | %-15s | %-15s | %-10s\n" "Small Batch" "${J_SMALLBATCH} K/s" "${R_SMALLBATCH} K/s" "${SMALLBATCH_SPEEDUP}x"
echo ""
echo "📄 Full report: $REPORT_FILE"
echo "========================================"
