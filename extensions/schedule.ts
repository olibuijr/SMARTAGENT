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
			"Durable cron scheduler (semdb-journaled, replay-safe). Actions: 'add' registers an agent-safe " +
			"notification reminder (cron recurring or at one-shot), 'list' shows jobs with last run, 'next' shows " +
			"next fire times, 'rm' removes by id, pause/resume toggle jobs, and 'tick' fires anything due right now.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["add", "list", "next", "rm", "pause", "resume", "tick"] },
				cron: { type: "string", description: "5-field cron expression for a recurring job (add)" },
					at: { type: "string", description: "YYYY-MM-DDTHH:MM for a ONE-SHOT reminder that self-removes after firing (add)" },
				notify: { type: "string", description: "Reminder message to push when the job fires (add). This is the only job type the agent can create." },
				id: { type: "string", description: "Job id (add optional; rm/pause/resume required)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			let out: string;
			switch (p.action) {
				case "add": {
					const when = p.at ? ["--at", p.at] : ["--cron", p.cron ?? ""];
					out = run(["add", ...when, "--notify", p.notify ?? "", ...(p.id ? ["--id", p.id] : [])]);
					break;
				}
				case "rm": out = run(["rm", "--id", p.id ?? ""]); break;
				case "pause": out = run(["pause", "--id", p.id ?? ""]); break;
				case "resume": out = run(["resume", "--id", p.id ?? ""]); break;
				case "tick": out = run(["run", "--once"]); break;
				default: out = run([p.action]);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
