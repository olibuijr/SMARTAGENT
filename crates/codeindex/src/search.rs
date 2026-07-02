//! Parallel line search across files (literal or regex-lite), context lines.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::regexlite::Regex;

pub struct Match {
    pub file: PathBuf,
    pub line_no: usize,
    pub text: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

pub struct Opts {
    pub regex: bool,
    pub ignore_case: bool,
    pub before: usize,
    pub after: usize,
}

enum Pat { Literal(String), Re(Regex) }

impl Pat {
    fn build(pattern: &str, o: &Opts) -> Result<Pat, String> {
        if o.regex {
            Ok(Pat::Re(Regex::new(pattern)?))
        } else {
            Ok(Pat::Literal(if o.ignore_case { pattern.to_lowercase() } else { pattern.to_string() }))
        }
    }
    fn hit(&self, line: &str, ic: bool) -> bool {
        match self {
            Pat::Literal(p) => if ic { line.to_lowercase().contains(p) } else { line.contains(p) },
            Pat::Re(r) => {
                let lowered;
                let target = if ic { lowered = line.to_lowercase(); lowered.as_str() } else { line };
                r.is_match(target)
            }
        }
    }
}

pub fn search(files: &[PathBuf], pattern: &str, o: &Opts) -> Result<Vec<Match>, String> {
    // Validate pattern once up front.
    Pat::build(pattern, o)?;
    let (tx, rx) = mpsc::channel();
    let n = files.len().min(std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4));
    let chunks: Vec<Vec<PathBuf>> = if n == 0 { vec![] } else {
        let mut cs = vec![Vec::new(); n];
        for (i, f) in files.iter().enumerate() { cs[i % n].push(f.clone()); }
        cs
    };
    std::thread::scope(|scope| {
        for chunk in &chunks {
            let tx = tx.clone();
            let pat = pattern.to_string();
            scope.spawn(move || {
                let p = Pat::build(&pat, o).unwrap();
                for file in chunk {
                    if let Ok(content) = std::fs::read_to_string(file) {
                        search_file(file, &content, &p, o, &tx);
                    }
                }
            });
        }
    });
    drop(tx);
    let mut out: Vec<Match> = rx.iter().collect();
    out.sort_by(|a, b| a.file.cmp(&b.file).then(a.line_no.cmp(&b.line_no)));
    Ok(out)
}

fn search_file(file: &Path, content: &str, pat: &Pat, o: &Opts, tx: &mpsc::Sender<Match>) {
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if pat.hit(line, o.ignore_case) {
            let before = (i.saturating_sub(o.before)..i).map(|j| lines[j].to_string()).collect();
            let after = ((i + 1)..(i + 1 + o.after).min(lines.len())).map(|j| lines[j].to_string()).collect();
            let _ = tx.send(Match { file: file.to_path_buf(), line_no: i + 1, text: line.to_string(), before, after });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn literal_and_case() {
        let d = scratch("ci-search");
        std::fs::write(d.join("a.rs"), "fn Main() {}\nlet x = 1;").unwrap();
        let files = vec![d.join("a.rs")];
        let o = Opts { regex: false, ignore_case: true, before: 0, after: 0 };
        let hits = search(&files, "main", &o).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line_no, 1);
    }

    #[test]
    fn regex_and_context() {
        let d = scratch("ci-re");
        std::fs::write(d.join("b.rs"), "one\nfn foo123()\nthree").unwrap();
        let files = vec![d.join("b.rs")];
        let o = Opts { regex: true, ignore_case: false, before: 1, after: 1 };
        let hits = search(&files, "foo[0-9]+", &o).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].before, vec!["one"]);
        assert_eq!(hits[0].after, vec!["three"]);
    }

    #[test]
    fn parallel_matches_single() {
        let d = scratch("ci-par");
        for i in 0..20 { std::fs::write(d.join(format!("f{i}.rs")), format!("target line {i}\nother")).unwrap(); }
        let files: Vec<PathBuf> = (0..20).map(|i| d.join(format!("f{i}.rs"))).collect();
        let o = Opts { regex: false, ignore_case: false, before: 0, after: 0 };
        let hits = search(&files, "target", &o).unwrap();
        assert_eq!(hits.len(), 20);
    }
}
