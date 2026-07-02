/**
 * statusline — pi extension that surfaces SMARTAGENT tool + service statuses
 * in the TUI, using pi's footer status entries and a belowEditor widget.
 *
 * Three surfaces:
 *  - Per-tool activity: tool_execution_start/end events → ctx.ui.setStatus(tool, …)
 *    (⚙ running, then ✓/✗ with duration; auto-clears after a few seconds).
 *  - Infra line (belowEditor): ⛭ services · 🧱 sandbox · 🔑 secrets auth ·
 *    🌐 chrome · 🔎 searx · 🕸 codegraph — slow-changing host/infra state.
 *  - Data line (belowEditor): 🧠 memory tiers · 📚 rag corpus · ⏰ next job ·
 *    📊 evals · 🤖 orchestrate — volatile stats, re-probed after related tools run.
 *
 * Every segment comes from a Rust `statusline` verb emitting `level|icon text`;
 * severity classification lives in Rust, this file only maps level → ANSI color
 * (ok=green tick text stays default, warn=yellow, err=red) and places lines.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFile } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = (name: string) => join(ROOT, "target", "release", name);

// ── Segment registry: tool → binary args + which widget line it lives on ──
type Line = "infra" | "data";
const SEGMENTS: { key: string; args: string[]; line: Line }[] = [
	{ key: "supervise", args: ["statusline"], line: "infra" },
	{ key: "sandbox", args: ["statusline"], line: "infra" },
	{ key: "secrets", args: ["statusline", "--store", join(ROOT, "data", "secrets")], line: "infra" },
	{ key: "browser", args: ["statusline"], line: "infra" },
	{ key: "search", args: ["statusline"], line: "infra" },
	{ key: "codegraph", args: ["statusline", join(ROOT, "data", "codegraph.json")], line: "infra" },
	{ key: "memory", args: ["statusline", "--dir", join(ROOT, "data", "memory")], line: "data" },
	{ key: "rag", args: ["statusline", join(ROOT, "data", "rag.semdb")], line: "data" },
	{ key: "schedule", args: ["statusline"], line: "data" },
	{ key: "evals", args: ["statusline", "--db", join(ROOT, "data", "evals.jsonl")], line: "data" },
	{ key: "orchestrate", args: ["statusline"], line: "data" },
];

// ANSI severity palette (raw escapes — runtime pi-tui imports are forbidden).
const RESET = "\x1b[0m";
const COLOR: Record<string, (s: string) => string> = {
	ok: (s) => `\x1b[32m${s}${RESET}`, // green
	warn: (s) => `\x1b[33m${s}${RESET}`, // yellow
	err: (s) => `\x1b[1;31m${s}${RESET}`, // bold red
};
const paint = (raw: string): string => {
	const cut = raw.indexOf("|");
	if (cut < 0) return raw; // e.g. supervise's plain `name:state` output
	const level = raw.slice(0, cut);
	const text = raw.slice(cut + 1);
	return (COLOR[level] ?? ((s: string) => s))(text);
};

// Tools whose footer activity is shown (all registered crate tools).
const TOOLS = new Set([
	"semdb", "memory", "codegraph", "codeindex", "vault", "skills", "schedule",
	"search", "notify", "secrets", "browser", "orchestrate", "mcp", "sandbox",
	"context", "evals", "rag", "supervise",
]);
// Tool run → which segments to re-probe afterwards.
const REFRESH_AFTER: Record<string, string[]> = {
	supervise: ["supervise", "browser", "schedule"],
	browser: ["browser"],
	schedule: ["schedule"],
	search: ["search"],
	memory: ["memory"],
	rag: ["rag"],
	evals: ["evals"],
	orchestrate: ["orchestrate"],
	codegraph: ["codegraph"],
	secrets: ["secrets"],
	sandbox: ["sandbox"],
};

const CLEAR_AFTER_MS = 5000;
const REFRESH_MS = 30_000;

export default function (pi: ExtensionAPI) {
	const started = new Map<string, number>(); // toolCallId → start ms
	const clearTimers = new Map<string, ReturnType<typeof setTimeout>>();
	const painted = new Map<string, string>(); // segment key → colored text
	let ui: any; // latest ctx.ui, captured from events (only used when hasUI)
	let timer: ReturnType<typeof setInterval> | undefined;

	function render() {
		if (!ui) return;
		const row = (line: Line, icon: string) =>
			`${icon} ` +
			SEGMENTS.filter((s) => s.line === line)
				.map((s) => painted.get(s.key))
				.filter(Boolean)
				.join(" \x1b[2m·\x1b[0m ");
		ui.setWidget("smartagent-statusline", [row("infra", "⛭"), row("data", "▦")], {
			placement: "belowEditor",
		});
	}

	function probe(keys: string[]) {
		for (const key of keys) {
			const seg = SEGMENTS.find((s) => s.key === key);
			if (!seg) continue;
			execFile(BIN(seg.key), seg.args, { encoding: "utf8", timeout: 10_000, cwd: ROOT }, (err, stdout) => {
				painted.set(seg.key, err ? COLOR.err(`${seg.key}?`) : paint(stdout.trim()));
				render();
			});
		}
	}

	const probeAll = () => probe(SEGMENTS.map((s) => s.key));

	pi.on("session_start", async (_event, ctx) => {
		if (!ctx.hasUI) return;
		ui = ctx.ui;
		probeAll();
		if (!timer) timer = setInterval(probeAll, REFRESH_MS);
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
		ctx.ui.setStatus(event.toolName, `\x1b[36m${event.toolName} ⚙ running…${RESET}`);
	});

	pi.on("tool_execution_end", async (event: any, ctx) => {
		if (!ctx.hasUI || !TOOLS.has(event.toolName)) return;
		ui = ctx.ui;
		const t0 = started.get(event.toolCallId);
		started.delete(event.toolCallId);
		const ms = t0 ? Date.now() - t0 : 0;
		const done = event.isError
			? COLOR.err(`${event.toolName} ✗ ${ms}ms`)
			: COLOR.ok(`${event.toolName} ✓ ${ms}ms`);
		ctx.ui.setStatus(event.toolName, done);
		clearTimers.set(
			event.toolName,
			setTimeout(() => ctx.ui.setStatus(event.toolName, undefined), CLEAR_AFTER_MS),
		);
		const refresh = REFRESH_AFTER[event.toolName];
		if (refresh) probe(refresh);
	});
}
