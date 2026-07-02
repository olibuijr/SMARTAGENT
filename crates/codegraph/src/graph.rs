//! Graph model: symbols + edges, with JSON persistence (via semdb's json)
//! and the query surface (defs / refs / callers).

use std::path::Path;

use semdb::json::{self, Value};

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub kind: String, // defines | calls | impls
    pub from: String,
    pub to: String,
}

pub struct Graph {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
}

impl Graph {
    pub fn new() -> Graph {
        Graph { symbols: Vec::new(), edges: Vec::new() }
    }

    pub fn add_symbol(&mut self, s: Symbol) {
        self.symbols.push(s);
    }

    pub fn add_edge(&mut self, e: Edge) {
        self.edges.push(e);
    }

    /// Where a symbol is defined (name → list of file:line kind).
    pub fn defs(&self, name: &str) -> Vec<String> {
        self.symbols
            .iter()
            .filter(|s| s.name == name && !s.file.is_empty())
            .map(|s| format!("{}:{}\t{}", s.file, s.line, s.kind))
            .collect()
    }

    /// Types that implement `trait_name` (impls edges to → from).
    pub fn impls(&self, trait_name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .edges
            .iter()
            .filter(|e| e.kind == "impls" && e.to == trait_name)
            .map(|e| e.from.clone())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// A shortest call path from `from` to `to` (BFS over "calls" edges).
    /// Returns the chain of symbol names, or empty if unreachable.
    pub fn call_path(&self, from: &str, to: &str) -> Vec<String> {
        use std::collections::{HashMap, VecDeque};
        if from == to {
            return vec![from.to_string()];
        }
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &self.edges {
            if e.kind == "calls" {
                adj.entry(&e.from).or_default().push(&e.to);
            }
        }
        let mut prev: HashMap<&str, &str> = HashMap::new();
        let mut q = VecDeque::new();
        q.push_back(from);
        prev.insert(from, from);
        while let Some(cur) = q.pop_front() {
            if let Some(nexts) = adj.get(cur) {
                for &n in nexts {
                    if !prev.contains_key(n) {
                        prev.insert(n, cur);
                        if n == to {
                            // Reconstruct.
                            let mut chain = vec![to];
                            let mut c = to;
                            while c != from {
                                c = prev[c];
                                chain.push(c);
                            }
                            chain.reverse();
                            return chain.into_iter().map(String::from).collect();
                        }
                        q.push_back(n);
                    }
                }
            }
        }
        Vec::new()
    }

    /// Functions that call `name`.
    pub fn callers(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .edges
            .iter()
            .filter(|e| e.kind == "calls" && e.to == name)
            .map(|e| e.from.clone())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// All references to `name`: calls to it + impls involving it.
    pub fn refs(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .edges
            .iter()
            .filter(|e| (e.kind == "calls" || e.kind == "impls") && (e.to == name))
            .map(|e| format!("{}\t{}", e.kind, e.from))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn stats(&self) -> String {
        let files: std::collections::BTreeSet<&str> =
            self.symbols.iter().map(|s| s.file.as_str()).filter(|f| !f.is_empty()).collect();
        let calls = self.edges.iter().filter(|e| e.kind == "calls").count();
        let impls = self.edges.iter().filter(|e| e.kind == "impls").count();
        format!("files: {}\nsymbols: {}\nedges: {} (calls {}, impls {})", files.len(), self.symbols.len(), self.edges.len(), calls, impls)
    }

    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"symbols\":[");
        for (i, s) in self.symbols.iter().enumerate() {
            if i > 0 { out.push(','); }
            out.push_str(&format!(
                r#"{{"name":"{}","kind":"{}","file":"{}","line":{}}}"#,
                json::escape(&s.name), json::escape(&s.kind), json::escape(&s.file), s.line
            ));
        }
        out.push_str("],\"edges\":[");
        for (i, e) in self.edges.iter().enumerate() {
            if i > 0 { out.push(','); }
            out.push_str(&format!(
                r#"{{"kind":"{}","from":"{}","to":"{}"}}"#,
                json::escape(&e.kind), json::escape(&e.from), json::escape(&e.to)
            ));
        }
        out.push_str("]}");
        out
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, self.to_json()).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Graph, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let v = json::parse(&text).map_err(|e| format!("bad graph json: {e}"))?;
        let mut g = Graph::new();
        if let Some(arr) = v.get("symbols").and_then(Value::as_arr) {
            for s in arr {
                g.symbols.push(Symbol {
                    name: str_field(s, "name"),
                    kind: str_field(s, "kind"),
                    file: str_field(s, "file"),
                    line: s.get("line").and_then(Value::as_f64).unwrap_or(0.0) as usize,
                });
            }
        }
        if let Some(arr) = v.get("edges").and_then(Value::as_arr) {
            for e in arr {
                g.edges.push(Edge { kind: str_field(e, "kind"), from: str_field(e, "from"), to: str_field(e, "to") });
            }
        }
        Ok(g)
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let mut g = Graph::new();
        g.add_symbol(Symbol { name: "foo".into(), kind: "fn".into(), file: "a.rs".into(), line: 3 });
        g.add_edge(Edge { kind: "calls".into(), from: "main".into(), to: "foo".into() });
        let json = g.to_json();
        let parsed = json::parse(&json).unwrap();
        assert!(parsed.get("symbols").is_some());
        // reload
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("cg-roundtrip.json");
        g.save(&p).unwrap();
        let g2 = Graph::load(&p).unwrap();
        assert_eq!(g2.defs("foo"), vec!["a.rs:3\tfn"]);
        assert_eq!(g2.callers("foo"), vec!["main"]);
    }
}
