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
			"Agent Skills (SKILL.md) registry AND self-authoring — an agent's procedural memory, " +
			"full Agent Skills standard (scripts/references/assets + role/task filing; `memory` is " +
			"small facts always in context, this is multi-step how-tos + runnable scripts loaded when " +
			"relevant). Read: 'list' shows all discovered skills with descriptions (filter with " +
			"'role'/'task'); 'match' scores skills against a whole prompt/task sentence (BEST way to " +
			"pick a skill for incoming work or a workflow step; also takes 'role'/'task'); 'search' " +
			"ranks by a single term; 'show' loads a named skill's full SKILL.md body, or — pass 'path' " +
			"— one specific supporting file's contents (Level-2 disclosure, e.g. path: " +
			"'scripts/foo.sh'); 'files' lists a skill's supporting files (relative path + size) under " +
			"scripts/references/assets. Write — the run-once → codify loop: the first time you work out " +
			"a non-trivial reusable procedure or command (5+ tool calls, a path found through " +
			"errors/dead-ends, or a corrected approach), author it as a skill instead of letting it " +
			"evaporate. 'create' (name, desc ≤100 chars, content with '## When to Use'/'## Procedure'/" +
			"'## Pitfalls'/'## Verification' sections framed around REAL SMARTAGENT tools, no invented " +
			"commands; tag 'role' — Coordinator/Builder/QA/Ops — and 'task' so it can be filed and " +
			"found again) — rejects a name collision, use edit/patch instead. Then 'write-file' (name, " +
			"path, content) to attach the ACTUAL reusable command(s) under 'scripts/<name>.sh' — this " +
			"IS the CLI tool per the standard, no separate tool/recipe concept, and it is made " +
			"executable — plus any 'references/<file>' material. Next time: 'match' with 'role' finds " +
			"the skill, 'files' shows what's bundled, 'show' with 'path' views a script before running " +
			"it. 'patch' is a token-efficient exact old_string→new_string replace on SKILL.md (errors " +
			"if not unique); 'edit' rewrites a skill's whole SKILL.md; 'remove-file' deletes one " +
			"supporting file; 'delete' removes the whole skill (SKILL.md + all supporting files). Pass " +
			"'project' to scope to that workspace repo's own accumulated skills " +
			"(workspaces/<project>/.smartagent/skills) — reads MERGE global+project skills with the " +
			"project one winning a name collision; writes target the project dir instead of the global " +
			"root when 'project' is given.",
		parameters: {
			type: "object",
			properties: {
				action: {
					type: "string",
					enum: [
						"list",
						"match",
						"search",
						"show",
						"validate",
						"create",
						"patch",
						"edit",
						"delete",
						"write-file",
						"remove-file",
						"files",
					],
					description: "Operation to perform",
				},
				name: { type: "string", description: "skill name (show/files/create/patch/edit/delete/write-file/remove-file)" },
				query: { type: "string", description: "search term (search) or full prompt/task text (match)" },
				head: { type: "number", description: "show: first N lines only (progressive disclosure)" },
				root: { type: "string", description: "skills root dir (default ./skills)" },
				category: { type: "string", description: "create: optional subdir to nest the skill under (<root>/<category>/<name>)" },
				desc: { type: "string", description: "create/edit: one-line description, ≤100 chars" },
				role: {
					type: "string",
					description:
						"create: who this skill is for (Coordinator/Builder/QA/Ops, aligned with the gateway fleet) — filed under metadata.smartagent.role. Also a filter on list/match.",
				},
				task: {
					type: "string",
					description:
						"create: the task type this skill covers — filed under metadata.smartagent.task. Also a filter on list/match.",
				},
				path: {
					type: "string",
					description:
						"supporting-file relative path — must start with scripts/, references/, or assets/ (e.g. 'scripts/deploy.sh'). Required for write-file/remove-file; optional on show for Level-2 disclosure of one file instead of the whole SKILL.md.",
				},
				content: {
					type: "string",
					description:
						"create/edit: full SKILL.md body — When to Use / Procedure / Pitfalls / Verification sections. write-file: the file's raw content (script text, reference doc, etc).",
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
			const roleTask = [...(p.role ? ["--role", p.role] : []), ...(p.task ? ["--task", p.task] : [])];
			let out: string;
			if (p.action === "show") {
				out = run([
					"show",
					root,
					p.name ?? "",
					...(p.path ? ["--path", p.path] : []),
					...(p.head != null ? ["--head", String(p.head)] : []),
					...proj,
				]);
			} else if (p.action === "search") {
				out = run(["search", root, p.query ?? "", ...proj]);
			} else if (p.action === "match") {
				out = run(["match", root, p.query ?? "", ...roleTask, ...proj]);
			} else if (p.action === "validate") {
				out = run(["validate", root]);
			} else if (p.action === "files") {
				out = run(["files", root, "--name", p.name ?? "", ...proj]);
			} else if (p.action === "write-file") {
				out = run(["write-file", root, "--name", p.name ?? "", "--path", p.path ?? "", ...proj], p.content ?? "");
			} else if (p.action === "remove-file") {
				out = run(["remove-file", root, "--name", p.name ?? "", "--path", p.path ?? "", ...proj]);
			} else if (p.action === "create") {
				out = run(
					[
						"create",
						root,
						"--name",
						p.name ?? "",
						...(p.category ? ["--category", p.category] : []),
						...(p.desc ? ["--desc", p.desc] : []),
						...roleTask,
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
				out = run(["list", root, ...roleTask, ...proj]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
