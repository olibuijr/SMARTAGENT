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
 * Type-only pi imports; shared local statusline metadata lives in extensions/lib/.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { BIN, ROOT, STATUS_SEGMENTS, TASKS_DB, WORKFLOW_DB, parseLevel, statusTag } from "./lib/statusline-common.ts";

const MEMORY_DIR = join(ROOT, "data", "memory");
const SKILLS_ROOT = join(ROOT, "skills");
const REGISTRY = join(ROOT, "config", "slash_commands.tsv");

type SlashCommand = { name: string; description: string; telegram: boolean; tui: boolean; klass: string };
function slashCommands(): SlashCommand[] {
	return readFileSync(REGISTRY, "utf8")
		.split(/\r?\n/)
		.map((line) => line.trim())
		.filter((line) => line && !line.startsWith("#"))
		.map((line) => {
			const [name, description, telegram, tui, klass] = line.split("\t");
			return { name, description, telegram: telegram === "true", tui: tui === "true", klass };
		})
		.filter((c) => c.tui);
}

function commandHelp(): string {
	return slashCommands().map((c) => `/${c.name} — ${c.description}`).join("\n");
}

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
		description: "One-shot status using the same probes as the live statusline",
		handler: async () => {
			const lines = STATUS_SEGMENTS.map(({ key, args }) => {
				const { level, text } = parseLevel(run(key, args, 10_000));
				return `${statusTag(level)} ${key.padEnd(10)} ${text}`;
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

	for (const alias of ["help", "commands"]) {
		pi.registerCommand(alias, {
			description: "Show SMARTAGENT commands",
			handler: async () => show("commands", commandHelp()),
		});
	}

	pi.registerCommand("agents", {
		description: "Show gateway fleet state",
		handler: async () => show("gateway agents", run("gateway", ["agents"])),
	});

	pi.registerCommand("model", {
		description: "Choose this chat's reply model (Telegram-only persistence)",
		handler: async () => show("model", "Model selection is chat-scoped in Telegram. In the TUI, select models with pi's model controls or launch flags."),
	});

	for (const alias of ["verbosity", "verbose"]) {
		pi.registerCommand(alias, {
			description: "Show or set notification verbosity",
			handler: async () => show(alias, "Telegram notification verbosity is chat-scoped; use this command in Telegram to change that chat."),
		});
	}

	pi.registerCommand("reset", {
		description: "Clear this chat/thread rolling context",
		handler: async () => show("reset", "Telegram-only chat context command; TUI context resets by starting a fresh session."),
	});
	pi.registerCommand("remember", {
		description: "Remember a fact (usage: /remember fact)",
		handler: async (args) => show("remember", args.trim() ? run("memory", ["remember", "--tier", "semantic", "--text", args.trim()]) : "usage: /remember fact"),
	});
	pi.registerCommand("stop", {
		description: "Stop the active reply",
		handler: async () => show("stop", "Use Ctrl+C/Esc in the TUI; /stop cancels Telegram streaming replies."),
	});
	pi.registerCommand("resolve", {
		description: "Add custom resolution text for a blocked task",
		handler: async () => show("resolve", "Telegram blocker-resolution helper; use tasks block/unblock in the TUI."),
	});
	pi.registerCommand("assign", {
		description: "Show Telegram multiple-choice agent assignment",
		handler: async () => show("assign", "Telegram displays an inline multiple-choice agent picker: Coordinator, Builder, QA, Ops."),
	});
}
