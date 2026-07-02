/**
 * commands — instant slash commands over the SMARTAGENT tool binaries.
 *
 * Registers user-typed TUI commands (pi.registerCommand) that shell out to
 * target/release binaries and print the output straight into the chat as a
 * custom message — NO model round-trip. Output is appended to the session,
 * so the model sees it as context on the next turn (Claude Code behavior).
 *
 *   /board [project]   tasks board (root board, or a workspace repo's own)
 *   /tasks             tasks list (root board)
 *   /skills [query]    skills list, or skills match when a query is given
 *   /status            one-shot render of the statusline probes
 *   /index [project]   codeindex index (one project, or --all)
 *   /projects          codeindex projects
 *   /runs              workflow runs
 *   /audit             hooks audit --n 10
 *   /memory <query>    memory recall
 *
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = (name: string) => join(ROOT, "target", "release", name);
const TASKS_DB = join(ROOT, "data", "tasks.semdb");
const WORKFLOW_DB = join(ROOT, "data", "workflow.semdb");
const MEMORY_DIR = join(ROOT, "data", "memory");
const SKILLS_ROOT = join(ROOT, "skills");

function run(bin: string, args: string[], timeout = 60_000): string {
	try {
		return execFileSync(BIN(bin), args, { encoding: "utf8", timeout, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	// Print command output into the TUI (and session) without a model turn.
	const show = (title: string, out: string) => {
		pi.sendMessage({
			customType: "command",
			content: `**${title}**\n\`\`\`text\n${out || "(no output)"}\n\`\`\``,
			display: true,
		});
	};

	pi.registerCommand("board", {
		description: "Kanban board (usage: /board [project])",
		handler: async (args) => {
			const project = args.trim();
			const scope = project ? ["--project", project] : ["--db", TASKS_DB];
			show(project ? `tasks board — ${project}` : "tasks board", run("tasks", ["board", ...scope]));
		},
	});

	pi.registerCommand("tasks", {
		description: "List tasks on the root board",
		handler: async () => {
			show("tasks list", run("tasks", ["list", "--db", TASKS_DB]));
		},
	});

	pi.registerCommand("skills", {
		description: "List skills, or match against a prompt (usage: /skills [query])",
		handler: async (args) => {
			const query = args.trim();
			show(
				query ? `skills match — ${query}` : "skills list",
				query ? run("skills", ["match", SKILLS_ROOT, query]) : run("skills", ["list", SKILLS_ROOT]),
			);
		},
	});

	pi.registerCommand("status", {
		description: "One-shot status of services, index, board, and workflow",
		handler: async () => {
			// Same probes the statusline widget uses; level| prefix → tag.
			const probes: { label: string; bin: string; args: string[] }[] = [
				{ label: "services", bin: "supervise", args: ["statusline"] },
				{ label: "codeindex", bin: "codeindex", args: ["statusline"] },
				{ label: "tasks", bin: "tasks", args: ["statusline", "--db", TASKS_DB] },
				{ label: "workflow", bin: "workflow", args: ["statusline", "--root", ROOT, "--db", WORKFLOW_DB] },
			];
			const lines = probes.map(({ label, bin, args }) => {
				const raw = run(bin, args, 10_000);
				const cut = raw.indexOf("|");
				const level = cut > 0 ? raw.slice(0, cut) : "err";
				const text = cut > 0 ? raw.slice(cut + 1) : raw;
				const tag = level === "ok" ? "✓" : level === "warn" ? "⚠" : "✗";
				return `${tag} ${label.padEnd(10)} ${text}`;
			});
			show("status", lines.join("\n"));
		},
	});

	pi.registerCommand("index", {
		description: "Build workspace file index (usage: /index [project], default all)",
		handler: async (args) => {
			const project = args.trim();
			show(
				project ? `codeindex index — ${project}` : "codeindex index — all",
				run("codeindex", ["index", project || "--all"], 600_000),
			);
		},
	});

	pi.registerCommand("projects", {
		description: "List workspace projects with index status",
		handler: async () => {
			show("codeindex projects", run("codeindex", ["projects"]));
		},
	});

	pi.registerCommand("runs", {
		description: "List workflow runs",
		handler: async () => {
			show("workflow runs", run("workflow", ["runs", "--root", ROOT, "--db", WORKFLOW_DB]));
		},
	});

	pi.registerCommand("audit", {
		description: "Recent hook firings (hooks audit --n 10)",
		handler: async () => {
			show("hooks audit", run("hooks", ["audit", "--n", "10", "--root", ROOT]));
		},
	});

	pi.registerCommand("memory", {
		description: "Semantic memory recall (usage: /memory <query>)",
		handler: async (args) => {
			const query = args.trim();
			if (!query) {
				show("memory recall", "usage: /memory <query>");
				return;
			}
			show(`memory recall — ${query}`, run("memory", ["recall", "--dir", MEMORY_DIR, "--text", query, "--k", "5"]));
		},
	});
}
