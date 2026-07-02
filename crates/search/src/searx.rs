//! SearXNG JSON API client. We host SearXNG; this is the client surface only.

use httpc::json::Value;

use crate::encode::percent_encode;

pub struct Result {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct Query<'a> {
    pub instance: &'a str,
    /// SearXNG results page (1-based); default 1.
    pub pageno: usize,
    pub terms: &'a str,
    pub engines: Option<&'a str>,
    pub category: Option<&'a str>,
    /// SearXNG time filter: day|week|month|year.
    pub time_range: Option<&'a str>,
    pub limit: usize,
}

pub fn build_url(q: &Query) -> String {
    let mut url = format!(
        "{}/search?q={}&format=json",
        q.instance.trim_end_matches('/'),
        percent_encode(q.terms)
    );
    if let Some(e) = q.engines {
        url.push_str(&format!("&engines={}", percent_encode(e)));
    }
    if let Some(c) = q.category {
        url.push_str(&format!("&categories={}", percent_encode(c)));
    }
    if let Some(t) = q.time_range {
        url.push_str(&format!("&time_range={}", percent_encode(t)));
    }
    if q.pageno > 1 {
        url.push_str(&format!("&pageno={}", q.pageno));
    }
    url
}

/// Model-supplied instance URLs must be http(s) — anything else (or a
/// scheme-less internal host) is an SSRF vector.
pub fn validate_instance(instance: &str) -> std::result::Result<(), String> {
    if instance.starts_with("http://") || instance.starts_with("https://") {
        Ok(())
    } else {
        Err(format!("instance must start with http:// or https:// (got '{instance}')"))
    }
}

pub fn search(q: &Query) -> ResultList {
    validate_instance(q.instance)?;
    let url = build_url(q);
    // Explicit timeout: a hung SearXNG must not block until the extension's 60s kill.
    let resp = httpc::request("GET", &url).timeout(20).send().map_err(|e| format!("request failed: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status));
    }
    let json = resp.json().map_err(|e| format!("bad JSON: {e}"))?;
    Ok(parse_results(&json, q.limit))
}

pub type ResultList = std::result::Result<Vec<Result>, String>;

pub fn parse_results(json: &Value, limit: usize) -> Vec<Result> {
    let arr = match json.get("results").and_then(|r| r.as_arr()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .take(limit)
        .map(|r| Result {
            title: r.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            url: r.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            snippet: r.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        })
        .collect()
}

pub fn health(instance: &str) -> std::result::Result<u16, String> {
    validate_instance(instance)?;
    let resp = httpc::request("GET", instance.trim_end_matches('/')).timeout(10).send()?;
    Ok(resp.status)
}

#[cfg(test)]
mod scheme_tests {
    #[test]
    fn rejects_non_http_instance() {
        assert!(super::validate_instance("file:///etc/passwd").is_err());
        assert!(super::validate_instance("192.168.1.1:8080").is_err());
        assert!(super::validate_instance("http://searx.local").is_ok());
        assert!(super::validate_instance("https://searx.local").is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_url() {
        let q = Query { instance: "http://searx:8888/", pageno: 1, terms: "rust lang", engines: Some("google,bing"), category: None, time_range: None, limit: 10 };
        let u = build_url(&q);
        assert_eq!(u, "http://searx:8888/search?q=rust%20lang&format=json&engines=google%2Cbing");
    }

    #[test]
    fn parses_results() {
        let json = httpc::json::parse(r#"{"results":[{"title":"A","url":"http://a","content":"snip"},{"title":"B","url":"http://b","content":"x"}]}"#).unwrap();
        let r = parse_results(&json, 1);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "A");
        assert_eq!(r[0].url, "http://a");
    }

    #[test]
    fn empty_when_no_results_key() {
        let json = httpc::json::parse(r#"{"error":"nope"}"#).unwrap();
        assert!(parse_results(&json, 10).is_empty());
    }
}
