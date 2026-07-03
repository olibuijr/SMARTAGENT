/**
 * notify — pi extension glue over the pure-Rust ntfy push notification client.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";
import { bin, makeRunner, ROOT } from "./lib/common.ts";


const run = makeRunner(bin("notify"), 120000);

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
				click: { type: "string", description: "http(s) URL opened on tap" },
				markdown: { type: "boolean", description: "render body as markdown" },
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
			if (p.click) args.push("--click", p.click);
			if (p.markdown) args.push("--markdown");
			return { content: [{ type: "text", text: run(args) }] };
		},
	});
}
