/** skills — pi extension over the pure-Rust SKILL.md loader + self-authoring binary. */
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
			"Agent Skills (SKILL.md) registry AND self-authoring — an agent's procedural memory " +
			"(skills = multi-step reusable procedures loaded when relevant; `memory` is small facts " +
			"always in context). Read: 'list' shows all discovered skills with descriptions; 'match' " +
			"scores skills against a whole prompt/task sentence (BEST way to pick a skill for " +
			"incoming work or a workflow step); 'search' ranks by a single term; 'show' loads a " +
			"named skill's full body. Write — author your own skill after a non-trivial workflow " +
			"(5+ tool calls, a path found through errors/dead-ends, or a corrected approach): " +
			"'create' (name, desc ≤100 chars, content with '## When to Use'/'## Procedure'/" +
			"'## Pitfalls'/'## Verification' sections framed around REAL SMARTAGENT tools, no " +
			"invented commands) — rejects a name collision, use edit/patch instead; 'patch' is a " +
			"token-efficient exact old_string→new_string replace (errors if not unique); 'edit' " +
			"rewrites a skill's whole content; 'delete' removes one. Pass 'project' to scope to that " +
			"workspace repo's own accumulated skills (workspaces/<project>/.smartagent/skills) — " +
			"reads MERGE global+project skills with the project one winning a name collision; writes " +
			"target the project dir instead of the global root when 'project' is given.",
		parameters: {
			type: "object",
			properties: {
				action: {
					type: "string",
					enum: ["list", "match", "search", "show", "validate", "create", "patch", "edit", "delete"],
					description: "Operation to perform",
				},
				name: { type: "string", description: "skill name (show/create/patch/edit/delete)" },
				query: { type: "string", description: "search term (search) or full prompt/task text (match)" },
				head: { type: "number", description: "show: first N lines only (progressive disclosure)" },
				root: { type: "string", description: "skills root dir (default ./skills)" },
				category: { type: "string", description: "create: optional subdir to nest the skill under (<root>/<category>/<name>)" },
				desc: { type: "string", description: "create/edit: one-line description, ≤100 chars" },
				content: {
					type: "string",
					description: "create/edit: full SKILL.md body — When to Use / Procedure / Pitfalls / Verification sections",
				},
				old_string: { type: "string", description: "patch: exact text to replace (must be unique in the file)" },
				new_string: { type: "string", description: "patch: replacement text" },
				project: { type: "string", description: "workspace project name — scope to that repo's own self-created skills" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const root = p.root ?? DEFAULT_ROOT;
			const proj = p.project ? ["--project", p.project] : [];
			let out: string;
			if (p.action === "show") {
				out = run(["show", root, p.name ?? "", ...(p.head != null ? ["--head", String(p.head)] : []), ...proj]);
			} else if (p.action === "search") {
				out = run(["search", root, p.query ?? "", ...proj]);
			} else if (p.action === "match") {
				out = run(["match", root, p.query ?? "", ...proj]);
			} else if (p.action === "validate") {
				out = run(["validate", root]);
			} else if (p.action === "create") {
				out = run(
					[
						"create",
						root,
						"--name",
						p.name ?? "",
						...(p.category ? ["--category", p.category] : []),
						...(p.desc ? ["--desc", p.desc] : []),
						...proj,
					],
					p.content ?? "",
				);
			} else if (p.action === "patch") {
				out = run(["patch", root, "--name", p.name ?? "", "--old", p.old_string ?? "", "--new", p.new_string ?? "", ...proj]);
			} else if (p.action === "edit") {
				out = run(["edit", root, "--name", p.name ?? "", ...(p.desc ? ["--desc", p.desc] : []), ...proj], p.content ?? "");
			} else if (p.action === "delete") {
				out = run(["delete", root, "--name", p.name ?? "", ...proj]);
			} else {
				out = run(["list", root, ...proj]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
