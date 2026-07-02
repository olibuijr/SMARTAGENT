//! Minimal URL parsing: scheme://host[:port]/path?query

#[derive(Debug, Clone, PartialEq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// Path + query, always starting with '/'.
    pub path: String,
}

impl Url {
    pub fn parse(input: &str) -> Result<Url, String> {
        let (scheme, rest) = input
            .split_once("://")
            .ok_or_else(|| format!("no scheme in '{input}'"))?;
        if scheme != "http" && scheme != "https" {
            return Err(format!(
                "unsupported scheme '{scheme}' (expected http or https)"
            ));
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err(format!("no host in '{input}'"));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
                (h.to_string(), p.parse::<u16>().map_err(|e| e.to_string())?)
            }
            _ => (
                authority.to_string(),
                if scheme == "https" { 443 } else { 80 },
            ),
        };
        Ok(Url {
            scheme: scheme.to_string(),
            host,
            port,
            path: path.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full() {
        let u = Url::parse("http://titan:8081/v1/models?x=1").unwrap();
        assert_eq!(u.host, "titan");
        assert_eq!(u.port, 8081);
        assert_eq!(u.path, "/v1/models?x=1");
    }

    #[test]
    fn defaults() {
        let u = Url::parse("http://example.com").unwrap();
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/");
    }

    #[test]
    fn rejects_junk() {
        assert!(Url::parse("ftp://x").is_err());
        assert!(Url::parse("nourl").is_err());
        assert!(Url::parse("http://").is_err());
    }
}
