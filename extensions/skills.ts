/** skills — pi extension over the pure-Rust SKILL.md loader binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";
import { bin, makeRunner, ROOT } from "./lib/common.ts";

const DEFAULT_ROOT = join(ROOT, "skills");
const run = makeRunner(bin("skills"), 30_000);

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "skills",
		label: "Skills",
		description:
			"Agent Skills (SKILL.md) registry. Actions: 'list' shows all discovered skills with " +
			"descriptions; 'match' scores skills against a whole prompt/task sentence (BEST way to " +
			"pick a skill for incoming work or a workflow step); 'search' ranks by a single term; " +
			"'show' loads a named skill's full body. Load a matching skill before specialized work.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["list", "match", "search", "show", "validate"], description: "Operation to perform" },
				name: { type: "string", description: "skill name (show action)" },
				query: { type: "string", description: "search term (search) or full prompt/task text (match)" },
				head: { type: "number", description: "show: first N lines only (progressive disclosure)" },
				root: { type: "string", description: "skills root dir (default ./skills)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const root = p.root ?? DEFAULT_ROOT;
			let out: string;
			if (p.action === "show") out = run(["show", root, p.name ?? "", ...(p.head != null ? ["--head", String(p.head)] : [])]);
			else if (p.action === "search") out = run(["search", root, p.query ?? ""]);
			else if (p.action === "match") out = run(["match", root, p.query ?? ""]);
			else if (p.action === "validate") out = run(["validate", root]);
			else out = run(["list", root]);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
