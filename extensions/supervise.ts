/**
 * supervise — pi extension over the pure-Rust process manager. Lets the agent
 * see and control its own long-running services (scheduler daemon, gateway,
 * headless Chromium) without shelling out to systemctl.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "supervise");

function run(args: string[]): string {
	try {
		return execFileSync(BIN, args, { encoding: "utf8", timeout: 30_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "supervise",
		label: "Supervisor",
		description:
			"Manage SMARTAGENT's long-running background services (scheduler daemon, gateway, headless " +
			"Chromium for the browser tool). Actions: 'status' shows each service's state/pid/health, " +
			"'up' starts them, 'down' stops them, 'restart' restarts one. Use 'status' to diagnose " +
			"why browser/search/schedule/gateway tools fail (a dead service).",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["status", "up", "down", "restart", "logs"] },
				service: { type: "string", description: "service name (scheduler|gateway|chromium); omit to target all" },
				tail: { type: "number", description: "logs: last N lines (default 40)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const args = [p.action, ...(p.service ? [p.service] : [])];
			if (p.action === "logs" && p.tail != null) args.push("--tail", String(p.tail));
			return { content: [{ type: "text", text: run(args) }] };
		},
	});
}
