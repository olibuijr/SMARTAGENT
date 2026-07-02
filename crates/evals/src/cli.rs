//! CLI: log / score / diff / runs

use httpc::args::flag;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::score::{self, Matcher};
use crate::store::{self, Trace};

fn db(args: &[String]) -> PathBuf {
    flag(args, "--db").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data/evals.semdb"))
}

pub fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("log") => {
            let t = Trace {
                run: flag(args, "--run").ok_or("--run required")?,
                case: flag(args, "--case").ok_or("--case required")?,
                input: flag(args, "--input").unwrap_or_default(),
                output: flag(args, "--output").unwrap_or_default(),
                expected: flag(args, "--expected"),
                latency_ms: flag(args, "--latency-ms").and_then(|s| s.parse().ok()),
            };
            store::append(&db(args), &t)?;
            Ok(format!("logged {}/{}", t.run, t.case))
        }
        Some("score") => {
            let run = flag(args, "--run").ok_or("--run required")?;
            let matcher = Matcher::parse(&flag(args, "--matcher").unwrap_or_else(|| "exact".into()))?;
            let traces = store::load(&db(args));
            let scores = score::score_run(&traces, &run, matcher);
            if scores.is_empty() {
                return Ok(format!("no scored cases for run '{run}' (need --expected on traces)"));
            }
            let acc = score::accuracy(&scores);
            let passed = scores.iter().filter(|s| s.pass).count();
            // --fail-only: list just the failures (typical CI use). Otherwise all.
            let fail_only = args.iter().any(|a| a == "--fail-only");
            // Failures always print; PASS lines cap at 20 (token discipline —
            // a big green suite is summary-enough via the accuracy line).
            let mut lines: Vec<String> = scores.iter()
                .filter(|s| !s.pass)
                .map(|s| format!("FAIL\t{}", s.case))
                .collect();
            if !fail_only {
                let passes: Vec<&str> = scores.iter().filter(|s| s.pass).map(|s| s.case.as_str()).collect();
                for c in passes.iter().take(20) {
                    lines.push(format!("PASS\t{c}"));
                }
                if passes.len() > 20 {
                    lines.push(format!("…[+{} more PASS]", passes.len() - 20));
                }
            }
            lines.push(format!("accuracy: {passed}/{} = {:.1}%", scores.len(), acc * 100.0));
            // Latency aggregates from the stored-but-previously-dead latency_ms.
            let mut lats: Vec<i64> = traces.iter().filter(|t| t.run == run).filter_map(|t| t.latency_ms).collect();
            if !lats.is_empty() {
                lats.sort_unstable();
                let mean = lats.iter().sum::<i64>() / lats.len() as i64;
                let p = |q: f64| lats[((lats.len() as f64 - 1.0) * q) as usize];
                lines.push(format!("latency: n={} mean={}ms p50={}ms p95={}ms", lats.len(), mean, p(0.5), p(0.95)));
            }
            let report = lines.join("\n");
            // --min-pass N: nonzero exit (Err) if accuracy is below the threshold,
            // so a scheduler/CI gate can act without parsing the number.
            if let Some(min) = flag(args, "--min-pass").and_then(|s| s.parse::<f64>().ok()) {
                if acc < min {
                    return Err(format!("{report}\nBELOW THRESHOLD: {:.3} < {:.3}", acc, min));
                }
            }
            Ok(report)
        }
        Some("diff") => {
            let a = flag(args, "--run-a").or_else(|| flag(args, "--a")).ok_or("--run-a required")?;
            let b = flag(args, "--run-b").or_else(|| flag(args, "--b")).ok_or("--run-b required")?;
            let matcher = Matcher::parse(&flag(args, "--matcher").unwrap_or_else(|| "exact".into()))?;
            let traces = store::load(&db(args));
            // Score each run ONCE (was recomputed 4× for accuracy + maps).
            let scored_a = score::score_run(&traces, &a, matcher);
            let scored_b = score::score_run(&traces, &b, matcher);
            let sa: BTreeMap<String, bool> = scored_a.iter().map(|s| (s.case.clone(), s.pass)).collect();
            let sb: BTreeMap<String, bool> = scored_b.iter().map(|s| (s.case.clone(), s.pass)).collect();
            let mut regressions = Vec::new();
            let mut fixes = Vec::new();
            for (case, &pa) in &sa {
                if let Some(&pb) = sb.get(case) {
                    if pa && !pb { regressions.push(case.clone()); }
                    if !pa && pb { fixes.push(case.clone()); }
                }
            }
            let acc_a = score::accuracy(&scored_a);
            let acc_b = score::accuracy(&scored_b);
            let mut out = vec![format!("accuracy {a}: {:.1}%  →  {b}: {:.1}%  (Δ {:+.1}%)", acc_a*100.0, acc_b*100.0, (acc_b-acc_a)*100.0)];
            out.push(format!("regressions ({}): {}", regressions.len(), if regressions.is_empty() { "none".into() } else { regressions.join(", ") }));
            out.push(format!("new passes ({}): {}", fixes.len(), if fixes.is_empty() { "none".into() } else { fixes.join(", ") }));
            Ok(out.join("\n"))
        }
        Some("statusline") => {
            // `level|text` for UI statuslines: pass ratio of the latest run
            // (latest = last run id in trace order).
            let traces = store::load(&db(args));
            let last_run = match traces.last().map(|t| t.run.clone()) {
                Some(r) => r,
                None => return Ok("warn|📊 no runs".into()),
            };
            let scores = score::score_run(&traces, &last_run, Matcher::Exact);
            if scores.is_empty() {
                return Ok(format!("warn|📊 {last_run}: unscored"));
            }
            let passed = scores.iter().filter(|s| s.pass).count();
            let level = if passed == scores.len() { "ok" } else if passed * 2 >= scores.len() { "warn" } else { "err" };
            Ok(format!("{level}|📊 {passed}/{} {last_run}", scores.len()))
        }
        Some("runs") => {
            let traces = store::load(&db(args));
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for t in &traces { *counts.entry(t.run.clone()).or_default() += 1; }
            if counts.is_empty() { return Ok("no runs".into()); }
            Ok(counts.iter().map(|(r, n)| format!("{r}\t{n} cases")).collect::<Vec<_>>().join("\n"))
        }
        Some("triage") => {
            // Self-heal loop ingestion: failing runs → deduped, criteria-gated
            // board tasks the autonomous fleet pulls. Fired by the stop hook.
            let tasks_db = flag(args, "--tasks-db").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data/tasks.semdb"));
            let dry = args.iter().any(|a| a == "--dry-run");
            let all = args.iter().any(|a| a == "--all");
            crate::triage::triage(&db(args), &tasks_db, dry, all)
        }
        _ => Ok(HELP.trim().into()),
    }
}


const HELP: &str = r#"
evals — trace, score, and regression-diff (Langfuse concept)

USAGE:
  evals log   [--db data/evals.semdb] --run R --case ID --input '..' --output '..' [--expected '..'] [--latency-ms N]
  evals score [--db data/evals.semdb] --run R [--matcher exact|contains|regex-lite]
  evals diff  [--db data/evals.semdb] --run-a A --run-b B [--matcher ...]
  evals runs  [--db data/evals.semdb]
  evals triage [--db data/evals.semdb] [--tasks-db data/tasks.semdb] [--dry-run]
              failing runs → deduped board tasks (self-heal loop; p1 escalation
              on re-failure after a completed fix; skips runs owned by open T-n)

Storage: eval traces live in a semdb table. Legacy --db paths ending in
*.jsonl are accepted for compatibility and transparently map to *.semdb.
"#;
