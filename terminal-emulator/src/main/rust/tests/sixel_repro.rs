use termux_rust::engine::TerminalEngine;

#[test]
fn test_sixel_extended_parsing() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // Some sixel encoders send "1;1;100;100 at the start.
    // If our parser ignores ", but doesn't ignore the digits following it
    // then it might try to render those digits as sixel data!
    // '1' is ASCII 49. 49 is NOT in 63..126. So it would be ignored by _ => {}.

    // Wait, what if the bug is that it DOES process some characters it shouldn't?
    // Let's try sending digits and see if width changes.
    engine.process_bytes(b"\x1bPq\"1;1;100;100~\x1b\\");
    let d = &engine.state.sixel_decoder;
    println!(
        "Actual sixel width (with ignored quote/digits): {}",
        d.width
    );
}

#[test]
fn test_sixel_with_high_ascii() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    // ASCII 100 is 'd'. 100-63 = 37.
    // If someone sends 'd' (100) and it's misparsed?
    engine.process_bytes(b"\x1bPqd\x1b\\");
    let d = &engine.state.sixel_decoder;
    println!("Width for 'd': {}", d.width);
}
