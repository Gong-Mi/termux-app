// Carriage Return and Line Feed tests
// Run: cargo test --test crlf_test -- --nocapture
//
// These tests verify that LF (0x0A) and CR (0x0D) are handled correctly
// according to VT100 standard behavior, matching upstream Java.
//
// Key distinction:
// - LF (0x0A): Move cursor DOWN one row, do NOT return to column 0
// - CR (0x0D): Move cursor to column 0 (left margin), do NOT move down
// - LNM (DECSET 20) is not implemented, so LF never includes implicit CR

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

/// Test: LF should move cursor down WITHOUT returning to column 0
/// This matches upstream Java: doLinefeed() only changes row, not col
#[test]
fn test_lf_does_not_return_to_origin() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    // Fill first row to trigger about_to_wrap
    engine.process_bytes(b"1234567890");
    
    // Cursor should be at row 0, col 9 (about_to_wrap = true)
    assert_eq!(engine.state.cursor.y, 0);
    assert_eq!(engine.state.cursor.x, 9);
    assert!(engine.state.cursor.about_to_wrap);

    // Send LF
    engine.process_bytes(b"\n");

    // Cursor should be at row 1, col 9 (NOT col 0)
    // This matches upstream: doLinefeed() doesn't change column
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 9, "LF should NOT return cursor to column 0");
    assert!(!engine.state.cursor.about_to_wrap);

    println!("✅ LF does not return to origin (matches upstream)");
}

/// Test: CR should return cursor to start of line WITHOUT moving down
#[test]
fn test_cr_returns_to_start_without_moving() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"Hello");
    assert_eq!(engine.state.cursor.y, 0);
    assert_eq!(engine.state.cursor.x, 5);

    engine.process_bytes(b"\r");
    assert_eq!(engine.state.cursor.x, 0);
    assert_eq!(engine.state.cursor.y, 0, "CR should NOT change row");
    assert!(!engine.state.cursor.about_to_wrap);

    // Overwrite with new text
    engine.process_bytes(b"World");
    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim(), "World", "Row 0 should be 'World', got: '{}'", row0.trim());

    println!("✅ CR returns to start without moving down");
}

/// Test: Standard CRLF sequence (\r\n) works correctly
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

/// Test: CR at end of line followed by text overwrites from beginning
#[test]
fn test_cr_then_text() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"Hello\rWorld");
    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim(), "World", "CR+text should overwrite from start");

    println!("✅ CR then text overwrites correctly");
}

/// Test: Multiple LFs in a row
#[test]
fn test_multiple_lfs() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"A\n\n\n");

    // A at row 0, then 3 LFs → cursor at row 3, same column (0)
    assert_eq!(engine.state.cursor.y, 3);
    assert_eq!(engine.state.cursor.x, 1); // 'A' was at col 0, then x advanced to 1

    println!("✅ Multiple LFs work correctly");
}

/// Test: LF does not trigger extra wrap when followed by printable char
#[test]
fn test_lf_then_printable_no_extra_wrap() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    // Fill first row to trigger about_to_wrap
    engine.process_bytes(b"1234567890");
    assert!(engine.state.cursor.about_to_wrap);
    assert_eq!(engine.state.cursor.x, 9);

    // Send LF
    engine.process_bytes(b"\n");
    assert!(!engine.state.cursor.about_to_wrap);
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 9); // LF doesn't change column

    // Print 'A' at col 9 (last column)
    // Since cursor is already at last column, this overwrites col 9 and sets about_to_wrap
    // It does NOT advance to next line (cursor stays at x=9, about_to_wrap=true)
    engine.process_bytes(b"A");
    
    // Cursor stays at same position, just sets about_to_wrap=true
    assert_eq!(engine.state.cursor.y, 1, "Cursor should stay on row 1");
    assert_eq!(engine.state.cursor.x, 9, "Cursor should stay at col 9 (last col)");
    assert!(engine.state.cursor.about_to_wrap, "Should be marked as wrapped");
    
    // Verify 'A' is at row 1, col 9 (overwritten the placeholder)
    let row1 = get_row_text(&engine, 1);
    assert_eq!(row1.chars().nth(9), Some('A'), "'A' should be at row 1, col 9");

    println!("✅ LF then printable does not cause extra wrap");
}

/// Test: about_to_wrap cleared by LF
#[test]
fn test_lf_clears_about_to_wrap() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"1234567890");
    assert!(engine.state.cursor.about_to_wrap, "Should be wrapped after filling row");

    engine.process_bytes(b"\n");
    assert!(!engine.state.cursor.about_to_wrap, "LF should clear about_to_wrap");

    println!("✅ LF clears about_to_wrap");
}

/// Test: VT (0x0B) and FF (0x0C) behave like LF
#[test]
fn test_vt_ff_like_lf() {
    let mut engine = TerminalEngine::new(10, 5, 1000, 10, 20);

    engine.process_bytes(b"Hello");
    assert_eq!(engine.state.cursor.x, 5);
    assert_eq!(engine.state.cursor.y, 0);

    // VT (vertical tab)
    engine.process_bytes(b"\x0b");
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 5, "VT should not change column");

    // FF (form feed)
    engine.process_bytes(b"\x0c");
    assert_eq!(engine.state.cursor.y, 2);
    assert_eq!(engine.state.cursor.x, 5, "FF should not change column");

    println!("✅ VT and FF behave like LF (no column change)");
}
