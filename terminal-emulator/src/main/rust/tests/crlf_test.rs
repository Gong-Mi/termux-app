// Carriage Return and Line Feed tests
// Run: cargo test --test crlf_test -- --nocapture
//
// These tests verify that LF (0x0A) and CR (0x0D) are handled correctly
// and do not cause extra blank lines or cursor misplacement.

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

/// Test: LF should move cursor down WITHOUT triggering an extra wrap on next print
#[test]
fn test_lf_does_not_trigger_extra_wrap() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    // Fill first row to trigger about_to_wrap
    engine.process_bytes(b"1234567890");
    
    println!("DEBUG: After '1234567890': y={}, x={}, about_to_wrap={}", 
        engine.state.cursor.y, engine.state.cursor.x, engine.state.cursor.about_to_wrap);
    
    // Cursor should be at row 0, col 10 (about_to_wrap = true)
    assert_eq!(engine.state.cursor.y, 0);
    assert!(engine.state.cursor.about_to_wrap, "Cursor should be wrapped after filling row");

    // Send LF
    engine.process_bytes(b"\n");

    println!("DEBUG: After '\\n': y={}, x={}, about_to_wrap={}", 
        engine.state.cursor.y, engine.state.cursor.x, engine.state.cursor.about_to_wrap);

    // Cursor should be at row 1, col 0
    // about_to_wrap should be FALSE
    assert_eq!(engine.state.cursor.y, 1);
    assert!(!engine.state.cursor.about_to_wrap, "LF should clear about_to_wrap");

    // Print 'A' - should appear at row 1, col 0 (NOT row 2)
    engine.process_bytes(b"A");
    
    println!("DEBUG: After 'A': y={}, x={}, about_to_wrap={}", 
        engine.state.cursor.y, engine.state.cursor.x, engine.state.cursor.about_to_wrap);
    
    assert_eq!(engine.state.cursor.y, 1, "Cursor Y should be 1 after printing A");
    assert_eq!(engine.state.cursor.x, 1);
    
    let row1 = get_row_text(&engine, 1);
    assert!(row1.starts_with('A'), "Row 1 should start with 'A', got: '{}'", row1.trim());
    
    // Row 2 should be empty
    let row2 = get_row_text(&engine, 2);
    assert_eq!(row2.trim(), "", "Row 2 should be empty, got: '{}'", row2.trim());

    println!("✅ LF does not trigger extra wrap");
}

/// Test: CR should return cursor to start of line
#[test]
fn test_cr_returns_to_start() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"Hello");
    assert_eq!(engine.state.cursor.x, 5);

    engine.process_bytes(b"\r");
    assert_eq!(engine.state.cursor.x, 0);
    assert!(!engine.state.cursor.about_to_wrap);

    // Overwrite with new text
    engine.process_bytes(b"World");
    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim(), "World", "Row 0 should be 'World', got: '{}'", row0.trim());

    println!("✅ CR returns to start correctly");
}

/// Test: Standard CRLF sequence (\r\n)
#[test]
fn test_crlf_sequence() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"Line1\r\n");
    engine.process_bytes(b"Line2\r\n");

    assert_eq!(engine.state.cursor.y, 2);
    assert_eq!(engine.state.cursor.x, 0);

    let row0 = get_row_text(&engine, 0);
    let row1 = get_row_text(&engine, 1);
    
    assert_eq!(row0.trim(), "Line1", "Row 0 should be 'Line1'");
    assert_eq!(row1.trim(), "Line2", "Row 1 should be 'Line2'");

    println!("✅ CRLF sequence works correctly");
}

/// Test: LF alone should NOT cause double spacing
#[test]
fn test_lf_no_double_spacing() {
    let mut engine = TerminalEngine::new(80, 10, 1000, 10, 20);

    // Simulate shell output: text followed by LF (common in many terminals)
    engine.process_bytes(b"Prompt> ");
    engine.process_bytes(b"\n");
    engine.process_bytes(b"Next line");

    let row0 = get_row_text(&engine, 0);
    let row1 = get_row_text(&engine, 1);
    
    assert_eq!(row0.trim(), "Prompt>", "Row 0 should contain 'Prompt>'");
    assert_eq!(row1.trim(), "Next line", "Row 1 should contain 'Next line'");
    
    // Row 2 should be empty (no extra blank line)
    let row2 = get_row_text(&engine, 2);
    assert_eq!(row2.trim(), "", "Row 2 should be empty (no double spacing)");

    println!("✅ LF does not cause double spacing");
}

/// Test: Multiple LFs in a row
#[test]
fn test_multiple_lfs() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"A\n\n\nB");

    // A at row 0, then 3 LFs → cursor at row 3, then B at row 3
    assert_eq!(engine.state.cursor.y, 3);
    assert_eq!(engine.state.cursor.x, 1);

    let row0 = get_row_text(&engine, 0);
    let row3 = get_row_text(&engine, 3);
    
    assert_eq!(row0.trim(), "A", "Row 0 should be 'A'");
    assert!(row3.starts_with('B'), "Row 3 should start with 'B'");

    println!("✅ Multiple LFs work correctly");
}
