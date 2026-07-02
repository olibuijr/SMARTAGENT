/**
 * statusline — pi extension that surfaces SMARTAGENT tool + service statuses
 * in the TUI, using pi's footer status entries and a belowEditor widget.
 *
 * Two surfaces:
 *  - Per-tool activity: tool_execution_start/end events → ctx.ui.setStatus(tool, …)
 *    (running spinner text, then ✓/✗ with duration; auto-clears after a few seconds).
 *  - Services widget below the input: `supervise statusline` (pure-Rust verb)
 *    rendered as one line, refreshed on session start, after supervise/browser/
 *    schedule/search tool calls, and every 30s.
 *
 * No logic in TS: state text comes from the Rust binary; this file only places it.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFile } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "supervise");

// Tools whose activity is shown in the footer (keep in sync with extensions/).
const TOOLS = new Set([
	"semdb", "memory", "codegraph", "codeindex", "vault", "skills", "schedule",
	"search", "notify", "secrets", "browser", "orchestrate", "mcp", "sandbox",
	"context", "evals", "rag", "supervise",
]);
// Tool calls that can change service state → refresh the services widget after.
const SERVICE_TOUCHING = new Set(["supervise", "browser", "schedule", "search"]);

const CLEAR_AFTER_MS = 5000;
const REFRESH_MS = 30_000;

export default function (pi: ExtensionAPI) {
	const started = new Map<string, number>(); // toolCallId → start ms
	const clearTimers = new Map<string, ReturnType<typeof setTimeout>>();
	let ui: any; // latest ctx.ui, captured from events (only used when hasUI)
	let timer: ReturnType<typeof setInterval> | undefined;

	function refreshServices() {
		if (!ui) return;
		execFile(BIN, ["statusline"], { encoding: "utf8", timeout: 10_000, cwd: ROOT }, (err, stdout) => {
			if (!ui) return;
			const line = err ? "services: unavailable" : stdout.trim();
			ui.setWidget("smartagent-services", [`⛭ ${line}`], { placement: "belowEditor" });
		});
	}

	pi.on("session_start", async (_event, ctx) => {
		if (!ctx.hasUI) return;
		ui = ctx.ui;
		refreshServices();
		if (!timer) timer = setInterval(refreshServices, REFRESH_MS);
	});

	pi.on("session_shutdown", async () => {
		if (timer) clearInterval(timer);
		timer = undefined;
		ui = undefined;
	});

	pi.on("tool_execution_start", async (event: any, ctx) => {
		if (!ctx.hasUI || !TOOLS.has(event.toolName)) return;
		ui = ctx.ui;
		started.set(event.toolCallId, Date.now());
		const t = clearTimers.get(event.toolName);
		if (t) clearTimeout(t);
		ctx.ui.setStatus(event.toolName, `${event.toolName} ⚙ running…`);
	});

	pi.on("tool_execution_end", async (event: any, ctx) => {
		if (!ctx.hasUI || !TOOLS.has(event.toolName)) return;
		ui = ctx.ui;
		const t0 = started.get(event.toolCallId);
		started.delete(event.toolCallId);
		const ms = t0 ? Date.now() - t0 : 0;
		const mark = event.isError ? "✗" : "✓";
		ctx.ui.setStatus(event.toolName, `${event.toolName} ${mark} ${ms}ms`);
		clearTimers.set(
			event.toolName,
			setTimeout(() => ctx.ui.setStatus(event.toolName, undefined), CLEAR_AFTER_MS),
		);
		if (SERVICE_TOUCHING.has(event.toolName)) refreshServices();
	});
}
