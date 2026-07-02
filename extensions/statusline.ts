/**
 * statusline — pi extension that surfaces SMARTAGENT tool + service statuses
 * in the TUI, using pi's footer status entries and a belowEditor widget.
 *
 * Four surfaces:
 *  - Per-tool activity: tool_execution_start/end events → ctx.ui.setStatus(tool, …)
 *    (⚙ running, then ✓/✗ with duration; auto-clears after a few seconds).
 *  - Workspace line (belowEditor, first — most task-relevant): 🕸 code graph ·
 *    🗃 workspace repo index · 📋 tasks board · ▶ workflow run.
 *  - Data line (belowEditor): 🧠 memory · 📚 rag corpus · ⏰ schedule ·
 *    📊 evals · 🤖 orchestrate — re-probed after related tools run.
 *  - Infra line (belowEditor, last — least volatile): ⛭ services · 🧱 sandbox ·
 *    🔑 secrets auth · 🌐 chrome · 🔎 searx · 🪝 hooks.
 *  Healthy segments collapse to `icon Name ✓`; a segment only spends width on
 *  detail when its level is warn/err (the level is judged in Rust).
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
// Lines are scope-grouped: workspace (per-repo work state) → data (agent
// stores) → infra (host services). Workspace first: most task-relevant.
type Line = "workspace" | "data" | "infra";
const SEGMENTS: { key: string; args: string[]; line: Line }[] = [
	{ key: "codegraph", args: ["statusline", join(ROOT, "data", "codegraph.json")], line: "workspace" },
	{ key: "codeindex", args: ["statusline"], line: "workspace" },
	{ key: "tasks", args: ["statusline", "--db", join(ROOT, "data", "tasks.semdb")], line: "workspace" },
	{ key: "workflow", args: ["statusline", "--root", ROOT, "--db", join(ROOT, "data", "workflow.semdb")], line: "workspace" },
	{ key: "memory", args: ["statusline", "--dir", join(ROOT, "data", "memory")], line: "data" },
	{ key: "rag", args: ["statusline", join(ROOT, "data", "rag.semdb")], line: "data" },
	{ key: "schedule", args: ["statusline"], line: "data" },
	{ key: "evals", args: ["statusline", "--db", join(ROOT, "data", "evals.jsonl")], line: "data" },
	{ key: "orchestrate", args: ["statusline"], line: "data" },
	{ key: "gateway", args: ["statusline"], line: "workspace" },
	{ key: "supervise", args: ["statusline"], line: "infra" },
	{ key: "sandbox", args: ["statusline"], line: "infra" },
	{ key: "secrets", args: ["statusline", "--store", join(ROOT, "data", "secrets")], line: "infra" },
	{ key: "browser", args: ["statusline"], line: "infra" },
	{ key: "search", args: ["statusline"], line: "infra" },
	{ key: "hooks", args: ["statusline", "--root", ROOT], line: "infra" },
];
// Line prefix icons, in display order.
const LINES: { line: Line; icon: string }[] = [
	{ line: "workspace", icon: "⌂" },
	{ line: "data", icon: "▦" },
	{ line: "infra", icon: "⛭" },
];

// ANSI severity palette (raw escapes — runtime pi-tui imports are forbidden).
const RESET = "\x1b[0m";
const COLOR: Record<string, (s: string) => string> = {
	ok: (s) => `\x1b[32m${s}${RESET}`, // green
	warn: (s) => `\x1b[33m${s}${RESET}`, // yellow
	err: (s) => `\x1b[1;31m${s}${RESET}`, // bold red
};
// Display label per segment (Title-case tool name, a few friendlier names).
const LABEL: Record<string, string> = {
	supervise: "Services",
	codegraph: "Code",
	codeindex: "Index",
	orchestrate: "Agents",
	rag: "Docs",
	gateway: "DA",
};
const label = (key: string) => LABEL[key] ?? key[0].toUpperCase() + key.slice(1);
// Infra segments are pure health checks — when ok, their detail ("chrome✓",
// "2/2 up") is redundant with the level, so they collapse to `icon Name ✓`.
// Workspace/data segments carry real stats (board counts, repos indexed,
// memory tiers) and keep their detail: bold Name, plain text, no color wash.
const COMPACT = new Set(SEGMENTS.filter((s) => s.line === "infra").map((s) => s.key));
const paint = (key: string, raw: string): string => {
	const cut = raw.indexOf("|");
	if (cut < 0) return raw;
	const level = raw.slice(0, cut);
	const text = raw.slice(cut + 1);
	const sp = text.indexOf(" ");
	const first = sp > 0 ? text.slice(0, sp) : "";
	const iconLike = first.length > 0 && /[^\x00-\x7f]/.test(first);
	const icon = iconLike ? `${first} ` : "";
	const rest = iconLike ? text.slice(sp + 1) : text;
	if (level === "ok") {
		return COMPACT.has(key)
			? `${icon}\x1b[1m${label(key)}\x1b[22m ${COLOR.ok("✓")}`
			: `${icon}\x1b[1m${label(key)}\x1b[22m ${rest} ${COLOR.ok("✓")}`;
	}
	return (COLOR[level] ?? ((s: string) => s))(`${icon}\x1b[1m${label(key)}:\x1b[22m ${rest}`);
};

// Tools whose footer activity is shown (all registered crate tools).
const TOOLS = new Set([
	"semdb", "memory", "codegraph", "codeindex", "vault", "skills", "schedule",
	"search", "notify", "secrets", "browser", "orchestrate", "mcp", "sandbox",
	"context", "evals", "rag", "supervise", "tasks", "workflow",
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
	orchestrate: ["orchestrate", "codeindex"],
	codegraph: ["codegraph"],
	codeindex: ["codeindex"],
	secrets: ["secrets"],
	sandbox: ["sandbox"],
	tasks: ["tasks"],
	workflow: ["workflow"],
};

const CLEAR_AFTER_MS = 5000;
const REFRESH_MS = 30_000;

export default function (pi: ExtensionAPI) {
	const started = new Map<string, number>(); // toolCallId → start ms
	const clearTimers = new Map<string, ReturnType<typeof setTimeout>>();
	const painted = new Map<string, string>(); // segment key → colored text
	let ui: any; // latest ctx.ui, captured from events (only used when hasUI)
	let timer: ReturnType<typeof setInterval> | undefined;

	// Visible width of a cell: ANSI codes count 0, wide glyphs 2. Wide follows
	// wcwidth/kitty: East-Asian-Wide + DEFAULT-EMOJI-PRESENTATION ranges only.
	// Text-presentation pictographs (🕸 U+1F578, 🗃 U+1F5C3, ▶, ⛭, ▣, ⌂, ▦)
	// render ONE cell in kitty — counting them 2 skewed the top row's columns.
	const stripAnsi = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, "");
	function isWide(c: number): boolean {
		return (
			(c >= 0x1100 && c <= 0x115f) || (c >= 0x2e80 && c <= 0xa4cf) ||
			(c >= 0xac00 && c <= 0xd7a3) || (c >= 0xf900 && c <= 0xfaff) ||
			(c >= 0xfe30 && c <= 0xfe4f) ||
			(c >= 0x231a && c <= 0x231b) || (c >= 0x23e9 && c <= 0x23ec) ||
			c === 0x23f0 || c === 0x23f3 || (c >= 0x25fd && c <= 0x25fe) ||
			(c >= 0x2614 && c <= 0x2615) || (c >= 0x2648 && c <= 0x2653) ||
			c === 0x267f || c === 0x2693 || c === 0x26a1 ||
			(c >= 0x26aa && c <= 0x26ab) || (c >= 0x26bd && c <= 0x26be) ||
			(c >= 0x26c4 && c <= 0x26c5) || c === 0x26ce || c === 0x26d4 ||
			c === 0x26ea || (c >= 0x26f2 && c <= 0x26f3) || c === 0x26f5 ||
			c === 0x26fa || c === 0x26fd || c === 0x2705 ||
			(c >= 0x270a && c <= 0x270b) || c === 0x2728 || c === 0x274c ||
			c === 0x274e || (c >= 0x2753 && c <= 0x2755) || c === 0x2757 ||
			(c >= 0x2795 && c <= 0x2797) || c === 0x27b0 || c === 0x27bf ||
			(c >= 0x2b1b && c <= 0x2b1c) || c === 0x2b50 || c === 0x2b55 ||
			(c >= 0x1f300 && c <= 0x1f53d) || (c >= 0x1f550 && c <= 0x1f567) ||
			(c >= 0x1f5fb && c <= 0x1f64f) || (c >= 0x1f680 && c <= 0x1f6ff) ||
			(c >= 0x1f7e0 && c <= 0x1f7f0) || (c >= 0x1f90c && c <= 0x1f9ff) ||
			(c >= 0x1fa70 && c <= 0x1faff)
		);
	}
	function vwidth(s: string): number {
		let w = 0;
		let narrowSym = false; // last glyph was a narrow symbol (VS16 can widen it)
		for (const ch of stripAnsi(s)) {
			const c = ch.codePointAt(0) ?? 0;
			if (c === 0x200d) continue; // ZWJ
			if (c === 0xfe0f) {
				// VS16 upgrades a preceding narrow symbol to emoji (2 cells).
				if (narrowSym) { w += 1; narrowSym = false; }
				continue;
			}
			const wide = isWide(c);
			narrowSym = !wide && c >= 0x2100;
			w += wide ? 2 : 1;
		}
		return w;
	}

	function render() {
		if (!ui) return;
		// Segments per line, in registry order.
		const cells = LINES.map(({ line }) =>
			SEGMENTS.filter((s) => s.line === line)
				.map((s) => painted.get(s.key))
				.filter(Boolean) as string[],
		);
		// Column i is padded to the widest cell i across all lines, so the
		// separators line up vertically in a grid.
		const cols = Math.max(0, ...cells.map((r) => r.length));
		const widths = Array.from({ length: cols }, (_, i) =>
			Math.max(0, ...cells.map((r) => (r[i] ? vwidth(r[i]) : 0))),
		);
		const rows = cells.map(
			(r, li) =>
				`${LINES[li].icon} ` +
				r
					.map((c, i) => c + " ".repeat(Math.max(0, widths[i] - vwidth(c))))
					.join(" \x1b[2m·\x1b[0m ")
					.trimEnd(),
		);
		ui.setWidget("smartagent-statusline", rows, {
			placement: "belowEditor",
		});
	}

	function probe(keys: string[]) {
		for (const key of keys) {
			const seg = SEGMENTS.find((s) => s.key === key);
			if (!seg) continue;
			execFile(BIN(seg.key), seg.args, { encoding: "utf8", timeout: 10_000, cwd: ROOT }, (err, stdout) => {
				painted.set(seg.key, err ? COLOR.err(`${seg.key.toUpperCase()}: unavailable`) : paint(seg.key, stdout.trim()));
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
