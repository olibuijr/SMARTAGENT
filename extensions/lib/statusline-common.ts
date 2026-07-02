import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
export const BIN = (name: string) => join(ROOT, "target", "release", name);
export const TASKS_DB = join(ROOT, "data", "tasks.semdb");
export const WORKFLOW_DB = join(ROOT, "data", "workflow.semdb");

export type StatusLine = "workspace" | "data" | "infra";
export type StatusSegment = { key: string; args: string[]; line: StatusLine };

export const STATUS_SEGMENTS: StatusSegment[] = [
	{ key: "codegraph", args: ["statusline", join(ROOT, "data", "codegraph.json")], line: "workspace" },
	{ key: "codeindex", args: ["statusline"], line: "workspace" },
	{ key: "tasks", args: ["statusline", "--db", TASKS_DB], line: "workspace" },
	{ key: "workflow", args: ["statusline", "--root", ROOT, "--db", WORKFLOW_DB], line: "workspace" },
	{ key: "memory", args: ["statusline", "--dir", join(ROOT, "data", "memory")], line: "data" },
	{ key: "rag", args: ["statusline", join(ROOT, "data", "rag.semdb")], line: "data" },
	{ key: "schedule", args: ["statusline"], line: "data" },
	{ key: "evals", args: ["statusline", "--db", join(ROOT, "data", "evals.semdb")], line: "data" },
	{ key: "orchestrate", args: ["statusline"], line: "data" },
	{ key: "gateway", args: ["statusline"], line: "workspace" },
	{ key: "supervise", args: ["statusline"], line: "infra" },
	{ key: "sandbox", args: ["statusline"], line: "infra" },
	{ key: "secrets", args: ["statusline", "--store", join(ROOT, "data", "secrets")], line: "infra" },
	{ key: "browser", args: ["statusline"], line: "infra" },
	{ key: "search", args: ["statusline"], line: "infra" },
	{ key: "hooks", args: ["statusline", "--root", ROOT], line: "infra" },
];

export function parseLevel(raw: string): { level: string; text: string } {
	const cut = raw.indexOf("|");
	return cut > 0 ? { level: raw.slice(0, cut), text: raw.slice(cut + 1) } : { level: "err", text: raw };
}

export const statusTag = (level: string) => (level === "ok" ? "✓" : level === "warn" ? "⚠" : "✗");

// Live layout state shared between extensions in-process: overlays that
// reserve horizontal space (the agentpanel sidebar) register their width here
// so width-aware widgets (statusline) lay out for the REMAINING columns
// instead of the full terminal.
export const layout = { sidebarCols: 0 };
export const availableColumns = (): number => Math.max(40, (process.stdout.columns || 120) - layout.sidebarCols);

// Rendered-cell width (wcwidth-accurate for kitty) — shared so every
// widget pads by CELLS, not chars (mixed-width glyphs skewed borders).
export const stripAnsi = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, "");
export function isWide(c: number): boolean {
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
export function vwidth(s: string): number {
	// Empirically probed 2026-07-02: THIS tmux renders every glyph the TUI
	// uses (emoji, sextants, symbols) at ONE cell — inside tmux, char count
	// IS the rendered width. The wcwidth tables below apply to bare kitty.
	if (process.env.TMUX) return [...stripAnsi(s)].filter((ch) => ch.codePointAt(0) !== 0xfe0f && ch.codePointAt(0) !== 0x200d).length;
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
