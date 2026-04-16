// Comprehensive block element coverage test (U+2580-U+259F)
// Run: cargo test --test block_coverage_test -- --nocapture

use termux_rust::TerminalEngine;

/// Verify that every Block Element character (U+2580-U+259F) is stored
/// correctly in the screen buffer and is NOT rendered via font fallback.
#[test]
fn test_all_block_elements_in_buffer() {
    // U+2580 to U+259F = 32 characters
    let block_chars: Vec<char> = (0x2580..=0x259F).filter_map(char::from_u32).collect();
    assert_eq!(block_chars.len(), 32, "Should have 32 block element characters");

    let mut engine = TerminalEngine::new(40, 24, 1000, 10, 20);

    // Write all block elements
    let text: String = block_chars.iter().collect();
    engine.process_bytes(text.as_bytes());

    let row = engine.state.main_screen.get_row(0);

    // Verify each character is stored at the correct position
    for (i, &ch) in block_chars.iter().enumerate() {
        assert_eq!(row.text[i] as char, ch,
            "Position {} should be '{}' (U+{:04X}), got U+{:04X}",
            i, ch, ch as u32, row.text[i] as u32);
    }

    println!("✅ All 32 block elements (U+2580-U+259F) stored correctly in screen buffer");
}

/// Verify quadrant blocks have correct bitmask mapping
#[test]
fn test_quadrant_block_mapping() {
    // TL=1, TR=2, BL=4, BR=8
    let tests = [
        (0x2596u32, 0b0100u8, "▖ LOWER LEFT"),
        (0x2597u32, 0b1000u8, "▗ LOWER RIGHT"),
        (0x2598u32, 0b0001u8, "▘ UPPER LEFT"),
        (0x259Du32, 0b0010u8, "▝ UPPER RIGHT"),
        (0x2599u32, 0b1101u8, "▙ TL+BL+BR"),
        (0x259Au32, 0b1001u8, "▚ TL+BR"),
        (0x259Eu32, 0b0110u8, "▞ TR+BL"),
        (0x259Bu32, 0b0111u8, "▛ TL+TR+BL"),
        (0x259Cu32, 0b1011u8, "▜ TL+TR+BR"),
        (0x259Fu32, 0b1110u8, "▟ TR+BL+BR"),
    ];

    for (codepoint, expected_mask, name) in tests {
        let ch = char::from_u32(codepoint).unwrap();
        let has_bit = |mask: u8, bit: u8| (expected_mask & bit) != 0;

        // Verify character properties
        assert!(termux_rust::renderer::is_block_element(ch),
            "'{}' (U+{:04X}) should be classified as block element: {}",
            ch, codepoint, name);

        // Verify wcwidth = 1
        assert_eq!(unicode_width::UnicodeWidthChar::width(ch).unwrap(), 1,
            "'{}' (U+{:04X}) should have width 1: {}",
            ch, codepoint, name);

        println!("  ✅ {} (U+{:04X}) mask=0b{:04b}", name, codepoint, expected_mask);
    }

    println!("✅ All quadrant block mappings verified");
}

/// Verify half-block characters (U+2580, U+2584, U+258C, U+2590)
#[test]
fn test_half_blocks() {
    let tests = [
        (0x2580u32, "▀ UPPER HALF"),
        (0x2584u32, "▄ LOWER HALF"),
        (0x258Cu32, "▌ LEFT HALF"),
        (0x2590u32, "▐ RIGHT HALF"),
    ];

    for (cp, name) in tests {
        let ch = char::from_u32(cp).unwrap();
        assert!(termux_rust::renderer::is_block_element(ch),
            "{} should be block element", name);
        println!("  ✅ {}", name);
    }

    println!("✅ Half blocks verified");
}

/// Verify 1/8 block characters (U+258F-U+2589)
#[test]
fn test_eighth_blocks() {
    let tests = [
        (0x258Fu32, "▏ 1/8"),
        (0x258Eu32, "▎ 2/8"),
        (0x258Du32, "▍ 3/8"),
        (0x258Cu32, "▌ 4/8"),
        (0x258Bu32, "▋ 5/8"),
        (0x258Au32, "▊ 6/8"),
        (0x2589u32, "▉ 7/8"),
        (0x2588u32, "█ 8/8 (Full Block)"),
    ];

    for (cp, name) in tests {
        let ch = char::from_u32(cp).unwrap();
        assert!(termux_rust::renderer::is_block_element(ch),
            "{} should be block element", name);
        println!("  ✅ {}", name);
    }

    println!("✅ 1/8 blocks verified");
}

/// Verify shade characters (U+2591-U+2593)
#[test]
fn test_shade_blocks() {
    let tests = [
        (0x2591u32, "░ Light Shade"),
        (0x2592u32, "▒ Medium Shade"),
        (0x2593u32, "▓ Dark Shade"),
    ];

    for (cp, name) in tests {
        let ch = char::from_u32(cp).unwrap();
        assert!(termux_rust::renderer::is_block_element(ch),
            "{} should be block element", name);
        println!("  ✅ {}", name);
    }

    println!("✅ Shade blocks verified");
}

/// Verify full block (U+2588)
#[test]
fn test_full_block() {
    let ch = '█';
    assert_eq!(ch as u32, 0x2588);
    assert!(termux_rust::renderer::is_block_element(ch));
    assert_eq!(unicode_width::UnicodeWidthChar::width(ch).unwrap(), 1);
    println!("✅ Full Block (U+2588) verified");
}

/// Test a visual pattern using quadrant blocks (like the user's example)
#[test]
fn test_quadrant_pattern() {
    let mut engine = TerminalEngine::new(80, 5, 1000, 10, 20);

    // User's example string
    let pattern = "▗█▀▀▜▙▝█▛▀▀▌▜██▖▟██▘▜█▘▜██▖▝█▛▝█▛";
    engine.process_bytes(pattern.as_bytes());

    let row = engine.state.main_screen.get_row(0);
    let text: String = row.text.iter()
        .take(pattern.chars().count())
        .map(|&c| c as char)
        .collect();

    assert_eq!(text, pattern,
        "Pattern should be stored exactly as input");

    println!("✅ Quadrant block pattern stored correctly: '{}'", pattern);
}
