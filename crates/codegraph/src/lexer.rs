//! A tiny Rust lexer — just enough to skip comments and string/char literals
//! and emit identifier + punctuation tokens with line numbers. Not a full
//! Rust parser; documented limits:
//!   - raw strings `r#"..."#` handled by a simple `r#`/`"#` scan
//!   - no macro expansion; tokens inside macros are seen literally
//!   - lifetimes (`'a`) are lexed as a punct `'` + ident, harmless for our use

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Punct(char),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
}

pub fn lex(src: &str) -> Vec<Token> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                let mut depth = 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            'r' if matches!(chars.get(i + 1), Some('"') | Some('#')) => {
                i = skip_raw_string(&chars, i, &mut line);
            }
            '"' => {
                i = skip_string(&chars, i + 1, '"', &mut line);
            }
            '\'' => {
                // Char literal or lifetime. If it looks like 'x' consume it,
                // else treat the quote as a lone punct (lifetime).
                if is_char_literal(&chars, i) {
                    i = skip_string(&chars, i + 1, '\'', &mut line);
                } else {
                    out.push(Token { tok: Tok::Punct('\''), line });
                    i += 1;
                }
            }
            c if c.is_alphanumeric() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Token { tok: Tok::Ident(chars[start..i].iter().collect()), line });
            }
            c => {
                out.push(Token { tok: Tok::Punct(c), line });
                i += 1;
            }
        }
    }
    out
}

fn is_char_literal(chars: &[char], i: usize) -> bool {
    // 'x'  or  '\n'  → char literal; otherwise treat as lifetime tick.
    match chars.get(i + 1) {
        Some('\\') => chars.get(i + 3) == Some(&'\''),
        Some(_) => chars.get(i + 2) == Some(&'\''),
        None => false,
    }
}

fn skip_string(chars: &[char], mut i: usize, quote: char, line: &mut usize) -> usize {
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '\n' => {
                *line += 1;
                i += 1;
            }
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    i
}

fn skip_raw_string(chars: &[char], start: usize, line: &mut usize) -> usize {
    // r, then N '#', then '"', ... '"' then N '#'
    let mut i = start + 1;
    let mut hashes = 0;
    while chars.get(i) == Some(&'#') {
        hashes += 1;
        i += 1;
    }
    if chars.get(i) != Some(&'"') {
        return start + 1; // not actually a raw string, back off
    }
    i += 1;
    let closing: String = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
    let closing: Vec<char> = closing.chars().collect();
    while i < chars.len() {
        if chars[i] == '\n' {
            *line += 1;
        }
        if chars[i..].starts_with(&closing[..]) {
            return i + closing.len();
        }
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idents(src: &str) -> Vec<String> {
        lex(src).into_iter().filter_map(|t| match t.tok { Tok::Ident(s) => Some(s), _ => None }).collect()
    }

    #[test]
    fn skips_comments_and_strings() {
        let src = r#"
            // fn commented_out()
            fn real() {
                let s = "fn not_a_fn()";
                /* fn also_not() */
            }
        "#;
        let ids = idents(src);
        assert!(ids.contains(&"real".to_string()));
        assert!(!ids.contains(&"commented_out".to_string()));
        assert!(!ids.contains(&"not_a_fn".to_string()));
        assert!(!ids.contains(&"also_not".to_string()));
    }

    #[test]
    fn tracks_lines() {
        let toks = lex("a\n\nb");
        let b = toks.iter().find(|t| t.tok == Tok::Ident("b".into())).unwrap();
        assert_eq!(b.line, 3);
    }

    #[test]
    fn raw_strings() {
        let ids = idents(r##"fn f() { let x = r#"fn ghost()"#; }"##);
        assert!(ids.contains(&"f".to_string()));
        assert!(!ids.contains(&"ghost".to_string()));
    }
}
