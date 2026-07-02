/** codeindex — pi extension over the pure-Rust fast code-search binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "codeindex");

function run(args: string[], timeout = 60_000): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "codeindex",
		label: "Code Search",
		description: "Fast literal/regex code search (ripgrep concept), respecting .gitignore, plus per-project workspace indexing. Actions: 'search' finds a pattern across a dir with optional case-insensitivity, regex, context lines, and extension filter; 'files' lists indexable files; 'projects' lists the repos under workspaces/ with index status; 'index' builds the per-project file index (one project, or all workspace repos when no project is given). Use 'project' to scope search/files to a workspace repo.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["search", "files", "projects", "index"], description: "Operation to perform" },
				project: { type: "string", description: "workspace project name (scopes search/files; selects the repo for index)" },
					mode: { type: "string", enum: ["lines", "files", "count"], description: "search output: lines (default), files (names), count (totals only) — use count/files to gauge breadth cheaply" },
					max: { type: "number", description: "cap matched lines (default 50)" },
				pattern: { type: "string", description: "text or regex pattern (search action)" },
				dir: { type: "string", description: "directory to search (default .)" },
				ext: { type: "string", description: "restrict to files with this extension" },
				ignore_case: { type: "boolean", description: "case-insensitive match (search)" },
				regex: { type: "boolean", description: "treat pattern as regex (search)" },
				before: { type: "number", description: "lines of context before each match (search)" },
				after: { type: "number", description: "lines of context after each match (search)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const dir = p.dir ?? ".";
			let out: string;
			if (p.action === "projects") {
				out = run(["projects"]);
			} else if (p.action === "index") {
				out = run(p.project ? ["index", p.project] : ["index", "--all"], 600_000);
			} else if (p.action === "files") {
				const args = ["files", dir];
				if (p.project) args.push("--project", p.project);
				if (p.ext) args.push("-t", p.ext);
				if (p.max) args.push("--limit", String(p.max));
				out = run(args);
			} else {
				const args = ["search", p.pattern ?? "", dir];
				if (p.project) args.push("--project", p.project);
				if (p.ignore_case) args.push("-i");
				if (p.regex) args.push("-e");
				if (p.mode === "count") args.push("-c");
				else if (p.mode === "files") args.push("-l");
				else {
					if (p.before != null) args.push("-B", String(p.before));
					if (p.after != null) args.push("-A", String(p.after));
					if (p.max != null) args.push("-m", String(p.max));
				}
				if (p.ext) args.push("-t", p.ext);
				out = run(args);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
