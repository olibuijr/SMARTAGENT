//! Extract symbols and edges from a Rust token stream.
//!
//! Symbols: fn / struct / enum / trait / impl / mod / const / static, captured
//! by the `<keyword> <name>` pattern at token level (so commented/stringed
//! occurrences are already gone — the lexer dropped them).
//!
//! Edges:
//!   defines — file → symbol
//!   calls   — fn → identifier immediately followed by `(` inside its body
//!   impls   — `impl Trait for Type` and `impl Type`

use crate::graph::{Edge, Graph, Symbol};
use crate::lexer::{lex, Tok, Token};

const KINDS: &[&str] = &["fn", "struct", "enum", "trait", "impl", "mod", "const", "static"];

pub fn index_source(graph: &mut Graph, file: &str, src: &str) {
    let toks = lex(src);
    extract_symbols(graph, file, &toks);
    extract_calls(graph, &toks);
    extract_impls(graph, &toks);
}

fn ident(t: &Token) -> Option<&str> {
    match &t.tok {
        Tok::Ident(s) => Some(s.as_str()),
        _ => None,
    }
}

fn is_punct(t: &Token, c: char) -> bool {
    t.tok == Tok::Punct(c)
}

fn extract_symbols(graph: &mut Graph, file: &str, toks: &[Token]) {
    let mut i = 0;
    while i < toks.len() {
        if let Some(kw) = ident(&toks[i]) {
            if KINDS.contains(&kw) {
                // `impl` names are handled by extract_impls; still record a symbol
                // for a plain `impl Type` via the type name.
                if let Some(name_tok) = toks.get(i + 1) {
                    if let Some(name) = ident(name_tok) {
                        // Skip `fn` keyword false-positives like `fn` in `impl fn`? rare.
                        if kw != "impl" {
                            graph.add_symbol(Symbol {
                                name: name.to_string(),
                                kind: kw.to_string(),
                                file: file.to_string(),
                                line: name_tok.line,
                            });
                            graph.add_edge(Edge { kind: "defines".into(), from: file.to_string(), to: name.to_string() });
                        }
                    }
                }
            }
        }
        i += 1;
    }
}

/// For each `fn NAME ( ... ) { BODY }`, record `calls` edges from NAME to any
/// `ident (` occurrence within the balanced body braces.
fn extract_calls(graph: &mut Graph, toks: &[Token]) {
    let mut i = 0;
    while i < toks.len() {
        if ident(&toks[i]) == Some("fn") {
            if let Some(fname) = toks.get(i + 1).and_then(ident) {
                // Find the opening brace of the body.
                let mut j = i + 2;
                while j < toks.len() && !is_punct(&toks[j], '{') {
                    // Stop if we hit a `;` (fn signature only, e.g. trait decl).
                    if is_punct(&toks[j], ';') {
                        break;
                    }
                    j += 1;
                }
                if j < toks.len() && is_punct(&toks[j], '{') {
                    let end = matching_brace(toks, j);
                    for k in (j + 1)..end {
                        if let Some(callee) = ident(&toks[k]) {
                            if toks.get(k + 1).map(|t| is_punct(t, '(')).unwrap_or(false) {
                                graph.add_edge(Edge { kind: "calls".into(), from: fname.to_string(), to: callee.to_string() });
                            }
                        }
                    }
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
}

fn matching_brace(toks: &[Token], open: usize) -> usize {
    let mut depth = 0;
    let mut i = open;
    while i < toks.len() {
        if is_punct(&toks[i], '{') {
            depth += 1;
        } else if is_punct(&toks[i], '}') {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
        i += 1;
    }
    toks.len()
}

fn extract_impls(graph: &mut Graph, toks: &[Token]) {
    let mut i = 0;
    while i < toks.len() {
        if ident(&toks[i]) == Some("impl") {
            // Collect idents up to the opening brace or `{`.
            let mut names = Vec::new();
            let mut saw_for = false;
            let mut j = i + 1;
            while j < toks.len() && !is_punct(&toks[j], '{') {
                if let Some(id) = ident(&toks[j]) {
                    if id == "for" {
                        saw_for = true;
                    } else {
                        names.push((id.to_string(), toks[j].line));
                    }
                }
                j += 1;
            }
            if saw_for && names.len() >= 2 {
                // impl Trait for Type
                let trait_name = &names[0].0;
                let type_name = &names[names.len() - 1].0;
                graph.add_symbol(Symbol { name: type_name.clone(), kind: "impl".into(), file: String::new(), line: names[0].1 });
                graph.add_edge(Edge { kind: "impls".into(), from: type_name.clone(), to: trait_name.clone() });
            } else if let Some((type_name, line)) = names.first() {
                graph.add_symbol(Symbol { name: type_name.clone(), kind: "impl".into(), file: String::new(), line: *line });
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_and_calls() {
        let src = r#"
            fn helper() {}
            struct Widget;
            fn main() {
                helper();
                { helper(); }  // nested block still counts
                let _ = "helper()"; // in a string — must NOT count
            }
        "#;
        let mut g = Graph::new();
        index_source(&mut g, "m.rs", src);
        assert!(g.symbols.iter().any(|s| s.name == "helper" && s.kind == "fn"));
        assert!(g.symbols.iter().any(|s| s.name == "Widget" && s.kind == "struct"));
        // main calls helper exactly twice (both real; the string one excluded)
        let calls = g.edges.iter().filter(|e| e.kind == "calls" && e.from == "main" && e.to == "helper").count();
        assert_eq!(calls, 2);
    }

    #[test]
    fn impl_for() {
        let src = "impl Display for Widget { fn fmt() {} }";
        let mut g = Graph::new();
        index_source(&mut g, "m.rs", src);
        assert!(g.edges.iter().any(|e| e.kind == "impls" && e.from == "Widget" && e.to == "Display"));
    }
}
