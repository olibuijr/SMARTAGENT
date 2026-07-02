//! Vector parsing and similarity math.

pub fn parse_vec(s: &str) -> Result<Vec<f32>, String> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        out.push(p.parse::<f32>().map_err(|e| format!("bad float '{p}': {e}"))?);
    }
    if out.is_empty() {
        return Err("empty vector".into());
    }
    Ok(out)
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub fn norm(v: &[f32]) -> f32 {
    dot(v, v).sqrt()
}

/// Cosine similarity for arbitrary (unnormalized) vectors. 0.0 if either is zero.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (na, nb) = (norm(a), norm(b));
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot(a, b) / (na * nb)
}

/// Return a unit-length copy (zero vector stays zero).
pub fn normalized(v: &[f32]) -> Vec<f32> {
    let n = norm(v);
    if n == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_measures() {
        let v = parse_vec("1, 0, 0").unwrap();
        let w = parse_vec("0,1,0").unwrap();
        assert_eq!(v.len(), 3);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
        assert!(cosine(&v, &w).abs() < 1e-6);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_vec("a,b").is_err());
        assert!(parse_vec("").is_err());
    }
}
