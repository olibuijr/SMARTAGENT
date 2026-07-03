/** vault — pi extension over the pure-Rust markdown second-brain binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";
import { bin, makeRunner, ROOT } from "./lib/common.ts";

const DEFAULT_VAULT = join(ROOT, "data", "vault");
const run = makeRunner(bin("vault"), 30_000);

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "vault",
		label: "Vault",
		description: "Markdown second brain (Obsidian pattern) with [[wikilinks]]. Actions: 'new' creates a note, 'read' returns its body, 'append' adds text, 'list' shows all notes, 'links' shows a note's outgoing links and backlinks, 'graph' renders the link graph, 'search' finds notes by keyword. Use to store and connect durable knowledge notes.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["new", "read", "append", "rm", "mv", "list", "links", "graph", "search", "orphans", "tags"], description: "Operation to perform" },
				note: { type: "string", description: "note title/name (new/read/append/rm/links); the OLD name for mv" },
				newName: { type: "string", description: "new title for mv (rewrites [[old]] links)" },
				text: { type: "string", description: "text to append (append action)" },
				tag: { type: "string", description: "search: filter by tag instead of keyword" },
				query: { type: "string", description: "search query (search action)" },
				head: { type: "number", description: "read: first N lines only (notes grow via append)" },
				graphNote: { type: "string", description: "graph: scope to this note's neighborhood" },
				depth: { type: "number", description: "graph: hops from graphNote (default 1)" },
				vault: { type: "string", description: "vault dir (default data/vault)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const v = p.vault ?? DEFAULT_VAULT;
			let out: string;
			switch (p.action) {
				case "new": out = run(["new", v, p.note ?? ""]); break;
				case "read": out = run(["read", v, p.note ?? "", ...(p.head != null ? ["--head", String(p.head)] : [])]); break;
				case "append": out = run(["append", v, p.note ?? "", p.text ?? ""]); break;
				case "rm": out = run(["rm", v, p.note ?? ""]); break;
				case "mv": out = run(["mv", v, p.note ?? "", p.newName ?? ""]); break;
				case "links": out = run(["links", v, p.note ?? ""]); break;
				case "graph": out = run(["graph", v, ...(p.graphNote ? ["--note", p.graphNote, "--depth", String(p.depth ?? 1)] : [])]); break;
				case "search": out = p.tag ? run(["search", v, "--tag", p.tag]) : run(["search", v, p.query ?? ""]); break;
				case "orphans": out = run(["orphans", v]); break;
				case "tags": out = run(["tags", v]); break;
				default: out = run(["list", v]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
