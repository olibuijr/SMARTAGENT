/** vault — pi extension over the pure-Rust markdown second-brain binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "vault");
const DEFAULT_VAULT = join(ROOT, "data", "vault");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 30_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "vault",
		label: "Vault",
		description: "Markdown second brain (Obsidian pattern) with [[wikilinks]]. Actions: 'new' creates a note, 'read' returns its body, 'append' adds text, 'list' shows all notes, 'links' shows a note's outgoing links and backlinks, 'graph' renders the link graph, 'search' finds notes by keyword. Use to store and connect durable knowledge notes.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["new", "read", "append", "rm", "mv", "list", "links", "graph", "search"], description: "Operation to perform" },
				note: { type: "string", description: "note title/name (new/read/append/rm/links); the OLD name for mv" },
				newName: { type: "string", description: "new title for mv (rewrites [[old]] links)" },
				text: { type: "string", description: "text to append (append action)" },
				query: { type: "string", description: "search query (search action)" },
				vault: { type: "string", description: "vault dir (default data/vault)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const v = p.vault ?? DEFAULT_VAULT;
			let out: string;
			switch (p.action) {
				case "new": out = run(["new", v, p.note ?? ""]); break;
				case "read": out = run(["read", v, p.note ?? ""]); break;
				case "append": out = run(["append", v, p.note ?? "", p.text ?? ""]); break;
				case "rm": out = run(["rm", v, p.note ?? ""]); break;
				case "mv": out = run(["mv", v, p.note ?? "", p.newName ?? ""]); break;
				case "links": out = run(["links", v, p.note ?? ""]); break;
				case "graph": out = run(["graph", v]); break;
				case "search": out = run(["search", v, p.query ?? ""]); break;
				default: out = run(["list", v]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
