use termux_rust::terminal::screen::Screen;

#[test]
fn test_selection_logic() {
    let mut s = Screen::new(10, 5, 10);
    // Row 0: "Hello" (exactly 5 chars)
    for (i, ch) in "Hello".chars().enumerate() {
        s.get_row_mut(0 as i64).set_char(i as u64, ch as u32, 0);
    }
    // Row 1: "World"
    for (i, ch) in "World".chars().enumerate() {
        s.get_row_mut(1 as i64).set_char(i as u64, ch as u32, 0);
    }

    let text = s.get_selected_text(0 as i64, 0 as i64, 4 as i64, 1 as i64);

    // Let's print it to see exactly what is extracted
    println!("Selected text: {:?}", text);
    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
}
