#[test]
fn test_char_width() {
    println!("Width of '你': {}", termux_rust::utils::get_char_width('你' as u32));
}
