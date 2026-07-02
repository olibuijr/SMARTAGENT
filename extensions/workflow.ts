/**
 * workflow — pi extension over the pure-Rust process engine. Workflows are
 * markdown under workflows/ and skills/<name>/Workflows/: steps route a skill
 * per phase (PAI pattern); advancing requires evidence — enforced by the binary.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "workflow");
const DB = join(ROOT, "data", "workflow.semdb");

function run(args: string[], project?: string, timeout = 30_000): string {
	// --project = run state lives with that workspace repo (pairs with the
	// repo's own tasks board); otherwise the root db. Defs always from ROOT.
	const scope = project ? ["--project", project] : ["--db", DB];
	try {
		return execFileSync(BIN, [...args, "--root", ROOT, ...scope], { encoding: "utf8", timeout, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "workflow",
		label: "Workflow engine",
		description:
			"Process engine for multi-step work. 'start' runs a definition (task_id links a board task); 'step' shows current instructions; 'advance' REQUIRES real evidence ('done'/'ok' rejected); 'runs' lists (live=true filters to actually-worked); 'abort'; 'drive' executes every step in fresh headless agents with validated evidence — for well-defined work; never inside a driven step. Built-ins: task-run, triage, retro. project scopes run state to a workspace repo.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["list", "show", "start", "step", "advance", "runs", "abort", "drive"] },
				name: { type: "string", description: "workflow name for show/start (e.g. task-run)" },
				run: { type: "string", description: "run id (W-1); defaults to latest running" },
				task_id: { type: "string", description: "start: link a kanban task (T-n)" },
				evidence: { type: "string", description: "advance: WHAT you verified and HOW (required, ≥10 chars)" },
				project: { type: "string", description: "workspace repo name — keep run state on that repo (match the tasks board)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const a: string[] = [p.action];
			if ((p.action === "show" || p.action === "start" || p.action === "drive") && p.name) a.push(p.name);
			if (p.task_id) a.push("--task", p.task_id);
			if (p.run) a.push("--run", p.run);
			if (p.evidence) a.push("--evidence", p.evidence);
			// drive spawns one headless pi per step — needs a long leash.
			const timeout = p.action === "drive" ? 1_800_000 : 30_000;
			return { content: [{ type: "text", text: run(a, p.project, timeout) }] };
		},
	});
}
