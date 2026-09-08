//! Characterization of the existing Unicode-15 width policy, not a table update.
use termux_rust::wcwidth::wcwidth;

#[test]
fn special_zero_width_and_control_boundaries() {
    for (value, expected) in [
        (0, 0),
        (31, 0),
        (32, 1),
        (0x7e, 1),
        (0x7f, 0),
        (0x9f, 0),
        (0xa0, 1),
        (0x200a, 1),
        (0x200b, 0),
        (0x200f, 0),
        (0x2010, 1),
        (0x2027, 1),
        (0x2028, 0),
        (0x2029, 0),
        (0x202a, 0),
        (0x202e, 0),
        (0x202f, 1),
        (0x205f, 1),
        (0x2060, 0),
        (0x2063, 0),
        (0x2064, 1),
        (0x4e00, 2),
        (0x1f600, 2),
        (0x110000, 1),
        (u32::MAX, 1),
    ] {
        assert_eq!(wcwidth(value), expected, "U+{value:04X}");
    }
}
