//! Small std-only multi-language front-end for codegraph.
//!
//! This intentionally avoids tree-sitter or language-server dependencies. It
//! extracts the common structural surface from the languages seen in the cloned
//! code-graph references: definitions and simple call edges. Precision is best
//! effort; exact language parsing remains out of scope for this lean binary.

use crate::graph::{Edge, Graph, Symbol};
use crate::lexer::{lex, Tok, Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Kotlin,
    Scala,
    Ruby,
    Php,
    Swift,
}

impl Language {
    pub fn from_path(path: &str) -> Option<Language> {
        let p = path.to_ascii_lowercase();
        let ext = p.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => Some(Language::Rust),
            "py" | "pyw" => Some(Language::Python),
            "js" | "jsx" | "mjs" | "cjs" | "vue" | "svelte" => Some(Language::JavaScript),
            "ts" | "tsx" | "mts" | "cts" | "ets" => Some(Language::TypeScript),
            "go" => Some(Language::Go),
            "java" | "groovy" | "gradle" => Some(Language::Java),
            "c" | "h" => Some(Language::C),
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "cu" | "cuh" | "metal" => {
                Some(Language::Cpp)
            }
            "cs" => Some(Language::CSharp),
            "kt" | "kts" => Some(Language::Kotlin),
            "scala" | "sc" => Some(Language::Scala),
            "rb" => Some(Language::Ruby),
            "php" | "phtml" => Some(Language::Php),
            "swift" => Some(Language::Swift),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Go => "Go",
            Language::Java => "Java/Groovy",
            Language::C => "C",
            Language::Cpp => "C++/CUDA/Metal",
            Language::CSharp => "C#",
            Language::Kotlin => "Kotlin",
            Language::Scala => "Scala",
            Language::Ruby => "Ruby",
            Language::Php => "PHP",
            Language::Swift => "Swift",
        }
    }

    pub fn all_names() -> &'static str {
        "Rust, Python, JavaScript/JSX, TypeScript/TSX, Go, Java/Groovy, C, C++/CUDA/Metal, C#, Kotlin, Scala, Ruby, PHP, Swift"
    }
}

pub fn index_source(graph: &mut Graph, file: &str, src: &str) {
    let Some(lang) = Language::from_path(file) else {
        return;
    };
    if matches!(lang, Language::Rust) {
        crate::index::index_rust_source(graph, file, src);
        return;
    }
    let clean = strip_comments_and_strings(src, lang);
    let toks = lex(&clean);
    extract_symbols(graph, file, lang, &toks);
    extract_calls(graph, lang, &toks);
}

fn ident(t: &Token) -> Option<&str> {
    match &t.tok {
        Tok::Ident(s) => Some(s.as_str()),
        _ => None,
    }
}
fn punct(t: &Token, c: char) -> bool {
    t.tok == Tok::Punct(c)
}
fn at(toks: &[Token], i: usize, s: &str) -> bool {
    toks.get(i).and_then(ident) == Some(s)
}

fn extract_symbols(graph: &mut Graph, file: &str, lang: Language, toks: &[Token]) {
    for i in 0..toks.len() {
        let Some(word) = ident(&toks[i]) else {
            continue;
        };
        match lang {
            Language::Python => {
                if matches!(word, "def" | "class") {
                    add_next(graph, file, toks, i, word);
                }
            }
            Language::JavaScript | Language::TypeScript => {
                extract_js_symbol(graph, file, toks, i, word)
            }
            Language::Go => {
                if word == "func" {
                    extract_go_func(graph, file, toks, i);
                }
                if matches!(word, "type" | "var" | "const") {
                    add_next(graph, file, toks, i, word);
                }
            }
            Language::Java
            | Language::CSharp
            | Language::Kotlin
            | Language::Scala
            | Language::Php
            | Language::Swift => {
                if matches!(
                    word,
                    "class" | "interface" | "enum" | "struct" | "trait" | "object" | "protocol"
                ) {
                    add_next(graph, file, toks, i, word);
                }
                if matches!(word, "fun" | "func" | "function" | "def") {
                    add_next(graph, file, toks, i, "fn");
                }
                extract_c_family_method(graph, file, toks, i);
            }
            Language::C | Language::Cpp => {
                if matches!(word, "struct" | "class" | "enum" | "union" | "namespace") {
                    add_next(graph, file, toks, i, word);
                }
                extract_c_family_method(graph, file, toks, i);
            }
            Language::Ruby => {
                if matches!(word, "def" | "class" | "module") {
                    add_next(graph, file, toks, i, word);
                }
            }
            Language::Rust => {}
        }
    }
}

fn add_next(graph: &mut Graph, file: &str, toks: &[Token], i: usize, kind: &str) {
    if let Some(name_tok) = toks.get(i + 1) {
        if let Some(name) = ident(name_tok) {
            graph.add_symbol(Symbol {
                name: name.to_string(),
                kind: kind.to_string(),
                file: file.to_string(),
                line: name_tok.line,
            });
            graph.add_edge(Edge {
                kind: "defines".into(),
                from: file.to_string(),
                to: name.to_string(),
            });
        }
    }
}

fn extract_js_symbol(graph: &mut Graph, file: &str, toks: &[Token], i: usize, word: &str) {
    if matches!(word, "function" | "class" | "interface" | "enum" | "type") {
        add_next(graph, file, toks, i, word);
    }
    if matches!(word, "const" | "let" | "var") {
        if let Some(name) = toks.get(i + 1).and_then(ident) {
            let mut j = i + 2;
            while j < toks.len() && !punct(&toks[j], ';') && !punct(&toks[j], '\n') {
                if punct(&toks[j], '=')
                    && (at(toks, j + 1, "function")
                        || toks.get(j + 1).map(|t| punct(t, '(')).unwrap_or(false))
                {
                    graph.add_symbol(Symbol {
                        name: name.into(),
                        kind: "fn".into(),
                        file: file.into(),
                        line: toks[i + 1].line,
                    });
                    graph.add_edge(Edge {
                        kind: "defines".into(),
                        from: file.into(),
                        to: name.into(),
                    });
                    break;
                }
                if punct(&toks[j], ';') || punct(&toks[j], '{') {
                    break;
                }
                j += 1;
            }
        }
    }
}

fn extract_go_func(graph: &mut Graph, file: &str, toks: &[Token], i: usize) {
    let mut j = i + 1;
    if toks.get(j).map(|t| punct(t, '(')).unwrap_or(false) {
        while j < toks.len() && !punct(&toks[j], ')') {
            j += 1;
        }
        j += 1;
    }
    if let Some(name) = toks.get(j).and_then(ident) {
        graph.add_symbol(Symbol {
            name: name.into(),
            kind: "fn".into(),
            file: file.into(),
            line: toks[j].line,
        });
        graph.add_edge(Edge {
            kind: "defines".into(),
            from: file.into(),
            to: name.into(),
        });
    }
}

fn extract_c_family_method(graph: &mut Graph, file: &str, toks: &[Token], i: usize) {
    let Some(name) = ident(&toks[i]) else {
        return;
    };
    if is_keyword(name) || !toks.get(i + 1).map(|t| punct(t, '(')).unwrap_or(false) {
        return;
    }
    if i > 0
        && matches!(
            ident(&toks[i - 1]),
            Some("if" | "for" | "while" | "switch" | "catch" | "return" | "new")
        )
    {
        return;
    }
    let mut j = i + 2;
    let mut depth = 1;
    while j < toks.len() && depth > 0 {
        if punct(&toks[j], '(') {
            depth += 1;
        }
        if punct(&toks[j], ')') {
            depth -= 1;
        }
        j += 1;
    }
    while j < toks.len() && !punct(&toks[j], '{') && !punct(&toks[j], ';') {
        j += 1;
    }
    if toks.get(j).map(|t| punct(t, '{')).unwrap_or(false) {
        graph.add_symbol(Symbol {
            name: name.into(),
            kind: "fn".into(),
            file: file.into(),
            line: toks[i].line,
        });
        graph.add_edge(Edge {
            kind: "defines".into(),
            from: file.into(),
            to: name.into(),
        });
    }
}

fn extract_calls(graph: &mut Graph, lang: Language, toks: &[Token]) {
    let funcs: Vec<(String, usize, usize)> = graph
        .symbols
        .iter()
        .filter(|s| s.kind == "fn" || s.kind == "def" || s.kind == "function")
        .map(|s| (s.name.clone(), s.line, usize::MAX))
        .collect();
    for (idx, (fname, line, _)) in funcs.iter().enumerate() {
        let start = toks.iter().position(|t| t.line >= *line).unwrap_or(0);
        let end = funcs
            .get(idx + 1)
            .and_then(|(_, l, _)| toks.iter().position(|t| t.line >= *l))
            .unwrap_or(toks.len());
        for k in start..end.saturating_sub(1) {
            if let Some(callee) = ident(&toks[k]) {
                if is_keyword(callee) {
                    continue;
                }
                if toks.get(k + 1).map(|t| punct(t, '(')).unwrap_or(false) && callee != fname {
                    graph.add_edge(Edge {
                        kind: "calls".into(),
                        from: fname.clone(),
                        to: callee.into(),
                    });
                }
            }
        }
    }
    let _ = lang;
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "for"
            | "while"
            | "switch"
            | "catch"
            | "return"
            | "new"
            | "class"
            | "struct"
            | "enum"
            | "interface"
            | "fn"
            | "def"
            | "function"
            | "func"
            | "fun"
            | "let"
            | "const"
            | "var"
            | "type"
            | "package"
            | "import"
            | "from"
            | "public"
            | "private"
            | "protected"
            | "static"
            | "async"
            | "await"
            | "else"
            | "do"
            | "try"
    )
}

fn strip_comments_and_strings(src: &str, lang: Language) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            out.push('\n');
            i += 1;
            continue;
        }
        if matches!(lang, Language::Python | Language::Ruby) && c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            out.push(' ');
            out.push(' ');
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            if i + 1 < chars.len() {
                out.push(' ');
                out.push(' ');
                i += 2;
            }
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            out.push(' ');
            i += 1;
            while i < chars.len() {
                let d = chars[i];
                out.push(if d == '\n' { '\n' } else { ' ' });
                i += 1;
                if d == '\\' && i < chars.len() {
                    out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                    i += 1;
                    continue;
                }
                if d == quote {
                    break;
                }
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(file: &str, src: &str) -> Vec<String> {
        let mut g = Graph::new();
        index_source(&mut g, file, src);
        g.symbols
            .into_iter()
            .map(|s| format!("{}:{}", s.kind, s.name))
            .collect()
    }

    #[test]
    fn language_by_extension() {
        assert_eq!(Language::from_path("x.tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_path("x.py"), Some(Language::Python));
        assert_eq!(Language::from_path("x.cu"), Some(Language::Cpp));
    }

    #[test]
    fn extracts_common_languages() {
        assert!(names("a.py", "class C:\n def f(self): g()\n")
            .iter()
            .any(|n| n == "class:C"));
        assert!(names(
            "a.ts",
            "export function run(){} class Box {} const f = () => run();"
        )
        .iter()
        .any(|n| n == "function:run"));
        assert!(
            names("a.go", "package p\nfunc Serve(){}\ntype Box struct{}\n")
                .iter()
                .any(|n| n == "fn:Serve")
        );
        assert!(names("A.java", "class A { void run() { help(); } }")
            .iter()
            .any(|n| n == "class:A"));
        assert!(names("a.cpp", "class A {}; int run() { help(); }")
            .iter()
            .any(|n| n == "fn:run"));
    }
}
