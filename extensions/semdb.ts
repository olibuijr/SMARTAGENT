/**
 * semdb — pi extension glue over the pure-Rust semantic database binary.
 * No logic here: every call shells out to target/release/semdb.
 * No runtime imports from pi packages — type-only imports + node builtins,
 * plain JSON-schema parameters (same shape TypeBox produces).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "semdb");
const DEFAULT_DB = join(ROOT, "data", "smartagent.semdb");

function run(args: string[]): string {
	try {
		return execFileSync(BIN, args, { encoding: "utf8", timeout: 90_000 }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "semdb",
		label: "Semantic DB",
		description:
			"Project semantic database (vector store with external embeddings). " +
			"Actions: 'embed' stores text under an id (auto-embeds), 'search' finds the " +
			"k most similar entries for a text query, 'get'/'del' fetch or remove by id, " +
			"'stats' shows entry counts. Use it to remember facts and recall them semantically.",
		parameters: {
			type: "object",
			properties: {
				action: {
					type: "string",
					enum: ["embed", "search", "get", "del", "count", "ids", "stats"],
					description: "Operation to perform",
				},
				id: { type: "string", description: "Entry id (embed/get/del)" },
				text: { type: "string", description: "Text to embed or search for" },
				k: { type: "number", description: "Result count for search (default 5)" },
				idsOnly: { type: "boolean", description: "search: score+id only (skip meta text)" },
				metaChars: { type: "number", description: "search: truncate meta blob in results" },
				filter: { type: "string", description: "search: keep only rows whose meta has key=value" },
				prefix: { type: "string", description: "count/ids: id prefix filter; del: delete all ids with prefix" },
				db: { type: "string", description: "Database file (default data/smartagent.semdb)" },
			},
			required: ["action"],
		} as any,

		async execute(_toolCallId: string, p: any) {
			const db = p.db ?? DEFAULT_DB;
			if (!existsSync(db)) run(["create", db]);
			let out: string;
			switch (p.action) {
				case "embed":
					out = run(["embed", db, "--id", p.id ?? "", "--text", p.text ?? ""]);
					break;
				case "search":
					{ const a=["search", db, "--text", p.text ?? "", "--k", String(p.k ?? 5)]; if (p.idsOnly) a.push("--ids-only"); if (p.metaChars!=null) a.push("--meta-chars", String(p.metaChars)); if (p.filter) a.push("--filter", p.filter); out = run(a); }
					break;
				case "count": out = run(["count", db, ...(p.prefix ? ["--prefix", p.prefix] : [])]); break;
				case "ids": out = run(["ids", db, ...(p.prefix ? ["--prefix", p.prefix] : []), ...(p.k ? ["--limit", String(p.k)] : [])]); break;
				case "get":
					out = run(["get", db, "--id", p.id ?? ""]);
					break;
				case "del":
					out = run(["del", db, ...(p.prefix ? ["--prefix", p.prefix] : ["--id", p.id ?? ""])]);
					break;
				default:
					out = run(["stats", db]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
