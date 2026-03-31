use std::ffi::CString;

pub enum LogPriority {
    VERBOSE = 2,
    DEBUG = 3,
    INFO = 4,
    WARN = 5,
    ERROR = 6,
}

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn __android_log_print(prio: i32, tag: *const libc::c_char, fmt: *const libc::c_char, ...);
}

pub fn android_log(prio: LogPriority, msg: &str) {
    #[cfg(target_os = "android")]
    {
        let tag = CString::new("Termux-Rust").unwrap();
        let msg_c = CString::new(msg).unwrap();
        unsafe {
            __android_log_print(prio as i32, tag.as_ptr(), msg_c.as_ptr());
        }
    }
    
    #[cfg(not(target_os = "android"))]
    {
        let prefix = match prio {
            LogPriority::ERROR => "E",
            LogPriority::WARN => "W",
            LogPriority::INFO => "I",
            _ => "D",
        };
        println!("[{}] Termux-Rust: {}", prefix, msg);
    }
}

pub fn map_line_drawing(c: u8) -> char {
    match c {
        b'_' => ' ', b'`' => '◆', b'0' => '█', b'a' => '▒', b'b' => '␉',
        b'c' => '␌', b'd' => '\r', b'e' => '␊', b'f' => '°', b'g' => '±',
        b'h' => '\n', b'i' => '␋', b'j' => '┘', b'k' => '┐', b'l' => '┌',
        b'm' => '└', b'n' => '┼', b'o' => '⎺', b'p' => '⎻', b'q' => '─',
        b'r' => '⎼', b's' => '⎽', b't' => '├', b'u' => '┤', b'v' => '┴',
        b'w' => '┬', b'x' => '│', b'y' => '≤', b'z' => '≥', b'{' => 'π',
        b'|' => '≠', b'}' => '£', b'~' => '·', _ => c as char,
    }
}

pub fn get_char_width(ucs: u32) -> usize {
    if ucs == 0 { return 0; } // 占位符必须为 0 宽
    
    use unicode_width::UnicodeWidthChar;
    if let Some(c) = std::char::from_u32(ucs) {
        if (ucs < 32) || (ucs >= 0x7F && ucs < 0xA0) { return 0; }
        c.width().unwrap_or(1)
    } else {
        1
    }
}
