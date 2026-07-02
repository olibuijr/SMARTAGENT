/** sandbox — pi extension over the pure-Rust isolated-exec binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFile } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "sandbox");

function run(args: string[], signal?: AbortSignal): Promise<string> {
	return new Promise((resolve) => {
		const child = execFile(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT, signal }, (e, stdout, stderr) => {
			if (e) resolve(`error: ${String(stderr || "").trim() || e.message}`);
			else resolve(String(stdout).trim());
		});
		signal?.addEventListener("abort", () => child.kill(), { once: true });
	});
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
				stdin: { type: "string", description: "file piped to the command stdin" },
			},
			required: ["command"],
		} as any,
		async execute(_id: string, p: any, signal?: AbortSignal) {
			const flags: string[] = ["run", "--timeout", String(p.timeout ?? 30)];
			if (p.isolate === false) flags.push("--no-isolate");
			if (p.net) flags.push("--net");
			if (p.tail) flags.push("--tail");
			if (p.maxOutput) flags.push("--max-output", String(p.maxOutput));
			if (p.stdin) flags.push("--stdin", p.stdin);
			flags.push("--", "sh", "-c", p.command ?? "");
			return { content: [{ type: "text", text: await run(flags, signal) }] };
		},
	});
}
