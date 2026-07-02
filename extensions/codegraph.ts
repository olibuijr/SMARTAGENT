/** codegraph — pi extension over the pure-Rust code knowledge graph binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "codegraph");
const GRAPH = join(ROOT, "data", "codegraph.json");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "codegraph",
		label: "Code Graph",
		description: "Rust code knowledge graph. Actions: 'index' scans a repo dir into a graph (add embed=true for semantic search); 'defs'/'callers'/'refs'/'impls' walk the structural graph for a symbol; 'path' finds a call path between two symbols; 'search' finds symbols by meaning; 'stats' summarizes. Use to understand where things are defined and what calls what.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["index", "defs", "callers", "refs", "impls", "path", "search", "stats", "unused"] },
				repo: { type: "string", description: "repo dir to index (index action)" },
				embed: { type: "boolean", description: "build semantic index too (index action)" },
				name: { type: "string", description: "symbol name (defs/callers/refs/impls); the FROM symbol for path" },
				to: { type: "string", description: "TO symbol (path — finds a call path from name→to)" },
				limit: { type: "number", description: "cap defs/callers/refs/impls results (default 50)" },
				query: { type: "string", description: "semantic query (search)" },
				k: { type: "number", description: "result count for search (default 5)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			let out: string;
			if (p.action === "index") {
				out = run(["index", p.repo ?? ".", "--out", GRAPH, ...(p.embed ? ["--embed"] : [])]);
			} else if (p.action === "search") {
				out = run(["search", GRAPH, p.query ?? "", "--k", String(p.k ?? 5)]);
			} else if (p.action === "stats") {
				out = run(["stats", GRAPH]);
			} else if (p.action === "path") {
				out = run(["path", GRAPH, p.name ?? "", p.to ?? ""]);
			} else {
				out = run([p.action, GRAPH, p.name ?? "", ...(p.limit != null ? ["--limit", String(p.limit)] : [])]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
