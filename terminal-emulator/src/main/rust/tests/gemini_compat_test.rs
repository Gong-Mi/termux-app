// Protocol contracts for sequences used by Gemini CLI.
// EL and DA references: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
// These are engine tests, not evidence of a full Gemini application session.
use termux_rust::terminal::style::EFFECT_UNDERLINE;
use termux_rust::TerminalEngine;

fn get_screen_as_text(engine: &TerminalEngine) -> Vec<String> {
    (0..engine.state.rows)
        .map(|row| {
            let mut text = vec![0u16; engine.state.cols as usize];
            engine.state.copy_row_text(row, &mut text);
            String::from_utf16_lossy(&text).trim_end().to_string()
        })
        .collect()
}

#[test]
fn keyboard_probe_does_not_enable_sgr_underline() {
    let mut engine = TerminalEngine::new(0, 80, 10, 100, 10, 20);
    assert_eq!(engine.state.effect & EFFECT_UNDERLINE, 0);
    engine.process_bytes(b"\x1b[>4;2m");
    assert_eq!(engine.state.effect & EFFECT_UNDERLINE, 0);
}

#[test]
fn erase_line_preserves_other_rows_and_carriage_return_resets_column() {
    let mut engine = TerminalEngine::new(0, 80, 10, 100, 10, 20);
    engine.process_bytes(b"OLD LINE 1\r\nOLD LINE 2\r\nOLD LINE 3");
    // Move to the middle row, away from column zero.
    engine.process_bytes(b"\x1b[2;5H");
    let before = get_screen_as_text(&engine);
    assert_eq!(&before[..3], &["OLD LINE 1", "OLD LINE 2", "OLD LINE 3"]);

    // CSI 2 K erases the current line, not the display or previous rows.
    engine.process_bytes(b"\x1b[2K\r");
    let after = get_screen_as_text(&engine);
    assert_eq!(after[1], "");
    for row in 0..after.len() {
        if row != 1 {
            assert_eq!(after[row], before[row], "EL modified row {row}");
        }
    }
    engine.process_bytes(b"NEW");
    assert_eq!(get_screen_as_text(&engine)[1], "NEW");
}

#[test]
fn secondary_device_attributes_produces_exactly_one_da2_response() {
    for query in [b"\x1b[>c".as_slice(), b"\x1b[>0c".as_slice()] {
        for chunk_size in 1..=query.len() {
            let mut engine = TerminalEngine::new(0, 80, 10, 100, 10, 20);
            assert!(engine.state.pending_responses.is_empty());
            for chunk in query.chunks(chunk_size) {
                engine.process_bytes(chunk);
            }
            // Characterize the identity currently advertised by this engine;
            // this does not assert that these version numbers are universal.
            assert_eq!(engine.state.pending_responses, ["\x1b[>41;320;0c"]);
            engine.state.pending_responses.clear();
            engine.process_bytes(b"text");
            assert!(engine.state.pending_responses.is_empty());
        }
    }
}
