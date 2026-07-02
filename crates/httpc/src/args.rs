//! Shared CLI argument helpers. Previously copy-pasted (identically) into ~15
//! crates; centralized here so a change (e.g. the `--name=value` form) lands
//! once. All tool crates already depend on semdb, so this needs no new crate.

/// Value of `--name X` or `--name=X`; None if absent or missing a value.
pub fn flag(args: &[String], name: &str) -> Option<String> {
    let eq = format!("{name}=");
    for (i, a) in args.iter().enumerate() {
        if a == name {
            return args.get(i + 1).cloned();
        }
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v.to_string());
        }
    }
    None
}

/// True if the boolean flag `name` is present.
pub fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn space_and_equals_forms() {
        assert_eq!(flag(&v(&["--k", "5"]), "--k").as_deref(), Some("5"));
        assert_eq!(flag(&v(&["--k=7"]), "--k").as_deref(), Some("7"));
        assert_eq!(flag(&v(&["--other", "x"]), "--k"), None);
        assert!(has(&v(&["--net"]), "--net"));
        assert!(!has(&v(&["--x"]), "--net"));
    }
}
