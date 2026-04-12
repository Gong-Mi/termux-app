use termux_rust::terminal::screen::Screen;
use termux_rust::terminal::style::STYLE_NORMAL;

#[test]
fn test_erase_in_display_mode_3_standard() {
    let mut screen = Screen::new(80, 24, 100);
    let style = STYLE_NORMAL;
    
    // 1. 制造一些历史记录 (通过滚动)
    for i in 0..50 {
        screen.scroll_up(0, 24, style);
        screen.get_row_mut(23).text[0] = std::char::from_u32('A' as u32 + (i % 26)).unwrap();
    }
    assert!(screen.active_transcript_rows > 0);
    
    // 2. 在当前屏幕写点内容
    screen.get_row_mut(10).text[0] = 'X';
    
    // 3. 执行 CSI 3 J (mode 3)
    screen.erase_in_display(3, 10, 0, style);
    
    // 4. 验证历史记录已清除
    assert_eq!(screen.active_transcript_rows, 0, "Transcript should be completely cleared in mode 3");
    
    // 5. 验证当前屏幕内容已保留 (对齐 xterm 标准)
    assert_eq!(screen.get_row(10).text[0], 'X', "Screen content should be preserved in mode 3 per xterm standards");
}
