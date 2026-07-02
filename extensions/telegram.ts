/**
 * telegram — pi extension glue over the pure-Rust Telegram Bot API bridge.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "telegram");

function run(args: string[]): string {
	try {
		return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "telegram",
		label: "Telegram",
		description:
			"Telegram Bot API bridge: send messages, poll inbound updates, or run listen. " +
			"Token comes only from secrets get telegram_bot_token; chat allow-list from config.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["send", "poll", "listen"], description: "Operation to perform" },
				chat: { type: "string", description: "Telegram chat id for send" },
				text: { type: "string", description: "Message text for send" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const args = [p.action];
			if (p.chat) args.push("--chat", p.chat);
			if (p.text) args.push("--text", p.text);
			return { content: [{ type: "text", text: run(args) }] };
		},
	});
}
