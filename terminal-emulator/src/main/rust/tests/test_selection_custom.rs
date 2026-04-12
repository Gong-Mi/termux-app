use termux_rust::terminal::screen::Screen;

#[test]
fn test_selection_logic() {
    let mut s = Screen::new(10, 5, 10);
    // Write "Hello" on row 0
    for (i, ch) in "Hello".chars().enumerate() {
        s.get_row_mut(0).set_char(i, ch as u32, 0);
    }
    // Write "World" on row 1
    for (i, ch) in "World".chars().enumerate() {
        s.get_row_mut(1).set_char(i, ch as u32, 0);
    }

    let text = s.get_selected_text(0, 0, 4, 1);
    // Let's print it to see exactly what is extracted
    println!("Selected text: {:?}", text);
    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
}
