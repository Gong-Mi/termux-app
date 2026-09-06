use termux_rust::vte_parser::{Params, Parser, Perform};

#[derive(Debug, Default, PartialEq)]
struct Events(Vec<String>);

impl Perform for Events {
    fn print(&mut self, c: char) {
        self.0.push(format!("print:{c}"));
    }

    fn execute(&mut self, b: u8) {
        self.0.push(format!("execute:{b}"));
    }

    fn esc_dispatch(&mut self, i: &[u8], ignore: bool, b: u8) {
        self.0.push(format!("esc:{i:?}:{ignore}:{b}"));
    }

    fn csi_dispatch(&mut self, p: &Params, i: &[u8], ignore: bool, c: char) {
        self.0.push(format!(
            "csi:{:?}:{i:?}:{ignore}:{c}",
            &p.values[..p.len]
        ));
    }

    fn osc_dispatch(&mut self, p: &[&[u8]], bell: bool) {
        self.0.push(format!("osc:{p:?}:{bell}"));
    }
}

fn parse(chunks: &[&[u8]]) -> Events {
    let mut parser = Parser::new();
    let mut events = Events::default();
    for chunk in chunks {
        parser.advance(&mut events, chunk);
    }
    events
}

fn all_splits(bytes: &[u8], expected: &Events) {
    for split in 0..=bytes.len() {
        assert_eq!(
            &parse(&[&bytes[..split], &bytes[split..]]),
            expected,
            "split={split}, bytes={bytes:?}"
        );
    }
    assert_eq!(
        &parse(&bytes.chunks(1).collect::<Vec<_>>()),
        expected,
        "byte-by-byte: {bytes:?}"
    );
}

#[test]
fn two_three_four_byte_characters_survive_every_split() {
    for text in [
        "é",
        "中",
        "😀",
        "tail-中文",
        "é中😀Z",
        "\u{80}\u{800}\u{10000}\u{10ffff}",
    ] {
        let expected = Events(text.chars().map(|c| format!("print:{c}")).collect());
        all_splits(text.as_bytes(), &expected);
    }
}

#[test]
fn utf8_interleaved_with_csi_osc_and_controls_is_chunk_invariant() {
    let text = "é\x1b[31m中\x1b]0;标题😀\x07Z\n\x1b[0m😀\x1b]2;é中\x1b\\!";
    let expected = parse(&[text.as_bytes()]);
    assert!(expected.0.contains(&"csi:[31]:[]:false:m".to_owned()));
    assert!(expected.0.contains(&"print:中".to_owned()));
    let osc_events: Vec<_> = expected.0.iter().filter(|e| e.starts_with("osc:")).collect();
    assert_eq!(
        osc_events,
        vec![
            &format!("osc:{:?}:true", [b"0".as_slice(), "标题😀".as_bytes()]),
            &format!("osc:{:?}:true", [b"2".as_slice(), "é中".as_bytes()]),
        ]
    );
    all_splits(text.as_bytes(), &expected);
}

#[test]
fn invalid_bytes_never_swallow_ascii_escape_or_controls() {
    for bad in [
        &b"\xff"[..],
        b"\x80",
        b"\xc0\xaf",
        b"\xe4",
        b"\xe4\xb8",
        b"\xf0\x9f\x98",
        b"\xe0\x80",
        b"\xed\xa0\x80",
        b"\xf4\x90\x80\x80",
        b"\xc2\xc3\xa9",
    ] {
        for suffix in [&b"A\x1b[31mZ\n"[..], b"\x1b[31mZ\n"] {
            let mut bytes = bad.to_vec();
            bytes.extend_from_slice(suffix);
            // Independent UTF-8 replacement oracle, then existing valid protocol.
            let decoded = String::from_utf8_lossy(&bytes);
            let expected = parse(&[decoded.as_bytes()]);
            all_splits(&bytes, &expected);
        }
        // OSC forces the state-machine/scalar path even on SVE hosts.
        let mut osc = b"\x1b]0;".to_vec();
        osc.extend_from_slice(bad);
        osc.extend_from_slice(b"ASCII\x07Q");
        let decoded = String::from_utf8_lossy(&osc);
        all_splits(&osc, &parse(&[decoded.as_bytes()]));
    }
}

#[test]
fn truncated_character_waits_across_empty_advance() {
    let mut p = Parser::new();
    let mut events = Events::default();
    p.advance(&mut events, b"tail-\xe4");
    p.advance(&mut events, b"");
    assert_eq!(events, parse(&[b"tail-"]));
    p.advance(&mut events, b"\xb8\xad\xe6\x96\x87");
    assert_eq!(events, parse(&["tail-中文".as_bytes()]));
}

#[test]
fn long_valid_prefix_and_invalid_tail_preserve_boundaries() {
    let text = format!("{}中😀\x1b[2Aé", "x".repeat(1023));
    all_splits(text.as_bytes(), &parse(&[text.as_bytes()]));
    let mut bytes = text.into_bytes();
    bytes.extend_from_slice(b"\xff\x1b[3AZ");
    let decoded = String::from_utf8_lossy(&bytes);
    all_splits(&bytes, &parse(&[decoded.as_bytes()]));
}

#[test]
fn malformed_streams_match_lossy_decode_without_swallowing_following_ascii() {
    let mut seed = 0x91d6_273bu32;
    for _ in 0..4096 {
        let mut bytes = Vec::with_capacity(33);
        for _ in 0..32 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            bytes.push((seed >> 24) as u8);
        }
        // Force any incomplete scalar to resolve, without adding an EOF policy.
        bytes.push(b'~');
        let decoded = String::from_utf8_lossy(&bytes);
        let expected = parse(&[decoded.as_bytes()]);
        assert_eq!(parse(&[&bytes]), expected, "whole input: {bytes:?}");
        assert_eq!(parse(&bytes.chunks(1).collect::<Vec<_>>()), expected, "fragmented: {bytes:?}");
    }
}

#[test]
fn sve_scanner_matches_scalar_at_unaligned_and_short_tail_boundaries() {
    #[cfg(target_arch = "aarch64")]
    {
        if !termux_rust::vte_sve::has_sve_support() {
            eprintln!("SKIP direct SVE scan: host has no SVE");
            return;
        }
        for len in 0..=512 {
            for offset in [0, 1, 15, 16, 31] {
                let mut bytes = vec![0xa5; offset + len].into_boxed_slice();
                let data = &mut bytes[offset..];
                assert_eq!(unsafe { termux_rust::vte_sve::find_first_control_sve(data) }, len);
                if len != 0 {
                    for index in [0, len / 2, len - 1] {
                        for control in [0, 31, 127] {
                            data[index] = control;
                            assert_eq!(unsafe { termux_rust::vte_sve::find_first_control_sve(data) }, index);
                            data[index] = 0xa5;
                        }
                    }
                }
            }
        }
        eprintln!("PASS direct hardware SVE scan lengths 0..=512 and unaligned tails");
    }
    #[cfg(not(target_arch = "aarch64"))]
    eprintln!("SKIP direct SVE scan: non-AArch64 host; parser scalar tests still run");
}
