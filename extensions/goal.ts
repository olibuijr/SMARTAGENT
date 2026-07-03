/**
 * goal — /goal slash command (Claude Code parity): work until an independently
 * verified condition holds. Thin glue over the `goal` binary; all logic is Rust.
 *
 *   /goal <condition>   set the completion condition (a fresh model checks each turn)
 *   /goal               show status: condition, turns evaluated, last reason
 *   /goal clear         drop the active goal (aliases: stop off reset none cancel)
 *
 * The loop itself is driven by the `goal-check` stop hook (config/hooks.conf):
 * after each turn a small model reads the transcript and decides met / not-met.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "goal");
const CLEAR = new Set(["clear", "stop", "off", "reset", "none", "cancel"]);

function run(args: string[]): string {
	try {
		return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerCommand("goal", {
		description: "Work until a verified condition holds (/goal <condition> | /goal | /goal clear)",
		handler: async (args: string) => {
			const a = (args ?? "").trim();
			let out: string;
			if (a === "") out = run(["status"]);
			else if (CLEAR.has(a.toLowerCase())) out = run([a.toLowerCase()]);
			else out = run(["set", a]);
			// Display-only: the goal + guidance land in the session so the model
			// works toward the condition on the next turn (pi extensions cannot
			// auto-start a turn — the stop hook verifies after each turn).
			pi.sendMessage({
				customType: "command",
				content: `**/goal**\n\`\`\`text\n${out || "(no output)"}\n\`\`\``,
				display: true,
			});
		},
	});
}
