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
		description: "Rust code knowledge graph. Actions: 'index' scans a repo dir into a graph (add embed=true for semantic search); 'defs'/'callers'/'refs'/'impls' walk the structural graph for a symbol; 'path' finds a call path between two symbols; 'search' finds symbols by meaning; 'stats' summarizes. Set 'project' to use a workspace repo's OWN graph (workspaces/<project>/.smartagent/codegraph.json) for both indexing and queries — per-repo graphs never clobber each other; omit it for the root SMARTAGENT graph. Use to understand where things are defined and what calls what.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["index", "defs", "callers", "refs", "impls", "path", "search", "stats", "unused"] },
				project: { type: "string", description: "workspace repo name — index/query that repo's own graph instead of the root graph" },
				repo: { type: "string", description: "repo dir to index (index action; ignored when project is set)" },
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
			// project → per-repo graph resolved in Rust; otherwise root graph path.
			const scope = p.project ? ["--project", p.project] : [];
			const graph = p.project ? [] : [GRAPH];
			let out: string;
			if (p.action === "index") {
				out = run(["index", ...(p.project ? [] : [p.repo ?? ".", "--out", GRAPH]), ...scope, ...(p.embed ? ["--embed"] : [])]);
			} else if (p.action === "search") {
				out = run(["search", ...graph, p.query ?? "", "--k", String(p.k ?? 5), ...scope]);
			} else if (p.action === "stats") {
				out = run(["stats", ...graph, ...scope]);
			} else if (p.action === "path") {
				out = run(["path", ...graph, p.name ?? "", p.to ?? "", ...scope]);
			} else {
				out = run([p.action, ...graph, p.name ?? "", ...(p.limit != null ? ["--limit", String(p.limit)] : []), ...scope]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
