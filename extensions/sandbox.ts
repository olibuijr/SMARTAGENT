/** sandbox — pi extension over the pure-Rust isolated-exec binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "sandbox");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "sandbox",
		label: "Sandbox",
		description: "Run a shell command in an isolated scratch workspace (Daytona concept). Writes are confined to the workspace; supports a wall-clock timeout and namespace isolation (isolate=true). Use to run untrusted or destructive commands safely.",
		parameters: {
			type: "object",
			properties: {
				command: { type: "string", description: "shell command to run" },
				timeout: { type: "number", description: "wall-clock timeout seconds (default 30)" },
				isolate: { type: "boolean", description: "use namespace isolation if available" },
				net: { type: "boolean", description: "allow network (default false when isolated)" },
			},
			required: ["command"],
		} as any,
		async execute(_id: string, p: any) {
			const flags: string[] = ["run", "--timeout", String(p.timeout ?? 30)];
			if (p.isolate) flags.push("--isolate");
			if (p.net) flags.push("--net");
			flags.push("--", "sh", "-c", p.command ?? "");
			return { content: [{ type: "text", text: run(flags) }] };
		},
	});
}
