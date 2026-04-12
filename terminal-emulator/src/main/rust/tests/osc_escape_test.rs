// OSC Escape Sequence parsing tests
// Run: cargo test --test osc_escape_test -- --nocapture
//
// These tests verify that OSC (Operating System Command) sequences
// like setting the window title do NOT leak the String Terminator '\'
// character onto the screen, especially when the sequence is split
// across multiple process_bytes calls (simulating PTY chunked reads).

use termux_rust::TerminalEngine;

fn get_row_text(engine: &TerminalEngine, row: i32) -> String {
    let cols = engine.state.cols as usize;
    let mut text = vec![0u16; cols];
    engine.state.copy_row_text(row, &mut text);
    String::from_utf16_lossy(&text).replace('\0', " ")
}

/// Test: Send a complete OSC sequence in one go.
/// Verify that NO '\' appears on the screen.
#[test]
fn test_osc_sequence_complete() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // \x1b]0;My Title\x1b\  (ESC ] 0 ; My Title ESC \)
    engine.process_bytes(b"\x1b]0;My Title\x1b\\");

    // The screen should be empty (only title changed)
    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim(), "", "OSC sequence should NOT print characters");

    // Verify title was actually set (if accessible, otherwise just check screen is clean)
    // For this test, screen cleanliness is the key indicator.
    println!("✅ Complete OSC sequence handled correctly, screen is clean");
}

/// Test: Send an OSC sequence split across TWO chunks.
/// This simulates PTY behavior where the read buffer splits the String Terminator.
/// Chunk 1: \x1b]0;Title\x1b (Ends with ESC)
/// Chunk 2: \ (The String Terminator backslash)
#[test]
fn test_osc_sequence_split_st() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // Simulate Chunk 1: Everything up to the final backslash
    engine.process_bytes(b"\x1b]0;Title\x1b");

    // Simulate Chunk 2: The String Terminator backslash
    engine.process_bytes(b"\\");

    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim(), "", "Split OSC sequence should NOT print backslash");

    println!("✅ Split OSC sequence handled correctly, screen is clean");
}

/// Test: Simulate the exact user scenario:
/// Shell sets title to "/data/user/0" via OSC, then prints prompt.
#[test]
fn test_osc_title_with_path_prompt() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // 1. Set title to path (simulating PS1 title update)
    // Split it to trigger the bug if present:
    engine.process_bytes(b"\x1b]0;/data/user/0\x1b");
    engine.process_bytes(b"\\");

    // 2. Print the actual prompt text
    engine.process_bytes(b"$ ");

    let row0 = get_row_text(&engine, 0);
    
    // The row should only contain "$ " (plus trailing spaces)
    // It should NOT start with "\/data..." or "\$ "
    assert!(
        row0.starts_with("$ "),
        "Row should start with '$ ', but got: '{}'", 
        row0.trim()
    );
    assert!(
        !row0.contains('\\'),
        "Row should NOT contain backslash from OSC sequence, got: '{}'",
        row0.trim()
    );

    println!("✅ Title+Prompt scenario handled correctly");
}

/// Test: What if the split happens BEFORE the ESC?
/// Chunk 1: \x1b]0;Title
/// Chunk 2: \x1b\
#[test]
fn test_osc_sequence_split_before_esc() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    engine.process_bytes(b"\x1b]0;Title");
    engine.process_bytes(b"\x1b\\");

    let row0 = get_row_text(&engine, 0);
    assert_eq!(row0.trim(), "", "Split before ESC should NOT print backslash");

    println!("✅ Split before ESC handled correctly");
}

/// Test: Nested/Complex characters in title (like slashes in path)
/// Verify they don't break the parser state.
#[test]
fn test_osc_title_with_special_chars() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);

    // Title containing slashes and backslashes
    engine.process_bytes(b"\x1b]0;C:\\Windows\\System32\x1b\\");
    
    // Then print something
    engine.process_bytes(b"Hello");

    let row0 = get_row_text(&engine, 0);
    assert!(
        row0.starts_with("Hello"),
        "Row should start with 'Hello', got: '{}'",
        row0.trim()
    );
    // Ensure the title content didn't leak
    assert!(
        !row0.contains("Windows"),
        "Title content should not leak to screen"
    );

    println!("✅ Special chars in title handled correctly");
}
