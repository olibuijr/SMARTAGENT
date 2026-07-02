//! A tiny regex subset matcher (backtracking), std-only.
//! Supports: literals, '.', '*', '+', '?', character classes [...] (with
//! ranges and leading '^' negation), '^' and '$' anchors, and alternation '|'
//! at the top level. NOT a full regex engine — documented limits:
//!   - no grouping/capture parens, no {n,m}, no escapes beyond \\. \\* etc.
//!   - alternation only splits the whole pattern, not sub-expressions.

pub struct Regex {
    branches: Vec<Vec<Token>>,
    anchored_start: bool,
    anchored_end: bool,
}

enum Token {
    Char(char),
    Any,
    Class { negated: bool, ranges: Vec<(char, char)> },
    Star(Box<Token>),
    Plus(Box<Token>),
    Opt(Box<Token>),
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Regex, String> {
        let anchored_start = pattern.starts_with('^');
        let anchored_end = pattern.ends_with('$') && !pattern.ends_with("\\$");
        let core = &pattern[if anchored_start { 1 } else { 0 }..pattern.len() - if anchored_end { 1 } else { 0 }];
        let mut branches = Vec::new();
        for alt in core.split('|') {
            branches.push(compile(alt)?);
        }
        Ok(Regex { branches, anchored_start, anchored_end })
    }

    /// True if the pattern matches anywhere in `line` (respecting anchors).
    pub fn is_match(&self, line: &str) -> bool {
        let chars: Vec<char> = line.chars().collect();
        for branch in &self.branches {
            let starts: Vec<usize> = if self.anchored_start { vec![0] } else { (0..=chars.len()).collect() };
            for start in starts {
                if let Some(end) = match_here(branch, &chars, start) {
                    if !self.anchored_end || end == chars.len() {
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn compile(pat: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = pat.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let base = match chars[i] {
            '\\' => {
                i += 1;
                Token::Char(*chars.get(i).ok_or("trailing backslash")?)
            }
            '.' => Token::Any,
            '[' => {
                let (tok, next) = compile_class(&chars, i)?;
                i = next;
                tok
            }
            c => Token::Char(c),
        };
        // Quantifier?
        let tok = match chars.get(i + 1) {
            Some('*') => { i += 1; Token::Star(Box::new(base)) }
            Some('+') => { i += 1; Token::Plus(Box::new(base)) }
            Some('?') => { i += 1; Token::Opt(Box::new(base)) }
            _ => base,
        };
        tokens.push(tok);
        i += 1;
    }
    Ok(tokens)
}

fn compile_class(chars: &[char], start: usize) -> Result<(Token, usize), String> {
    let mut i = start + 1;
    let negated = chars.get(i) == Some(&'^');
    if negated {
        i += 1;
    }
    let mut ranges = Vec::new();
    while i < chars.len() && chars[i] != ']' {
        if chars.get(i + 1) == Some(&'-') && chars.get(i + 2).map(|c| *c != ']').unwrap_or(false) {
            ranges.push((chars[i], chars[i + 2]));
            i += 3;
        } else {
            ranges.push((chars[i], chars[i]));
            i += 1;
        }
    }
    if chars.get(i) != Some(&']') {
        return Err("unterminated character class".into());
    }
    Ok((Token::Class { negated, ranges }, i))
}

fn tok_matches(tok: &Token, c: char) -> bool {
    match tok {
        Token::Char(t) => *t == c,
        Token::Any => true,
        Token::Class { negated, ranges } => {
            let hit = ranges.iter().any(|(lo, hi)| c >= *lo && c <= *hi);
            hit != *negated
        }
        _ => false,
    }
}

/// Returns the end index if the token sequence matches starting at `pos`.
fn match_here(tokens: &[Token], chars: &[char], pos: usize) -> Option<usize> {
    if tokens.is_empty() {
        return Some(pos);
    }
    match &tokens[0] {
        Token::Star(inner) => match_repeat(inner, &tokens[1..], chars, pos, 0),
        Token::Plus(inner) => match_repeat(inner, &tokens[1..], chars, pos, 1),
        Token::Opt(inner) => {
            if pos < chars.len() && tok_matches(inner, chars[pos]) {
                if let Some(e) = match_here(&tokens[1..], chars, pos + 1) {
                    return Some(e);
                }
            }
            match_here(&tokens[1..], chars, pos)
        }
        t => {
            if pos < chars.len() && tok_matches(t, chars[pos]) {
                match_here(&tokens[1..], chars, pos + 1)
            } else {
                None
            }
        }
    }
}

fn match_repeat(inner: &Token, rest: &[Token], chars: &[char], pos: usize, min: usize) -> Option<usize> {
    // Greedy: consume as many as possible, then backtrack.
    let mut count = 0;
    let mut p = pos;
    while p < chars.len() && tok_matches(inner, chars[p]) {
        p += 1;
        count += 1;
    }
    while count >= min {
        if let Some(e) = match_here(rest, chars, pos + count) {
            return Some(e);
        }
        if count == 0 {
            break;
        }
        count -= 1;
    }
    if min == 0 {
        match_here(rest, chars, pos)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool { Regex::new(p).unwrap().is_match(s) }

    #[test]
    fn literals_and_dot() {
        assert!(m("abc", "xabcy"));
        assert!(m("a.c", "azc"));
        assert!(!m("a.c", "ac"));
    }

    #[test]
    fn quantifiers() {
        assert!(m("ab*c", "ac"));
        assert!(m("ab*c", "abbbc"));
        assert!(m("ab+c", "abc"));
        assert!(!m("ab+c", "ac"));
        assert!(m("ab?c", "ac"));
    }

    #[test]
    fn classes_and_anchors() {
        assert!(m("[0-9]+", "abc123"));
        assert!(m("[^a-z]", "A"));
        assert!(m("^fn ", "fn main"));
        assert!(!m("^fn ", "  fn main"));
        assert!(m("world$", "hello world"));
    }

    #[test]
    fn alternation() {
        assert!(m("cat|dog", "a dog here"));
        assert!(m("cat|dog", "a cat here"));
        assert!(!m("cat|dog", "a fish here"));
    }
}
