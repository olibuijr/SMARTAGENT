/** context — pi extension over the pure-Rust principal-context loader binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";
import { bin, makeRunner, ROOT } from "./lib/common.ts";

const DEFAULT_DIR = join(ROOT, "context");

const run = makeRunner(bin("context"), 30_000);

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "context",
		label: "Context",
		description: "Principal identity/context loader (TELOS pattern). Actions: 'compose' merges the context/ markdown files into one system-prompt block trimmed to a char budget (lowest-priority dropped first, per the ORDER file); 'validate' checks the dir; 'stat' summarizes files and sizes. Use to load who the principal is and their goals into context.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["compose", "validate", "stat"], description: "Operation to perform" },
				budget: { type: "number", description: "char budget for compose (default 4000)" },
				dir: { type: "string", description: "context dir (default ./context)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const dir = p.dir ?? DEFAULT_DIR;
			let out: string;
			if (p.action === "compose") out = run(["compose", "--dir", dir, "--budget", String(p.budget ?? 4000)]);
			else if (p.action === "validate") out = run(["validate", "--dir", dir]);
			else out = run(["stat", "--dir", dir]);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
