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

function run(args: string[]): string {
	try {
		return execFileSync(BIN, [...args, "--root", ROOT, "--db", DB], { encoding: "utf8", timeout: 30_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "workflow",
		label: "Workflow engine",
		description:
			"Markdown-defined multi-step process engine (PAI pattern): each step names the skill/tool " +
			"to use; advancing REQUIRES evidence of what you verified ('done'/'ok' rejected). " +
			"Built-in workflows: task-run (observe→plan→execute→verify→learn for one kanban task), " +
			"triage (backlog refinement), retro (flow retrospective). Actions: list (discover), " +
			"show (outline), start (begin; link a board task with task_id), step (current instructions), " +
			"advance (complete step with evidence), runs, abort. Follow the printed step instructions, " +
			"load the step's skill, verify, then advance.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["list", "show", "start", "step", "advance", "runs", "abort"] },
				name: { type: "string", description: "workflow name for show/start (e.g. task-run)" },
				run: { type: "string", description: "run id (W-1); defaults to latest running" },
				task_id: { type: "string", description: "start: link a kanban task (T-n)" },
				evidence: { type: "string", description: "advance: WHAT you verified and HOW (required, ≥10 chars)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const a: string[] = [p.action];
			if ((p.action === "show" || p.action === "start") && p.name) a.push(p.name);
			if (p.task_id) a.push("--task", p.task_id);
			if (p.run) a.push("--run", p.run);
			if (p.evidence) a.push("--evidence", p.evidence);
			return { content: [{ type: "text", text: run(a) }] };
		},
	});
}
