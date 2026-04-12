use termux_rust::terminal::screen::Screen;
use termux_rust::terminal::style::STYLE_NORMAL;

#[test]
fn test_reflow_wide_char_atomicity() {
    let mut screen = Screen::new(10, 5, 100);
    let style = STYLE_NORMAL;
    let char_zhong = '\u{4e2d}'; // "中"

    // 在旧行末尾写入 "A中" (A 占 1 位，中占 2 位)
    // 布局：[A, 中, \0] -> 占 index 7, 8, 9
    screen.get_row_mut(0).set_char(7, 'A' as u32, style);
    screen.get_row_mut(0).set_char(8, char_zhong as u32, style);
    screen.get_row_mut(0).text[9] = '\0'; 
    screen.get_row_mut(0).line_wrap = true;

    println!("BEFORE REFLOW:");
    let idx0 = screen.internal_row(0);
    let row0_info: String = screen.buffer[idx0].text.iter()
        .map(|&c| if c == '\0' { "[0]".to_string() } else if c == ' ' { "_".to_string() } else { c.to_string() })
        .collect();
    println!("Internal Row 0 (idx {}): {}", idx0, row0_info);

    // 缩减到宽度 9
    screen.resize_with_reflow(9, 5, style, 0, 0);

    println!("AFTER REFLOW:");
    let mut found_zhong = false;
    for r in 0..5 {
        let row = screen.get_row(r as i32);
        let row_hex: Vec<String> = row.text.iter().map(|&c| format!("U+{:04X}", c as u32)).collect();
        println!("Row {}: {:?}", r, row_hex);
        
        for c in 0..row.text.len() {
            if row.text[c] == char_zhong {
                found_zhong = true;
                if c + 1 < row.text.len() {
                    assert_eq!(row.text[c+1], '\0', "Wide character and its placeholder split at row {}, col {}", r, c);
                } else {
                    panic!("Wide character U+4E2D at the very end of row {} with no room for placeholder", r);
                }
            }
        }
    }
    assert!(found_zhong, "Character U+4E2D lost during reflow");
}


#[test]
fn test_reflow_empty_lines_preservation() {
    let mut screen = Screen::new(80, 24, 100);
    let style = STYLE_NORMAL;
    
    // 1. 写入一行文字，空两行，再写入一行文字
    screen.get_row_mut(0).text[0] = 'A';
    // 行 1, 2 保持全空
    screen.get_row_mut(3).text[0] = 'B';
    
    // 2. 触发 reflow (宽度变窄)
    screen.resize_with_reflow(40, 24, style, 0, 0);
    
    // 3. 验证 'B' 是否还在原来的相对位置（即前面是否有空行保留）
    // 如果 B 移动到了第 1 行（相对于 A），说明空行被错误过滤了
    let mut found_b = -1;
    for i in 0..10 {
        if screen.get_row(i).text[0] == 'B' {
            found_b = i;
            break;
        }
    }
    assert!(found_b >= 2, "Empty lines between content should be preserved during reflow");
}
