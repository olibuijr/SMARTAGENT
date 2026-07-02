//! Percent-encoding for query strings (RFC 3986 unreserved set kept literal).

pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b;
        // Unreserved: A-Z a-z 0-9 - _ . ~
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            out.push('%');
            out.push_str(&format!("{c:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_specials() {
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("x&y#z"), "x%26y%23z");
        assert_eq!(percent_encode("Keep-_.~"), "Keep-_.~");
    }

    #[test]
    fn encodes_icelandic() {
        // ö = U+00F6 → UTF-8 C3 B6
        assert_eq!(percent_encode("ö"), "%C3%B6");
        assert_eq!(percent_encode("æð"), "%C3%A6%C3%B0");
    }
}
