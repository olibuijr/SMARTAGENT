//! Key decoding from a raw byte burst read off the terminal.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Space,
    Tab,
    Backspace,
    Esc,
    CtrlC,
    CtrlN,
    CtrlP,
    CtrlU,
    Char(char),
    Unknown,
}

/// Parse a single key from a burst of bytes. Terminals deliver an escape
/// sequence (e.g. an arrow key = `ESC [ A`) in one read, so matching the whole
/// burst disambiguates arrows from a lone Esc without timers.
pub fn parse(b: &[u8]) -> Key {
    match b {
        [0x1b, b'[', b'A', ..] => Key::Up,
        [0x1b, b'[', b'B', ..] => Key::Down,
        [0x1b, b'[', b'C', ..] => Key::Right,
        [0x1b, b'[', b'D', ..] => Key::Left,
        [0x1b, b'[', b'H', ..] | [0x1b, b'O', b'H', ..] => Key::Home,
        [0x1b, b'[', b'F', ..] | [0x1b, b'O', b'F', ..] => Key::End,
        [0x1b, b'[', b'5', b'~', ..] => Key::PageUp,
        [0x1b, b'[', b'6', b'~', ..] => Key::PageDown,
        [0x1b] => Key::Esc,
        [0x0d, ..] | [0x0a, ..] => Key::Enter,
        [0x20, ..] => Key::Space,
        [0x09, ..] => Key::Tab,
        [0x7f, ..] | [0x08, ..] => Key::Backspace,
        [0x03, ..] => Key::CtrlC,
        [0x0e, ..] => Key::CtrlN,
        [0x10, ..] => Key::CtrlP,
        [0x15, ..] => Key::CtrlU,
        _ => {
            if let Ok(s) = std::str::from_utf8(b) {
                if let Some(c) = s.chars().next() {
                    if !c.is_control() {
                        return Key::Char(c);
                    }
                }
            }
            Key::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_and_controls() {
        assert_eq!(parse(&[0x1b, b'[', b'A']), Key::Up);
        assert_eq!(parse(&[0x1b, b'[', b'B']), Key::Down);
        assert_eq!(parse(&[0x1b]), Key::Esc);
        assert_eq!(parse(&[0x0d]), Key::Enter);
        assert_eq!(parse(&[0x03]), Key::CtrlC);
        assert_eq!(parse(&[0x7f]), Key::Backspace);
        assert_eq!(parse(&[0x20]), Key::Space);
    }

    #[test]
    fn utf8_char() {
        assert_eq!(parse("a".as_bytes()), Key::Char('a'));
        assert_eq!(parse("ó".as_bytes()), Key::Char('ó'));
    }
}
