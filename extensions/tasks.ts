/**
 * tasks — pi extension over the pure-Rust kanban board. Kanban policies (WIP
 * limits, pull-based next, criteria-gated done) are enforced by the binary;
 * methodology guidance lives in skills/Kanban.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "tasks");
const DB = join(ROOT, "data", "tasks.semdb");

function run(args: string[]): string {
	try {
		return execFileSync(BIN, [...args, "--db", DB], { encoding: "utf8", timeout: 30_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "tasks",
		label: "Kanban tasks",
		description:
			"Kanban task board (backlog→ready→doing→review→done). Policies are ENFORCED: WIP limits " +
			"block over-committing, 'done' requires all criteria checked, 'next' only pulls when there " +
			"is capacity. Actions: board (render columns), add/todo (capture; criteria as 'a;b;c'), " +
			"next (what to pull), move, done, show, list, crit (add/check/uncheck acceptance criteria), " +
			"block/unblock (reason required), wip (limits), metrics (cycle time/throughput), rm. " +
			"Load the kanban skill (skills show kanban) for methodology; run 'workflow start task-run " +
			"--task T-n' to work a task through the phase loop.",
		parameters: {
			type: "object",
			properties: {
				action: {
					type: "string",
					enum: ["board", "add", "todo", "next", "move", "done", "show", "list", "crit", "block", "unblock", "wip", "metrics", "rm"],
				},
				id: { type: "string", description: "task id (T-1) for move/done/show/crit/block/unblock/rm" },
				title: { type: "string", description: "title for add/todo; reason for block" },
				column: { type: "string", description: "target column for move (backlog|ready|doing|review|done)" },
				prio: { type: "string", enum: ["p1", "p2", "p3"], description: "priority for add" },
				criteria: { type: "string", description: "add: semicolon-separated acceptance criteria" },
				tags: { type: "string", description: "add: comma-separated tags; list: single tag filter" },
				crit_op: { type: "string", enum: ["add", "check", "uncheck"], description: "crit sub-action" },
				crit_arg: { type: "string", description: "crit: criterion text (add) or number (check/uncheck)" },
				col: { type: "string", description: "list: column filter" },
				doing: { type: "number", description: "wip: doing limit" },
				review: { type: "number", description: "wip: review limit" },
				force: { type: "boolean", description: "override WIP/criteria gates (avoid — see kanban skill)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const a: string[] = [p.action];
			switch (p.action) {
				case "add":
				case "todo":
					if (p.title) a.push(p.title);
					if (p.prio) a.push("--prio", p.prio);
					if (p.column) a.push("--col", p.column);
					if (p.criteria) a.push("--criteria", p.criteria);
					if (p.tags) a.push("--tags", p.tags);
					break;
				case "move":
					if (p.id) a.push(p.id);
					if (p.column) a.push(p.column);
					break;
				case "done":
				case "show":
				case "rm":
				case "unblock":
					if (p.id) a.push(p.id);
					break;
				case "block":
					if (p.id) a.push(p.id);
					if (p.title) a.push(p.title);
					break;
				case "crit":
					if (p.crit_op) a.push(p.crit_op);
					if (p.id) a.push(p.id);
					if (p.crit_arg) a.push(p.crit_arg);
					break;
				case "list":
					if (p.col) a.push("--col", p.col);
					if (p.tags) a.push("--tag", p.tags);
					break;
				case "wip":
					if (p.doing != null) a.push("--doing", String(p.doing));
					if (p.review != null) a.push("--review", String(p.review));
					break;
			}
			if (p.force) a.push("--force");
			return { content: [{ type: "text", text: run(a) }] };
		},
	});
}
