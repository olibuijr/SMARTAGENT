//! CLI: query / health

use httpc::args::flag;
use crate::searx::{self, Query};

pub fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("query") => {
            let terms = args.get(1).ok_or("usage: search query '<terms>' [--instance URL]")?;
            let instance = resolve_instance(args)?;
            let limit = flag(args, "--k").and_then(|s| s.parse().ok()).unwrap_or(10);
            let engines = flag(args, "--engines");
            let category = flag(args, "--category");
            let time_range = flag(args, "--time-range");
            // --site restricts to one domain (SearXNG honors `site:` in the query).
            let scoped_terms = match flag(args, "--site") {
                Some(d) => format!("site:{d} {terms}"),
                None => terms.to_string(),
            };
            let q = Query {
                instance: &instance,
                terms: &scoped_terms,
                engines: engines.as_deref(),
                category: category.as_deref(),
                time_range: time_range.as_deref(),
                limit,
            };
            let results = searx::search(&q)?;
            if results.is_empty() {
                return Ok("no results".into());
            }
            Ok(results
                .iter()
                .enumerate()
                .map(|(i, r)| format!("{}\t{}\t{}\t{}", i + 1, r.title, r.url, r.snippet))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("health") => {
            let instance = resolve_instance(args)?;
            let status = searx::health(&instance)?;
            Ok(format!("instance {instance} → HTTP {status}"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

/// Instance resolution: --instance flag → $SEARX_INSTANCE → config searx_instance.
fn resolve_instance(args: &[String]) -> Result<String, String> {
    if let Some(i) = flag(args, "--instance") {
        return Ok(i);
    }
    semdb::config::Config::load()
        .resolve("searx_instance", "SEARX_INSTANCE", None)
        .ok_or_else(|| "--instance required (or set searx_instance in config)".into())
}


const HELP: &str = r#"
search — SearXNG metasearch client

USAGE:
  search query '<terms>' [--instance http://host:port] [--k 10]
                          [--engines e1,e2] [--category general|news|it]
  search health [--instance http://host:port]

Instance resolution: --instance → $SEARX_INSTANCE → config searx_instance.
"#;

#[cfg(test)]
mod neg_tests {
    use super::*;

    #[test]
    fn rejects_bad_args() {
        let s=|v:&[&str]|v.iter().map(|x|x.to_string()).collect::<Vec<_>>();
        assert!(run(&s(&["query"])).is_err());  // missing terms
        // unknown subcommand falls through to help (Ok), not an error:
        assert!(run(&s(&["bogus"])).is_ok());
    }

}
