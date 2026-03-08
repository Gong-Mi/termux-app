/// 快速扫描 ASCII 字节流，直到遇到控制字符、非 ASCII 字符或触发状态变更
pub fn scan_ascii_batch(input: &[u8], _use_line_drawing: bool) -> usize {
    let mut processed = 0;

    for &b in input {
        // 即使在绘图模式下，我们也可以批量处理 ASCII (0x20-0x7E)
        // 具体的转换将在写入阶段由 writeASCIIBatchNative 完成。
        if (0x20..=0x7E).contains(&b) {
            processed += 1;
        } else {
            break;
        }
    }

    processed
}
