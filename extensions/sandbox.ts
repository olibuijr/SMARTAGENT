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
		description: "Run a shell command in an isolated scratch workspace (Daytona concept). Writes are confined to the workspace, the parent environment (secrets/keys) is scrubbed, and namespace isolation is ON by default. Set isolate=false only if a command needs the real namespace (drops to filesystem-only). Use for untrusted or destructive commands.",
		parameters: {
			type: "object",
			properties: {
				command: { type: "string", description: "shell command to run" },
				timeout: { type: "number", description: "wall-clock timeout seconds (default 30)" },
				isolate: { type: "boolean", description: "namespace isolation (default true); set false to opt out" },
				net: { type: "boolean", description: "allow network (default false when isolated)" },
				tail: { type: "boolean", description: "keep the LAST output bytes not the first (right for build logs)" },
				maxOutput: { type: "number", description: "output byte cap (default 16384)" },
			},
			required: ["command"],
		} as any,
		async execute(_id: string, p: any) {
			const flags: string[] = ["run", "--timeout", String(p.timeout ?? 30)];
			if (p.isolate === false) flags.push("--no-isolate");
			if (p.net) flags.push("--net");
			if (p.tail) flags.push("--tail");
			if (p.maxOutput) flags.push("--max-output", String(p.maxOutput));
			flags.push("--", "sh", "-c", p.command ?? "");
			return { content: [{ type: "text", text: run(flags) }] };
		},
	});
}
