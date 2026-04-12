// Truecolor (24-bit RGB) rendering tests
// Run: cargo test --test truecolor_render_test -- --nocapture

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

/// Verify truecolor foreground sequences are parsed and stored
#[test]
fn test_truecolor_fg_parsed() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // \x1b[38;2;255;128;64m = truecolor foreground RGB(255,128,64)
    engine.process_bytes(b"\x1b[38;2;255;128;64mHello");

    let style = engine.state.main_screen.get_row(0).styles[0];
    assert!((style & 0x200) != 0, "STYLE_TRUECOLOR_FG flag should be set");

    let decoded = termux_rust::terminal::style::decode_fore_color(style);
    // Should be 0xffFF8040
    let r = ((decoded >> 16) & 0xFF) as u8;
    let g = ((decoded >> 8) & 0xFF) as u8;
    let b = (decoded & 0xFF) as u8;
    assert_eq!(r, 255, "Red channel should be 255");
    assert_eq!(g, 128, "Green channel should be 128");
    assert_eq!(b, 64, "Blue channel should be 64");

    println!("✅ Truecolor FG parsed correctly: RGB({}, {}, {})", r, g, b);
}

/// Verify truecolor background sequences are parsed and stored
#[test]
fn test_truecolor_bg_parsed() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // \x1b[48;2;0;128;255m = truecolor background RGB(0,128,255)
    engine.process_bytes(b"\x1b[48;2;0;128;255mWorld");

    let style = engine.state.main_screen.get_row(0).styles[0];
    assert!((style & 0x400) != 0, "STYLE_TRUECOLOR_BG flag should be set");

    let decoded = termux_rust::terminal::style::decode_back_color(style);
    let r = ((decoded >> 16) & 0xFF) as u8;
    let g = ((decoded >> 8) & 0xFF) as u8;
    let b = (decoded & 0xFF) as u8;
    assert_eq!(r, 0, "Red channel should be 0");
    assert_eq!(g, 128, "Green channel should be 128");
    assert_eq!(b, 255, "Blue channel should be 255");

    println!("✅ Truecolor BG parsed correctly: RGB({}, {}, {})", r, g, b);
}

/// Verify 256-color palette sequences work
#[test]
fn test_256_color_palette() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // \x1b[38;5;196m = red in 256-color palette
    engine.process_bytes(b"\x1b[38;5;196mRed");

    let style = engine.state.main_screen.get_row(0).styles[0];
    // Should NOT have truecolor flag
    assert!((style & 0x200) == 0, "Should NOT have truecolor flag for palette colors");

    let decoded = termux_rust::terminal::style::decode_fore_color(style);
    assert_eq!(decoded as usize, 196, "Should decode to palette index 196");

    println!("✅ 256-color palette parsed correctly: index 196");
}

/// Verify truecolor + bold don't corrupt the color
#[test]
fn test_truecolor_with_bold() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // \x1b[1;38;2;255;0;0m = bold + truecolor red
    engine.process_bytes(b"\x1b[1;38;2;255;0;0mBOLD");

    let style = engine.state.main_screen.get_row(0).styles[0];
    assert!((style & 0x200) != 0, "Truecolor flag should still be set");

    let decoded = termux_rust::terminal::style::decode_fore_color(style);
    let r = ((decoded >> 16) & 0xFF) as u8;
    let g = ((decoded >> 8) & 0xFF) as u8;
    let b = (decoded & 0xFF) as u8;
    assert_eq!(r, 255, "Red should be 255 (not shifted by bold→bright)");
    assert_eq!(g, 0, "Green should be 0");
    assert_eq!(b, 0, "Blue should be 0");

    println!("✅ Truecolor + bold: RGB({}, {}, {}) correctly preserved", r, g, b);
}

/// Verify text content is stored correctly with truecolor
#[test]
fn test_truecolor_text_content() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    engine.process_bytes(b"\x1b[38;2;100;200;150mColored Text\x1b[0m");

    let text = get_row_text(&engine, 0);
    assert!(text.contains("Colored Text"), "Text should be stored: '{}'", text);

    println!("✅ Text content stored with truecolor: '{}'", text.trim_end());
}

/// Verify resetting colors works
#[test]
fn test_truecolor_reset() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // Set truecolor, write text, reset, write more text
    engine.process_bytes(b"\x1b[38;2;255;0;0mRed\x1b[0mNormal");

    // First 3 chars should have truecolor
    let style_red = engine.state.main_screen.get_row(0).styles[0];
    assert!((style_red & 0x200) != 0, "Red text should have truecolor flag");

    // After reset, chars should NOT have truecolor
    let style_normal = engine.state.main_screen.get_row(0).styles[3];
    assert!((style_normal & 0x200) == 0, "Normal text should NOT have truecolor flag");

    println!("✅ Truecolor reset works correctly");
}

/// Verify combined foreground + background truecolor in a single SGR sequence
#[test]
fn test_truecolor_fg_and_bg() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // \x1b[38;2;255;255;255;48;2;0;0m = white on black in one sequence
    engine.process_bytes(b"\x1b[38;2;255;255;255;48;2;0;0mWhiteOnBlack");

    let style = engine.state.main_screen.get_row(0).styles[0];
    assert!((style & 0x200) != 0, "FG truecolor flag should be set");
    assert!((style & 0x400) != 0, "BG truecolor flag should be set");

    let fg = termux_rust::terminal::style::decode_fore_color(style);
    let bg = termux_rust::terminal::style::decode_back_color(style);

    assert_eq!((fg >> 16) & 0xFF, 255, "FG red should be 255");
    assert_eq!((fg >> 8) & 0xFF, 255, "FG green should be 255");
    assert_eq!(fg & 0xFF, 255, "FG blue should be 255");

    assert_eq!((bg >> 16) & 0xFF, 0, "BG red should be 0");
    assert_eq!((bg >> 8) & 0xFF, 0, "BG green should be 0");
    assert_eq!(bg & 0xFF, 0, "BG blue should be 0");

    println!("✅ Truecolor FG+BG combined in one sequence: FG=0x{:08X}, BG=0x{:08X}", fg, bg);
}
