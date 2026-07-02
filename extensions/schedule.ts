/**
 * schedule — pi extension glue over the pure-Rust durable cron scheduler.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "schedule");

function run(args: string[]): string {
	try {
		return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "schedule",
		label: "Scheduler",
		description:
			"Durable cron scheduler (journaled, replay-safe). Actions: 'add' registers a job " +
			"(cron 5-field expression + shell cmd), 'list' shows jobs with last run, 'next' shows " +
			"next fire times, 'rm' removes by id, 'tick' fires anything due right now.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["add", "list", "next", "rm", "tick"] },
				cron: { type: "string", description: "5-field cron expression (add)" },
				cmd: { type: "string", description: "Shell command to run (add)" },
				id: { type: "string", description: "Job id (add optional, rm required)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const out =
				p.action === "add"
					? run(["add", "--cron", p.cron ?? "", "--cmd", p.cmd ?? "", ...(p.id ? ["--id", p.id] : [])])
					: p.action === "rm"
						? run(["rm", "--id", p.id ?? ""])
						: p.action === "tick"
							? run(["run", "--once"])
							: run([p.action]);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
