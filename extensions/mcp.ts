/** mcp — pi extension over the pure-Rust MCP client binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "mcp");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 60_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "mcp",
		label: "MCP Client",
		description: "Connect to any MCP server (stdio via a command, or HTTP via a URL). 'tools' lists the server's tools; 'call' invokes one with JSON arguments. Use to reach external MCP tool servers from pi.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["tools", "call"] },
				cmd: { type: "string", description: "stdio server command" },
				url: { type: "string", description: "streamable-HTTP server URL" },
				tool: { type: "string", description: "tool name (call action)" },
				args: { type: "string", description: "JSON arguments object (call action)" },
				namesOnly: { type: "boolean", description: "tools: names only, skip schemas (cheap discovery)" },
				filter: { type: "string", description: "tools: only tools matching this substring" },
				head: { type: "number", description: "call: cap response length" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const conn = p.cmd ? ["--cmd", p.cmd] : p.url ? ["--url", p.url] : [];
			let out: string;
			if (p.action === "tools") {
				const a = ["tools", ...conn];
				if (p.namesOnly) a.push("--names-only");
				if (p.filter) a.push("--filter", p.filter);
				out = run(a);
			} else {
				const a = ["call", ...conn, "--tool", p.tool ?? "", "--args", p.args ?? "{}"];
				if (p.head != null) a.push("--head", String(p.head));
				out = run(a);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
