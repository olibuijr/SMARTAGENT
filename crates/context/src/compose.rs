//! Compose / validate / stat over a priority-ordered context directory.

use std::path::Path;

/// Read the ORDER file: one filename per line, highest priority first.
/// If absent, fall back to alphabetical order of *.md files.
fn ordered_files(dir: &Path) -> Result<Vec<String>, String> {
    let order = super::order_path(dir);
    if let Ok(text) = std::fs::read_to_string(&order) {
        return Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect());
    }
    // Fallback: alphabetical .md files.
    let mut names = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for e in entries {
        let p = e.map_err(|x| x.to_string())?.path();
        if p.extension().map(|x| x == "md").unwrap_or(false) {
            if let Some(n) = p.file_name() {
                names.push(n.to_string_lossy().to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

pub fn compose(dir: &Path, budget: usize) -> Result<String, String> {
    let files = ordered_files(dir)?;
    // Load contents in priority order.
    let mut sections: Vec<(String, String)> = Vec::new();
    for name in &files {
        if let Ok(body) = std::fs::read_to_string(dir.join(name)) {
            sections.push((name.clone(), body));
        }
    }
    // Greedily include from highest priority; trim the last one that overflows.
    let mut out = String::new();
    for (name, body) in &sections {
        let header = format!("## {name}\n");
        let block = format!("{header}{}\n\n", body.trim_end());
        if out.chars().count() + block.chars().count() <= budget {
            out.push_str(&block);
        } else {
            // Trim this block to remaining budget (UTF-8 safe), then stop.
            let remaining = budget.saturating_sub(out.chars().count());
            if remaining > header.chars().count() + 8 {
                let keep = remaining - 4;
                let trimmed: String = block.chars().take(keep).collect();
                out.push_str(&trimmed);
                out.push_str(" …");
            }
            break;
        }
    }
    Ok(out.trim_end().to_string())
}

pub fn validate(dir: &Path) -> Result<String, String> {
    let files = ordered_files(dir)?;
    let mut report = Vec::new();
    for name in &files {
        let path = dir.join(name);
        let status = match std::fs::read_to_string(&path) {
            Ok(b) if b.trim().is_empty() => "EMPTY",
            Ok(_) => "ok",
            Err(_) => "MISSING",
        };
        report.push(format!("{status}\t{name}"));
    }
    Ok(report.join("\n"))
}

pub fn stat(dir: &Path) -> Result<String, String> {
    let files = ordered_files(dir)?;
    let mut out = Vec::new();
    let mut total = 0;
    for name in &files {
        let n = std::fs::read_to_string(dir.join(name)).map(|s| s.chars().count()).unwrap_or(0);
        total += n;
        out.push(format!("{n}\t{name}"));
    }
    out.push(format!("{total}\tTOTAL"));
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn trims_lowest_priority_first() {
        let d = scratch("ctx-trim");
        std::fs::write(d.join("ORDER"), "high.md\nlow.md").unwrap();
        std::fs::write(d.join("high.md"), "AAAA").unwrap();
        std::fs::write(d.join("low.md"), "B".repeat(1000)).unwrap();
        let out = compose(&d, 40).unwrap();
        assert!(out.contains("AAAA"));
        // low.md content should be largely dropped/trimmed
        assert!(out.matches('B').count() < 100);
    }

    #[test]
    fn utf8_safe_trim() {
        let d = scratch("ctx-utf8");
        std::fs::write(d.join("ORDER"), "ice.md").unwrap();
        std::fs::write(d.join("ice.md"), " öæð".repeat(100)).unwrap();
        // Must not panic on a char boundary mid-trim.
        let out = compose(&d, 30).unwrap();
        assert!(out.chars().count() <= 32);
    }

    #[test]
    fn validate_reports_missing_and_empty() {
        let d = scratch("ctx-val");
        std::fs::write(d.join("ORDER"), "a.md\nb.md\nc.md").unwrap();
        std::fs::write(d.join("a.md"), "content").unwrap();
        std::fs::write(d.join("b.md"), "   ").unwrap();
        let r = validate(&d).unwrap();
        assert!(r.contains("ok\ta.md"));
        assert!(r.contains("EMPTY\tb.md"));
        assert!(r.contains("MISSING\tc.md"));
    }
}
