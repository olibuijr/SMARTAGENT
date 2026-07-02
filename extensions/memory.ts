/** memory — pi extension over the pure-Rust 3-tier memory binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "memory");
const DIR = join(ROOT, "data", "memory");

function run(args: string[]): string {
	try { return execFileSync(BIN, [...args, "--dir", DIR], { encoding: "utf8", timeout: 90_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "memory",
		label: "Memory",
		description: "Persistent 3-tier memory (working=recent, episodic=events, semantic=durable facts). Actions: 'remember' stores a fact; 'update' overwrites an existing fact by id (dedupe/correct — prefer over remembering a contradiction); 'recall' semantically searches (optionally scoped to one tier); 'recent' lists the newest N in a tier; 'forget' deletes by id; 'promote' moves a fact between tiers; 'stats' shows counts.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["remember", "update", "recall", "recent", "forget", "promote", "stats"] },
				tier: { type: "string", enum: ["working", "episodic", "semantic"], description: "tier for remember/update/recent/forget (default semantic; recent defaults episodic)" },
				text: { type: "string", description: "fact to store (remember/update) or query (recall)" },
				k: { type: "number", description: "result count for recall (default 5)" },
				n: { type: "number", description: "count for recent (default 5)" },
				id: { type: "string", description: "memory id (update/forget; optional for remember)" },
				scope: { type: "string", enum: ["all", "working", "episodic", "semantic"], description: "restrict recall to one tier (default all)" },
				from: { type: "string", description: "source tier (promote)" },
				to: { type: "string", description: "destination tier (promote)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			let out: string;
			switch (p.action) {
				case "remember":
					out = run(["remember", "--tier", p.tier ?? "semantic", "--text", p.text ?? "", ...(p.id ? ["--id", p.id] : [])]);
					break;
				case "update":
					out = run(["remember", "--tier", p.tier ?? "semantic", "--id", p.id ?? "", "--text", p.text ?? ""]);
					break;
				case "recall":
					out = run(["recall", "--text", p.text ?? "", "--k", String(p.k ?? 5), "--tier", p.scope ?? "all"]);
					break;
				case "recent":
					out = run(["recent", "--tier", p.tier ?? "episodic", "--n", String(p.n ?? 5)]);
					break;
				case "forget":
					out = run(["forget", "--tier", p.tier ?? "semantic", "--id", p.id ?? ""]);
					break;
				case "promote":
					out = run(["promote", "--id", p.id ?? "", "--from", p.from ?? "", "--to", p.to ?? ""]);
					break;
				default:
					out = run(["stats"]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
