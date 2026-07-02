/**
 * hooks — bridges pi lifecycle events to the pure-Rust hooks dispatcher
 * (target/release/hooks). All policy lives in Rust + the user's hook commands
 * (config/hooks.conf, scripts in hooks.d/); this file only translates:
 *   tool_call          → dispatch tool_call   (block + updatedInput rewrite)
 *   input              → dispatch user_prompt (block with reason)
 *   before_agent_start → dispatch session_start (plain-text context injection)
 *   agent_end          → dispatch stop        (fire-and-forget, audit only)
 * Contract per hook command: payload JSON on stdin; exit 0 allow, exit 2 block.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "hooks");
const CONF = join(ROOT, "config", "hooks.conf");

type Decision = {
	block?: boolean;
	reason?: string;
	updatedInput?: unknown;
	context?: string[];
	warnings?: string[];
};

function dispatch(event: string, subject: string, payload: unknown): Decision {
	try {
		const out = execFileSync(BIN, ["dispatch", "--event", event, "--subject", subject, "--root", ROOT], {
			encoding: "utf8",
			input: JSON.stringify(payload ?? {}),
			timeout: 120_000,
			cwd: ROOT,
		});
		return JSON.parse(out.trim()) as Decision;
	} catch {
		// Dispatcher failure fails OPEN — hooks must never wedge the agent.
		return {};
	}
}

export default function (pi: ExtensionAPI) {
	// No config → zero overhead: don't subscribe at all.
	if (!existsSync(CONF)) return;

	pi.on("tool_call", async (event: any) => {
		const d = dispatch("tool_call", event.toolName ?? "", { toolName: event.toolName, input: event.input });
		if (d.block) return { block: true, reason: d.reason || "blocked by hook" };
		if (d.updatedInput && typeof d.updatedInput === "object") {
			Object.assign(event.input, d.updatedInput);
		}
		return undefined;
	});

	pi.on("input", async (event: any) => {
		const d = dispatch("user_prompt", "", { text: event.text });
		if (d.block) return { action: "handled", text: `Hook blocked this prompt: ${d.reason}` };
		return { action: "continue" };
	});

	pi.on("before_agent_start", async (event: any) => {
		const d = dispatch("session_start", "startup", { prompt: event?.message ?? "" });
		if (d.context && d.context.length > 0) {
			return { message: `Hook context:\n${d.context.join("\n")}` };
		}
		return undefined;
	});

	pi.on("agent_end", async () => {
		dispatch("stop", "", {});
	});
}
