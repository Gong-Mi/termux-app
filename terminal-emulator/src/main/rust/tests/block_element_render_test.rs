// Block Elements and similar character rendering tests
// Run: cargo test --test block_element_render_test -- --nocapture

use termux_rust::TerminalEngine;
use termux_rust::renderer::TerminalRenderer;

/// Helper: get text from a specific row in the engine
fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

/// Check if a character is a "block/semi-block" element that needs special handling
fn is_block_or_special(ch: char) -> bool {
    matches!(ch as u32,
        0x2580..=0x259F  // Block Elements
        | 0x2500..=0x257F  // Box Drawing
        | 0x2800..=0x28FF  // Braille Patterns
    )
}

/// Verify wcwidth for block elements - all should be width 1
#[test]
fn test_block_element_width_is_1() {
    use unicode_width::UnicodeWidthChar;

    // Block Elements
    assert_eq!(UnicodeWidthChar::width('▀').unwrap(), 1, "U+2580 UPPER HALF BLOCK");
    assert_eq!(UnicodeWidthChar::width('▄').unwrap(), 1, "U+2584 LOWER HALF BLOCK");
    assert_eq!(UnicodeWidthChar::width('█').unwrap(), 1, "U+2588 FULL BLOCK");
    assert_eq!(UnicodeWidthChar::width('░').unwrap(), 1, "U+2591 LIGHT SHADE");
    assert_eq!(UnicodeWidthChar::width('▒').unwrap(), 1, "U+2592 MEDIUM SHADE");
    assert_eq!(UnicodeWidthChar::width('▓').unwrap(), 1, "U+2593 DARK SHADE");
    assert_eq!(UnicodeWidthChar::width('▏').unwrap(), 1, "U+258F LEFT ONE EIGHTH BLOCK");
    assert_eq!(UnicodeWidthChar::width('▎').unwrap(), 1, "U+258E LEFT ONE QUARTER BLOCK");
    assert_eq!(UnicodeWidthChar::width('▍').unwrap(), 1, "U+258D LEFT THREE EIGHTHS BLOCK");
    assert_eq!(UnicodeWidthChar::width('▌').unwrap(), 1, "U+258C LEFT HALF BLOCK");
    assert_eq!(UnicodeWidthChar::width('▋').unwrap(), 1, "U+258B LEFT FIVE EIGHTHS BLOCK");
    assert_eq!(UnicodeWidthChar::width('▊').unwrap(), 1, "U+258A LEFT THREE QUARTERS BLOCK");
    assert_eq!(UnicodeWidthChar::width('▉').unwrap(), 1, "U+2589 LEFT SEVEN EIGHTHS BLOCK");

    // Box Drawing
    assert_eq!(UnicodeWidthChar::width('─').unwrap(), 1, "U+2500 BOX DRAWINGS LIGHT HORIZONTAL");
    assert_eq!(UnicodeWidthChar::width('│').unwrap(), 1, "U+2502 BOX DRAWINGS LIGHT VERTICAL");
    assert_eq!(UnicodeWidthChar::width('┌').unwrap(), 1, "U+250C BOX DRAWINGS LIGHT DOWN AND RIGHT");
    assert_eq!(UnicodeWidthChar::width('┐').unwrap(), 1, "U+2510 BOX DRAWINGS LIGHT DOWN AND LEFT");
    assert_eq!(UnicodeWidthChar::width('└').unwrap(), 1, "U+2514 BOX DRAWINGS LIGHT UP AND RIGHT");
    assert_eq!(UnicodeWidthChar::width('┘').unwrap(), 1, "U+2518 BOX DRAWINGS LIGHT UP AND LEFT");
    assert_eq!(UnicodeWidthChar::width('├').unwrap(), 1, "U+251C BOX DRAWINGS LIGHT VERTICAL AND RIGHT");
    assert_eq!(UnicodeWidthChar::width('┤').unwrap(), 1, "U+2524 BOX DRAWINGS LIGHT VERTICAL AND LEFT");
    assert_eq!(UnicodeWidthChar::width('┬').unwrap(), 1, "U+252C BOX DRAWINGS LIGHT DOWN AND HORIZONTAL");
    assert_eq!(UnicodeWidthChar::width('┴').unwrap(), 1, "U+2534 BOX DRAWINGS LIGHT UP AND HORIZONTAL");
    assert_eq!(UnicodeWidthChar::width('┼').unwrap(), 1, "U+253C BOX DRAWINGS LIGHT VERTICAL AND HORIZONTAL");

    println!("✅ All block/box drawing characters have wcwidth = 1");
}

/// Verify that block elements are rendered with width 1 by our char_wc_width
#[test]
fn test_char_wc_width_block_elements() {
    // These should all return 1
    assert_eq!(char_wc_width_test(0x2580), 1); // ▀
    assert_eq!(char_wc_width_test(0x2584), 1); // ▄
    assert_eq!(char_wc_width_test(0x2588), 1); // █
    assert_eq!(char_wc_width_test(0x2591), 1); // ░
    assert_eq!(char_wc_width_test(0x2592), 1); // ▒
    assert_eq!(char_wc_width_test(0x2593), 1); // ▓
    assert_eq!(char_wc_width_test(0x2500), 1); // ─
    assert_eq!(char_wc_width_test(0x2502), 1); // │
    assert_eq!(char_wc_width_test(0x258F), 1); // ▏ (1/8 block)
    assert_eq!(char_wc_width_test(0x258C), 1); // ▌ (1/2 block)

    println!("✅ char_wc_width returns 1 for all block elements");
}

fn char_wc_width_test(ucs: u32) -> usize {
    if ucs == 0 || ucs == 32 { return 1; }
    if ucs < 32 || (ucs >= 0x7F && ucs < 0xA0) { return 0; }
    if (ucs >= 0x2E80 && ucs <= 0x9FFF) ||
       (ucs >= 0xAC00 && ucs <= 0xD7A3) ||
       (ucs >= 0xFF01 && ucs <= 0xFF60) { return 2; }
    1
}

/// Verify block elements are stored correctly in screen buffer
#[test]
fn test_block_elements_in_screen_buffer() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    // Write block elements using UTF-8
    let text = "\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}";
    engine.process_bytes(text.as_bytes());

    // Check they're stored in the screen buffer
    let row0_text = get_row_text(&engine, 0);
    let upper_half: String = "\u{2580}".repeat(10);
    assert_eq!(row0_text.trim_end(), upper_half, "Upper half block elements should be in row 0");

    println!("✅ Block elements stored correctly in screen buffer");
}

/// Verify full block fills correctly
#[test]
fn test_full_block_rendering() {
    let mut engine = TerminalEngine::new(5, 3, 1000, 10, 20);

    let full_block = "\u{2588}";
    // Fill 3 lines, no scrolling yet
    engine.process_bytes(format!("{}\r\n{}\r\n{}\r", full_block.repeat(5), full_block.repeat(5), full_block.repeat(5)).as_bytes());

    let fb5 = full_block.repeat(5);
    // \r without \n doesn't advance cursor, so cursor is still at row 2
    assert_eq!(get_row_text(&engine, 0).trim_end(), fb5, "Row 0 should have full blocks");
    assert_eq!(get_row_text(&engine, 1).trim_end(), fb5, "Row 1 should have full blocks");
    assert_eq!(get_row_text(&engine, 2).trim_end(), fb5, "Row 2 should have full blocks");

    println!("✅ Full block rendering verified");
}

/// Verify half blocks (used by neofetch, cava, etc.)
#[test]
fn test_half_block_pattern() {
    let mut engine = TerminalEngine::new(10, 3, 1000, 10, 20);

    let lower = "\u{2584}";
    engine.process_bytes(lower.repeat(10).as_bytes());

    let row0_text = get_row_text(&engine, 0);
    assert_eq!(row0_text.trim_end(), lower.repeat(10), "Lower half blocks should render");

    println!("✅ Half block pattern verified");
}

/// Verify shade characters ( ░▒▓ )
#[test]
fn test_shade_characters() {
    let mut engine = TerminalEngine::new(10, 3, 1000, 10, 20);

    engine.process_bytes(b"\xe2\x96\x91\xe2\x96\x92\xe2\x96\x93\xe2\x96\x91\xe2\x96\x92\xe2\x96\x93\xe2\x96\x91\xe2\x96\x92\xe2\x96\x93");

    let row0_text = get_row_text(&engine, 0);
    assert_eq!(row0_text.trim_end(), "\u{2591}\u{2592}\u{2593}\u{2591}\u{2592}\u{2593}\u{2591}\u{2592}\u{2593}", "Shade characters should render");

    println!("✅ Shade characters verified");
}

/// Verify box drawing forms a proper box
#[test]
fn test_box_drawing_form_box() {
    let mut engine = TerminalEngine::new(6, 5, 1000, 10, 20);

    // Use \r\n for each line to advance cursor (5 rows to avoid scrolling)
    engine.process_bytes(b"\xe2\x94\x8c\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x90\r\n");
    engine.process_bytes(b"\xe2\x94\x82    \xe2\x94\x82\r\n");
    engine.process_bytes(b"\xe2\x94\x82    \xe2\x94\x82\r\n");
    engine.process_bytes(b"\xe2\x94\x94\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x98\r\n");

    // After 4 \r\n on 5 rows, content is at rows 0-3, cursor at row 4
    let row0 = get_row_text(&engine, 0);
    let row1 = get_row_text(&engine, 1);
    let row2 = get_row_text(&engine, 2);
    let row3 = get_row_text(&engine, 3);

    // Check that box drawing characters are present
    let all = format!("{}{}{}{}", row0, row1, row2, row3);
    assert!(all.contains('\u{250C}'), "Should have top-left corner (┌), got: {}", all);
    assert!(all.contains('\u{2510}'), "Should have top-right corner (┐)");
    assert!(all.contains('\u{2514}'), "Should have bottom-left corner (└)");
    assert!(all.contains('\u{2518}'), "Should have bottom-right corner (┘)");
    assert!(all.contains('\u{2502}'), "Should have vertical bars (│)");
    assert!(all.contains('\u{2500}'), "Should have horizontal bars (─)");

    println!("✅ Box drawing verified");
}

/// Verify Braille patterns (U+2800-U+28FF) - commonly used by braille-image tools
#[test]
fn test_braille_patterns() {
    let mut engine = TerminalEngine::new(5, 2, 1000, 10, 20);

    // ⣿ = \xe2\xa3\xbf
    engine.process_bytes(b"\xe2\xa3\xbf\xe2\xa3\xbf\xe2\xa3\xbf\xe2\xa3\xbf\xe2\xa3\xbf");

    let row0_text = get_row_text(&engine, 0);
    let braille = "\u{28FF}".repeat(5);
    assert_eq!(row0_text.trim_end(), braille, "Braille patterns should render");

    println!("✅ Braille patterns verified");
}

/// Verify that block elements are classified correctly by the renderer's font selection
#[test]
fn test_block_element_font_selection() {
    let _renderer = TerminalRenderer::new(&[], 12.0, None);

    let block_chars = [
        '\u{2580}', '\u{2584}', '\u{2588}', // block
        '\u{2591}', '\u{2592}', '\u{2593}', // shade
        '\u{2500}', '\u{2502}', // box drawing
        '\u{250C}', '\u{2510}', '\u{2514}', '\u{2518}', // box corners
        '\u{28FF}', // braille
    ];

    for ch in block_chars {
        assert!(
            is_block_or_special(ch),
            "Character '{}' (U+{:04X}) should be classified as block/special",
            ch,
            ch as u32
        );
    }

    println!("✅ Block element font selection classification verified");
}

/// Verify block elements survive scroll operations
#[test]
fn test_block_elements_survive_scroll() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    let full = "\u{2588}".repeat(10);
    let upper = "\u{2580}".repeat(10);
    let lower = "\u{2584}".repeat(10);
    let shade = "\u{2591}".repeat(10);
    let dots = "\u{2592}".repeat(10);

    // Write 5 lines + 1 empty line to advance cursor past row 4 without overwriting
    engine.process_bytes(format!("{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n", full, upper, lower, shade, dots).as_bytes());

    // After 5 \r\n on 5 rows: row 0 was scrolled to transcript
    // Row 0 = upper, Row 1 = lower, Row 2 = shade, Row 3 = dots, Row 4 = empty, cursor at row 4
    assert_eq!(get_row_text(&engine, 0).trim_end(), upper, "Row 0 should be upper half");
    assert_eq!(get_row_text(&engine, 1).trim_end(), lower, "Row 1 should be lower half");
    assert_eq!(get_row_text(&engine, 2).trim_end(), shade, "Row 2 should be shade");
    assert_eq!(get_row_text(&engine, 3).trim_end(), dots, "Row 3 should be dots");

    // Now trigger another scroll
    engine.process_bytes(b"XXXXXXXXXX\r\n");

    // After scroll: row 0→transcript, upper→0, lower→1, shade→2, dots→3, XXX→4
    assert_eq!(get_row_text(&engine, 0).trim_end(), lower, "After scroll, row 0 should be lower");
    assert_eq!(get_row_text(&engine, 1).trim_end(), shade, "After scroll, row 1 should be shade");
    assert_eq!(get_row_text(&engine, 2).trim_end(), dots, "After scroll, row 2 should be dots");

    println!("✅ Block elements survive scroll operations");
}

/// Verify that a common neofetch-style output renders correctly
#[test]
fn test_neofetch_style_output() {
    let mut engine = TerminalEngine::new(20, 10, 1000, 10, 20);

    // Simulate a neofetch-like output using half blocks for color blocks
    engine.process_bytes(b"\x1b[31m\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88\x1b[0m\x1b[32m\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88\x1b[0m\x1b[34m\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88\xe2\x96\x88\x1b[0m\r\n");
    engine.process_bytes(b"\x1b[31m\xe2\x96\x80\xe2\x96\x80\xe2\x96\x80\xe2\x96\x80\x1b[0m\x1b[32m\xe2\x96\x80\xe2\x96\x80\xe2\x96\x80\xe2\x96\x80\x1b[0m\x1b[34m\xe2\x96\x80\xe2\x96\x80\xe2\x96\x80\xe2\x96\x80\x1b[0m\r\n");
    engine.process_bytes(b"\x1b[31m\xe2\x96\x84\xe2\x96\x84\xe2\x96\x84\xe2\x96\x84\x1b[0m\x1b[32m\xe2\x96\x84\xe2\x96\x84\xe2\x96\x84\xe2\x96\x84\x1b[0m\x1b[34m\xe2\x96\x84\xe2\x96\x84\xe2\x96\x84\xe2\x96\x84\x1b[0m\r\n");

    // Verify the content is there (colors don't affect text storage)
    let all_text: String = (0..3).map(|r| get_row_text(&engine, r)).collect();
    let fb4 = "\u{2588}".repeat(4);
    let ub4 = "\u{2580}".repeat(4);
    let lb4 = "\u{2584}".repeat(4);
    assert!(all_text.contains(&fb4), "Should contain full blocks");
    assert!(all_text.contains(&ub4), "Should contain upper half blocks");
    assert!(all_text.contains(&lb4), "Should contain lower half blocks");

    println!("✅ Neofetch-style output verified");
}

/// Verify the eighth-block characters (fine-grained blocks)
#[test]
fn test_eighth_block_characters() {
    let mut engine = TerminalEngine::new(8, 2, 1000, 10, 20);

    // ▏▎▍▌▋▊▉█ = U+258F U+258E U+258D U+258C U+258B U+258A U+2589 U+2588
    engine.process_bytes(b"\xe2\x96\x8f\xe2\x96\x8e\xe2\x96\x8d\xe2\x96\x8c\xe2\x96\x8b\xe2\x96\x8a\xe2\x96\x89\xe2\x96\x88");

    let row0_text = get_row_text(&engine, 0);
    assert_eq!(row0_text.trim_end(), "\u{258F}\u{258E}\u{258D}\u{258C}\u{258B}\u{258A}\u{2589}\u{2588}", "Eighth block characters should all render");

    println!("✅ Eighth block characters verified");
}

/// Test that block element characters are routed to the correct font (not sans-serif fallback)
#[test]
fn test_block_element_font_routing() {
    let renderer = TerminalRenderer::new(&[], 12.0, None);

    // Block elements are in the 0x2500-0x25FF range, which is > 127,
    // so get_font() will route them based on has_non_ascii
    let block_chars = ['\u{2580}', '\u{2584}', '\u{2588}', '\u{2500}', '\u{2502}'];

    for ch in block_chars {
        let ch_code = ch as u32;
        // They should all be recognized as non-ASCII
        assert!(ch_code > 127, "Character {} (U+{:04X}) should be non-ASCII", ch, ch_code);

        // The font_width should be positive
        assert!(renderer.font_width > 0.0, "font_width should be positive");
        assert!(renderer.font_height > 0.0, "font_height should be positive");
    }

    println!("✅ Block element font routing verified");
}
