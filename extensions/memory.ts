/** memory — pi extension over the pure-Rust 3-tier memory binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";
import { bin, runFile, ROOT } from "./lib/common.ts";

const BIN = bin("memory");
const DIR = join(ROOT, "data", "memory");

function run(args: string[], project?: string): string {
	// --project = that workspace repo's own memory (.smartagent/memory);
	// otherwise the agent-global store.
	const scope = project ? ["--project", project] : ["--dir", DIR];
	return runFile(BIN, [...args, ...scope], 90_000);
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "memory",
		label: "Memory",
		description: "3-tier memory (working/episodic/semantic) plus medvitund self-history recall. remember, update (correct by id — prefer over contradicting), recall (semantic search, tier scoped), recent, forget, promote, medvitund, stats. project = a workspace repo's own store (repo facts live there, not globally).",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["remember", "update", "recall", "recent", "forget", "promote", "medvitund", "stats"] },
				tier: { type: "string", enum: ["working", "episodic", "semantic"], description: "tier for remember/update/recent/forget (default semantic; recent defaults episodic)" },
				text: { type: "string", description: "fact to store (remember/update) or query (recall/medvitund)" },
				k: { type: "number", description: "result count for recall (default 5)" },
				n: { type: "number", description: "count for recent (default 5)" },
				id: { type: "string", description: "memory id (update/forget; optional for remember)" },
				scope: { type: "string", enum: ["all", "working", "episodic", "semantic"], description: "restrict recall to one tier (default all)" },
				from: { type: "string", description: "source tier (promote)" },
				to: { type: "string", description: "destination tier (promote)" },
				project: { type: "string", description: "workspace repo name — use that repo's own memory store instead of the global one" },
				since: { type: "string", description: "medvitund lower bound: unix seconds/ns or YYYY-MM-DD day" },
				agent: { type: "string", description: "medvitund agent name filter" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const r = (a: string[]) => run(a, p.project);
			let out: string;
			switch (p.action) {
				case "remember":
					out = r(["remember", "--tier", p.tier ?? "semantic", "--text", p.text ?? "", ...(p.id ? ["--id", p.id] : [])]);
					break;
				case "update":
					out = r(["remember", "--tier", p.tier ?? "semantic", "--id", p.id ?? "", "--text", p.text ?? ""]);
					break;
				case "recall":
					out = r(["recall", "--text", p.text ?? "", "--k", String(p.k ?? 5), "--tier", p.scope ?? "all"]);
					break;
				case "recent":
					out = r(["recent", "--tier", p.tier ?? "episodic", "--n", String(p.n ?? 5)]);
					break;
				case "forget":
					out = r(["forget", "--tier", p.tier ?? "semantic", "--id", p.id ?? ""]);
					break;
				case "promote":
					out = r(["promote", "--id", p.id ?? "", "--from", p.from ?? "", "--to", p.to ?? ""]);
					break;
				case "medvitund":
					out = run(["medvitund", "--n", String(p.n ?? 10), ...(p.text ? ["--query", p.text] : []), ...(p.since ? ["--since", p.since] : []), ...(p.agent ? ["--agent", p.agent] : [])]);
					break;
				default:
					out = r(["stats"]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
