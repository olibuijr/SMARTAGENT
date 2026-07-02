/** context — pi extension over the pure-Rust principal-context loader binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "context");
const DEFAULT_DIR = join(ROOT, "context");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 30_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

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
