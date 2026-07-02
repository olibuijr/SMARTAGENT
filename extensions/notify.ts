/**
 * notify — pi extension glue over the pure-Rust ntfy push notification client.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "notify");

function run(args: string[]): string {
	try {
		return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "notify",
		label: "Notifications",
		description:
			"Send a push notification to the principal (ntfy protocol). Use to alert about " +
			"finished background work, errors needing attention, or requested reminders. " +
			"Server comes from config/smartagent.conf unless overridden.",
		parameters: {
			type: "object",
			properties: {
				topic: { type: "string", description: "ntfy topic to publish to" },
				message: { type: "string", description: "notification body" },
				title: { type: "string", description: "optional title" },
				priority: { type: "string", description: "1 (min) to 5 (urgent)" },
				tags: { type: "string", description: "comma-separated tags/emoji shortcodes" },
			},
			required: ["topic", "message"],
		} as any,

		async execute(_id: string, p: any) {
			const args = ["send", "--topic", p.topic, "--message", p.message];
			if (p.title) args.push("--title", p.title);
			if (p.priority) args.push("--priority", p.priority);
			if (p.tags) args.push("--tags", p.tags);
			return { content: [{ type: "text", text: run(args) }] };
		},
	});
}
