/** evals — pi extension over the pure-Rust trace/score/regression binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "evals");
const DEFAULT_DB = join(ROOT, "data", "evals.jsonl");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 30_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "evals",
		label: "Evals",
		description: "Trace logging, scoring, and regression diffing (Langfuse concept). Actions: 'log' records one case trace under a run; 'score' scores a run against expected outputs (matcher exact|contains|regex-lite); 'diff' compares two runs for regressions and new passes; 'runs' lists runs and case counts. Use to measure and track agent quality over time.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["log", "score", "diff", "runs"], description: "Operation to perform" },
				run: { type: "string", description: "run id (log/score)" },
				case: { type: "string", description: "case id (log)" },
				input: { type: "string", description: "case input (log)" },
				output: { type: "string", description: "case output (log)" },
				expected: { type: "string", description: "expected output for scoring (log)" },
				latency_ms: { type: "number", description: "case latency in ms (log)" },
				matcher: { type: "string", description: "score/diff matcher: exact|contains|regex-lite (default exact)" },
				minPass: { type: "number", description: "score: error (nonzero) if accuracy below this 0..1 threshold" },
				failOnly: { type: "boolean", description: "score: list only failing cases" },
				runA: { type: "string", description: "baseline run id (diff)" },
				runB: { type: "string", description: "candidate run id (diff)" },
				db: { type: "string", description: "trace db file (default data/evals.jsonl)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const db = p.db ?? DEFAULT_DB;
			let out: string;
			if (p.action === "log") {
				const args = ["log", "--db", db, "--run", p.run ?? "", "--case", p.case ?? "",
					"--input", p.input ?? "", "--output", p.output ?? ""];
				if (p.expected != null) args.push("--expected", p.expected);
				if (p.latency_ms != null) args.push("--latency-ms", String(p.latency_ms));
				out = run(args);
			} else if (p.action === "score") {
				{ const a = ["score", "--db", db, "--run", p.run ?? "", "--matcher", p.matcher ?? "exact"]; if (p.minPass != null) a.push("--min-pass", String(p.minPass)); if (p.failOnly) a.push("--fail-only"); out = run(a); }
			} else if (p.action === "diff") {
				out = run(["diff", "--db", db, "--run-a", p.runA ?? "", "--run-b", p.runB ?? "", "--matcher", p.matcher ?? "exact"]);
			} else {
				out = run(["runs", "--db", db]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
