use unicode_width::UnicodeWidthChar;

/// 将 ASCII 字符映射为 VT100 绘图字符 (Special Character and Line Drawing Set)
pub fn map_line_drawing(c: u8) -> u16 {
    match c {
        b'_' => ' ' as u16,  // Blank
        b'`' => '◆' as u16,  // Diamond
        b'0' => '█' as u16,  // Solid block
        b'a' => '▒' as u16,  // Checker board
        b'b' => '␉' as u16,  // Horizontal tab
        b'c' => '␌' as u16,  // Form feed
        b'd' => '\r' as u16, // Carriage return
        b'e' => '␊' as u16,  // Linefeed
        b'f' => '°' as u16,  // Degree
        b'g' => '±' as u16,  // Plus-minus
        b'h' => '\n' as u16, // Newline
        b'i' => '␋' as u16,  // Vertical tab
        b'j' => '┘' as u16,  // Lower right corner
        b'k' => '┐' as u16,  // Upper right corner
        b'l' => '┌' as u16,  // Upper left corner
        b'm' => '└' as u16,  // Lower left corner
        b'n' => '┼' as u16,  // Crossing lines
        b'o' => '⎺' as u16,  // Horizontal line - scan 1
        b'p' => '⎻' as u16,  // Horizontal line - scan 3
        b'q' => '─' as u16,  // Horizontal line - scan 5
        b'r' => '⎼' as u16,  // Horizontal line - scan 7
        b's' => '⎽' as u16,  // Horizontal line - scan 9
        b't' => '├' as u16,  // T facing rightwards
        b'u' => '┤' as u16,  // T facing leftwards
        b'v' => '┴' as u16,  // T facing upwards
        b'w' => '┬' as u16,  // T facing downwards
        b'x' => '│' as u16,  // Vertical line
        b'y' => '≤' as u16,  // Less than or equal to
        b'z' => '≥' as u16,  // Greater than or equal to
        b'{' => 'π' as u16,  // Pi
        b'|' => '≠' as u16,  // Not equal to
        b'}' => '£' as u16,  // UK pound
        b'~' => '·' as u16,  // Centered dot
        _ => c as u16,
    }
}

pub fn get_char_width(ucs: u32) -> usize {
    if let Some(c) = std::char::from_u32(ucs) {
        c.width().unwrap_or(1)
    } else {
        1
    }
}
