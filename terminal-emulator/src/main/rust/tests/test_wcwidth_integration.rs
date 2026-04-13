use termux_rust_new::wcwidth::wcwidth;
use termux_rust_new::terminal::screen::TerminalRow;

#[test]
fn test_wcwidth_basic_categories() {
    // 1. ASCII 字符 (宽度 1)
    assert_eq!(wcwidth('A' as u32), 1);
    assert_eq!(wcwidth('a' as u32), 1);
    assert_eq!(wcwidth('0' as u32), 1);
    assert_eq!(wcwidth(' ' as u32), 1);

    // 2. 宽字符 (CJK 汉字, 宽度 2)
    assert_eq!(wcwidth('中' as u32), 2); // U+4E2D
    assert_eq!(wcwidth('文' as u32), 2);

    // 3. Emoji 表情 (宽度 2)
    assert_eq!(wcwidth('😀' as u32), 2); // U+1F600
    assert_eq!(wcwidth('🚀' as u32), 2); // U+1F680

    // 4. 零宽字符 / 组合字符 (宽度 0)
    assert_eq!(wcwidth(0x0300), 0); // Combining Grave Accent
    assert_eq!(wcwidth(0x200B), 0); // Zero Width Space

    // 5. 控制字符 (宽度 0)
    assert_eq!(wcwidth(0), 0); // Null (用作宽字符的占位符)
    assert_eq!(wcwidth('\n' as u32), 0);
    assert_eq!(wcwidth('\r' as u32), 0);
}

#[test]
fn test_terminal_row_find_char_index_with_wide_chars() {
    // 创建一个包含宽字符的 TerminalRow 进行测试
    // 假设终端宽度为 10
    let mut row = TerminalRow::new(10);
    
    // 填充数据: "A中B"
    // 'A' (宽 1), '中' (宽 2, 占位符 \0), 'B' (宽 1)
    row.text[0] = 'A';
    row.text[1] = '中';
    row.text[2] = '\0'; // 宽字符占位符
    row.text[3] = 'B';
    
    // 测试 find_char_index_at_column，该方法依赖 local_get_width (已替换为 wcwidth)
    // 逻辑列 0 -> 'A' (索引 0)
    assert_eq!(row.find_char_index_at_column(0), 0);
    
    // 逻辑列 1 -> '中' (索引 1)
    assert_eq!(row.find_char_index_at_column(1), 1);
    
    // 逻辑列 2 -> 宽字符占位符对应同一字符，或者映射到占位符
    // 由于 '中' 宽度为 2，查找列 2 时，cur_col 累加后会跳过，取决于实现。
    // 在官方逻辑中，列 1 和列 2 都属于 '中'。
    // 但当前的 find_char_index_at_column 实现：
    // width('中') = 2. cur_col += 2 -> 3.  查找列 2 时，遇到 \0 (width=0)，直接返回。
    assert_eq!(row.find_char_index_at_column(2), 2);
    
    // 逻辑列 3 -> 'B' (索引 3)
    assert_eq!(row.find_char_index_at_column(3), 3);
}

#[test]
fn test_terminal_row_space_used() {
    let mut row = TerminalRow::new(10);
    row.text[0] = 'H';
    row.text[1] = 'i';
    row.text[2] = ' ';
    row.text[3] = ' '; // 尾部空格
    
    // get_space_used 应该忽略尾部空格和 \0
    assert_eq!(row.get_space_used(), 2);
    
    row.text[0] = '中';
    row.text[1] = '\0'; // 宽字符占位符
    row.text[2] = ' ';
    
    assert_eq!(row.get_space_used(), 1);
}
