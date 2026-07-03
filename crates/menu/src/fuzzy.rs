//! Live fuzzy filter: subsequence match with word-boundary/contiguity bonuses.
//! Ported in spirit from Hermes `curses_ui.py` (`_fuzzy_score`).

/// Returns Some(score) when every char of `query` appears in `label` in order
/// (case-insensitive), None otherwise. Higher score = better match. An empty
/// query scores 0 for everything (keeps original order).
pub fn score(label: &str, query: &str) -> Option<f64> {
    if query.is_empty() {
        return Some(0.0);
    }
    let l: Vec<char> = label.chars().collect();
    let lower: Vec<char> = label.chars().flat_map(|c| c.to_lowercase()).collect();
    // to_lowercase can change length; fall back to char-by-char lower.
    let lower: Vec<char> = if lower.len() == l.len() {
        lower
    } else {
        l.iter().map(|c| c.to_ascii_lowercase()).collect()
    };
    let q: Vec<char> = query.to_lowercase().chars().collect();

    let mut qi = 0usize;
    let mut total = 0.0f64;
    let mut prev_match: Option<usize> = None;

    for (i, &ch) in lower.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if ch == q[qi] {
            let mut s = 1.0;
            // Contiguous with the previous matched char.
            if let Some(p) = prev_match {
                if p + 1 == i {
                    s += 2.0;
                }
            }
            // Word boundary (start, after separator, or camelCase upper).
            if is_boundary(&l, i) {
                s += 3.0;
            }
            total += s;
            prev_match = Some(i);
            qi += 1;
        }
    }

    if qi == q.len() {
        // Prefer shorter labels on ties.
        Some(total - (l.len() as f64) * 0.001)
    } else {
        None
    }
}

fn is_boundary(chars: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = chars[i - 1];
    if matches!(prev, '-' | '_' | '/' | '.' | ' ' | ':') {
        return true;
    }
    // lower -> upper camelCase transition
    prev.is_lowercase() && chars[i].is_uppercase()
}

/// Filter+rank `items` by `query`, returning the surviving indices best-first.
/// Stable within equal scores (preserves original order).
pub fn filter(items: &[String], query: &str) -> Vec<usize> {
    let mut scored: Vec<(usize, f64)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, s)| score(s, query).map(|sc| (i, sc)))
        .collect();
    // stable sort by score desc; equal scores keep original index order.
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    scored.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_only() {
        assert!(score("gateway", "gw").is_some());
        assert!(score("gateway", "xz").is_none());
        assert!(score("gateway", "").is_some());
    }

    #[test]
    fn boundary_beats_middle() {
        // "tb" matches "tasks-board" at boundaries — should beat "fooTbar".
        let a = score("tasks-board", "tb").unwrap();
        let b = score("footbar", "tb").unwrap();
        assert!(a > b, "{a} !> {b}");
    }

    #[test]
    fn filter_ranks_and_drops() {
        let items: Vec<String> = ["gateway", "tasks", "settings"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = filter(&items, "set");
        assert_eq!(out, vec![2]); // only "settings"
    }
}
