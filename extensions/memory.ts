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
		description: "Persistent 3-tier agent memory. Actions: 'remember' stores a fact in a tier (working=recent, episodic=events, semantic=durable facts); 'recall' semantically searches memories for a query; 'stats' shows counts. Use to retain and recall facts about the user and work across sessions.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["remember", "recall", "stats"] },
				tier: { type: "string", enum: ["working", "episodic", "semantic"], description: "tier for remember (default semantic)" },
				text: { type: "string", description: "fact to store or query to recall" },
				k: { type: "number", description: "result count for recall (default 5)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const out = p.action === "remember"
				? run(["remember", "--tier", p.tier ?? "semantic", "--text", p.text ?? ""])
				: p.action === "recall"
					? run(["recall", "--text", p.text ?? "", "--k", String(p.k ?? 5)])
					: run(["stats"]);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
