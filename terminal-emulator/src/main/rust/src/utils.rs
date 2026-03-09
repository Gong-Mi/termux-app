use unicode_width::UnicodeWidthChar;

/// 将 ASCII 字符映射为 VT100 绘图字符 (Special Character and Line Drawing Set)
pub fn map_line_drawing(c: u8) -> char {
    match c {
        b'_' => ' ',   // Blank
        b'`' => '◆',   // Diamond
        b'0' => '█',   // Solid block
        b'a' => '▒',   // Checker board
        b'b' => '␉',   // Horizontal tab
        b'c' => '␌',   // Form feed
        b'd' => '\r',  // Carriage return
        b'e' => '␊',   // Linefeed
        b'f' => '°',   // Degree
        b'g' => '±',   // Plus-minus
        b'h' => '\n',  // Newline
        b'i' => '␋',   // Vertical tab
        b'j' => '┘',   // Lower right corner
        b'k' => '┐',   // Upper right corner
        b'l' => '┌',   // Upper left corner
        b'm' => '└',   // Lower left corner
        b'n' => '┼',   // Crossing lines
        b'o' => '⎺',   // Horizontal line - scan 1
        b'p' => '⎻',   // Horizontal line - scan 3
        b'q' => '─',   // Horizontal line - scan 5
        b'r' => '⎼',   // Horizontal line - scan 7
        b's' => '⎽',   // Horizontal line - scan 9
        b't' => '├',   // T facing rightwards
        b'u' => '┤',   // T facing leftwards
        b'v' => '┴',   // T facing upwards
        b'w' => '┬',   // T facing downwards
        b'x' => '│',   // Vertical line
        b'y' => '≤',   // Less than or equal to
        b'z' => '≥',   // Greater than or equal to
        b'{' => 'π',   // Pi
        b'|' => '≠',   // Not equal to
        b'}' => '£',   // UK pound
        b'~' => '·',   // Centered dot
        _ => c as char,
    }
}

pub fn get_char_width(ucs: u32) -> usize {
    if let Some(c) = std::char::from_u32(ucs) {
        c.width().unwrap_or(1)
    } else {
        1
    }
}
