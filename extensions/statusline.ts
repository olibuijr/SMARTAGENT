/**
 * statusline — pi extension that surfaces SMARTAGENT tool + service statuses
 * in the TUI, using pi's footer status entries and a belowEditor widget.
 *
 * Four surfaces:
 *  - Per-tool activity: tool_execution_start/end events → ctx.ui.setStatus(tool, …)
 *    (⚙ running, then ✓/✗ with duration; auto-clears after a few seconds).
 *  - Workspace line (belowEditor, first — most task-relevant): 🕸 code graph ·
 *    🗃 workspace repo index · 📋 tasks board · ▶ workflow run.
 *  - Data line (belowEditor): 🧠 memory · 📚 corpus · ⏰ schedule ·
 *    📊 evals · 🤖 orchestrate — re-probed after related tools run.
 *  - Infra line (belowEditor, last — least volatile): ⛭ services · 🧱 sandbox ·
 *    🔑 secrets auth · 🌐 chrome · 🔎 searx · 🪝 hooks.
 *  Healthy segments collapse to `icon Name ✓`; a segment only spends width on
 *  detail when its level is warn/err (the level is judged in Rust).
 *
 * Every segment comes from a Rust `statusline` verb emitting `level|icon text`;
 * severity classification lives in Rust, this file only maps level → ANSI color
 * (ok=green tick text stays default, warn=yellow, err=red) and places lines.
 * Type-only pi imports; shared local statusline metadata lives in extensions/lib/.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFile } from "node:child_process";
import { BIN, ROOT, STATUS_SEGMENTS, parseLevel, layout, stripAnsi, vwidth } from "./lib/statusline-common.ts";

const SEGMENTS = STATUS_SEGMENTS;

// ── Segment registry: tool → binary args + which widget line it lives on ──
// Lines are scope-grouped: workspace (per-repo work state) → data (agent
// stores) → infra (host services). Workspace first: most task-relevant.
// Line prefix icons, in display order.
const LINES: { line: "workspace" | "data" | "infra"; icon: string }[] = [
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
	rag: "Corpus",
	gateway: "DA",
};
const label = (key: string) => LABEL[key] ?? key[0].toUpperCase() + key.slice(1);
// Infra segments are pure health checks — when ok, their detail ("chrome✓",
// "2/2 up") is redundant with the level, so they collapse to `icon Name ✓`.
// Workspace/data segments carry real stats (board counts, repos indexed,
// memory tiers) and keep their detail: bold Name, plain text, no color wash.
const COMPACT = new Set(SEGMENTS.filter((s) => s.line === "infra").map((s) => s.key));
const paint = (key: string, raw: string): string => {
	const { level, text } = parseLevel(raw);
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

	function compactOkCell(cell: string): string {
		if (!cell.includes(COLOR.ok("✓"))) return cell;
		const plain = stripAnsi(cell);
		const icon = plain.match(/^\S+ /)?.[0] ?? "";
		const name = plain.slice(icon.length).split(/\s+/)[0] ?? "";
		return `${icon}\x1b[1m${name}\x1b[22m ${COLOR.ok("✓")}`;
	}

	function iconOnlyCell(cell: string): string {
		const plain = stripAnsi(cell);
		const icon = plain.match(/^\S+/)?.[0] ?? "•";
		return cell.includes(COLOR.ok("✓")) ? `${icon} ${COLOR.ok("✓")}` : icon;
	}

	function rowsFor(cells: string[][]): string[] {
		const cols = Math.max(0, ...cells.map((r) => r.length));
		const widths = Array.from({ length: cols }, (_, i) =>
			Math.max(0, ...cells.map((r) => (r[i] ? vwidth(r[i]) : 0))),
		);
		return cells.map(
			(r, li) =>
				`${LINES[li].icon} ` +
				r
					.map((c, i) => c + " ".repeat(Math.max(0, widths[i] - vwidth(c))))
					.join(" \x1b[2m·\x1b[0m ")
					.trimEnd(),
		);
	}

	function fitRows(cells: string[][], width: number): string[] {
		// Degrade deterministically: full detail → compact ok details → icons only → fewer columns.
		for (const variant of [cells, cells.map((r) => r.map(compactOkCell)), cells.map((r) => r.map(iconOnlyCell))]) {
			const rows = rowsFor(variant);
			if (rows.every((r) => vwidth(r) <= width)) return rows;
		}
		let trimmed = cells.map((r) => r.map(iconOnlyCell));
		while (trimmed.some((r) => vwidth(rowsFor([r])[0] ?? "") > width) || rowsFor(trimmed).some((r) => vwidth(r) > width)) {
			const longest = trimmed.reduce((best, r, i) => (r.length > trimmed[best].length ? i : best), 0);
			if (trimmed[longest].length === 0) break;
			trimmed[longest] = trimmed[longest].slice(0, -1);
		}
		return rowsFor(trimmed);
	}

	function render() {
		if (!ui) return;
		const cells = LINES.map(({ line }) =>
			SEGMENTS.filter((s) => s.line === line)
				.map((s) => painted.get(s.key))
				.filter(Boolean) as string[],
		);
		// Component form: pi-tui passes the LIVE terminal width at render time
		// (process.stdout.columns went stale on resize / high-res fullscreen —
		// the widget collapsed to icons at 2K). Subtract the sidebar when open.
		ui.setWidget(
			"smartagent-statusline",
			(_tui: any, _theme: any) => ({
				render(width: number): string[] {
					return fitRows(cells, Math.max(40, width - layout.sidebarCols));
				},
				invalidate() {},
			}),
			{ placement: "belowEditor" },
		);
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
		process.stdout.on("resize", render);
		process.on("SIGWINCH", render);
		probeAll();
		if (!timer) timer = setInterval(probeAll, REFRESH_MS);
	});

	pi.on("session_shutdown", async () => {
		if (timer) clearInterval(timer);
		process.stdout.off("resize", render);
		process.off("SIGWINCH", render);
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
