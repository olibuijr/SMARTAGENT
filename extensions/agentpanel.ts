// agentpanel — GOAGENT-style AGENT TEAM sidebar for the pi TUI.
//
// A true right-side sidebar: nonCapturing pi-tui overlay anchored top-right —
// chat keeps the left side and keyboard focus (same mechanism as sa-browser).
// Visibility comes from a SOLID background fill + accent left edge: without
// them the panel is floating text and the chat bleeds straight through it.
//
// Sections: header (working count) → shared board task → working agents
// (dot+spinner, ⚙ tool ticker, last words) → idle agents (one dim line each)
// → RUNNING workflows (from `workflow runs` — visible even when the gateway
// is down) → /team hint.
//
// Data: `gateway agents` TSV — name, state, doing, role, tokens, tools, words
// (see crates/gateway/src/server.rs write_agents — keep in sync!) and
// `workflow runs` TSV. Display only — no logic.
// Auto-activates in the TUI; `/team` toggles.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = (name: string) => join(ROOT, "target", "release", name);
const REFRESH_MS = 5000;
const PANE_WIDTH = 47; // +30% for readability

const RESET = "\x1b[0m";
const BG = "\x1b[48;5;235m"; // solid panel background — the visibility fix
const BODY = "\x1b[38;5;250m"; // explicit body fg — every fragment restores it
const EDGE = `\x1b[38;5;60m▌${BODY}`;
// Self-contained color fragments: each sets its fg and restores BODY after —
// nothing inherits the edge color, and no SGR dim (dim over a dark bg is
// muddy and theme-dependent; explicit grays are predictable).
const fg = (c: string, s: string) => `\x1b[38;5;${c}m${s}${BODY}`;
const dim = (s: string) => fg("244", s);
const bold = (s: string) => `\x1b[1m${s}\x1b[22m`;
const ACCENTS = ["212", "80", "150", "215", "141", "117"];

// Circular avatar per agent: circled initial (Ⓑ Ⓜ Ⓞ Ⓠ …) in the agent's accent.
const avatar = (name: string): string => {
	const c = (name[0] ?? "?").toUpperCase().charCodeAt(0);
	return c >= 65 && c <= 90 ? String.fromCodePoint(0x24b6 + c - 65) : "◉";
};

// Captured extension context — lets the panel read live session state
// (context-window usage) without owning any logic.
let ctxRef: any;

type Agent = { name: string; state: string; task: string; role: string; tokens: string; tools: string; words: string };
type Run = { id: string; def: string; step: string; task: string };

let agents: Agent[] = [];
let runs: Run[] = [];
let gatewayUp = false;
let frame = 0;
const SPIN = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

function humanTokens(raw: string): string {
	const n = Number(raw);
	if (!Number.isFinite(n) || n <= 0) return "";
	if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "m";
	if (n >= 1_000) return Math.round(n / 1_000) + "k";
	return String(n);
}

function refresh(): void {
	try {
		const out = execFileSync(BIN("gateway"), ["agents"], { timeout: 3000, encoding: "utf8" });
		agents = out
			.split("\n")
			.filter((l) => l.includes("\t"))
			.map((l) => {
				const [name, state, task, role, tokens, tools, words] = l.split("\t");
				return {
					name: name ?? "?",
					state: state ?? "idle",
					task: (task ?? "").trim(),
					role: role ?? "Agent",
					tokens: (tokens ?? "").trim(),
					tools: (tools ?? "").trim(),
					words: (words ?? "").trim(),
				};
			})
			// working first, then by name — the active agents get the eye
			.sort((a, b) => Number(b.state === "working") - Number(a.state === "working") || a.name.localeCompare(b.name));
		gatewayUp = true;
	} catch {
		agents = [];
		gatewayUp = false;
	}
	try {
		const out = execFileSync(BIN("workflow"), ["runs", "--live", "--db", join(ROOT, "data", "workflow.semdb"), "--tasks-db", join(ROOT, "data", "tasks.semdb")], { timeout: 3000, encoding: "utf8" });
		runs = out
			.split("\n")
			.filter((l) => l.includes("\t") && !/\t(done|aborted)\t/.test(l))
			.map((l) => {
				const [id, def, _state, step, task] = l.split("\t");
				return { id: id ?? "?", def: def ?? "", step: (step ?? "").replace("step ", ""), task: (task ?? "").trim() };
			})
			.slice(0, 4);
	} catch {
		runs = [];
	}
}

const vlen = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, "").length;
const clip = (s: string, w: number) => (s.length > w ? s.slice(0, Math.max(0, w - 1)) + "…" : s);

/** One padded panel row: accent edge + content on the solid background. */
function row(content: string, width: number): string {
	const inner = width - 2; // edge + one space of padding
	const pad = Math.max(0, inner - vlen(content));
	return `${BG}${EDGE} ${content}${" ".repeat(pad)}${RESET}`;
}

function rule(width: number): string {
	return row(dim("─".repeat(Math.max(4, width - 4))), width);
}

function render(width: number): string[] {
	const w = Math.max(24, width);
	const inner = w - 4;
	const lines: string[] = [];
	const working = agents.filter((a) => a.state === "working");

	// Header: name + live status count — green when active, orange when the
	// whole fleet idles (status colors, never gray: the panel must look alive).
	const count = working.length ? `${working.length}/${agents.length} working` : "all idle";
	const gap = " ".repeat(Math.max(1, inner - vlen("AGENT TEAM") - count.length));
	lines.push(row(bold(fg("255", "AGENT TEAM")) + gap + (working.length ? fg("46", count) : fg("208", count)), w));
	lines.push(rule(w));
	// Context-window usage bar: fills as this session's context is consumed.
	{
		const u = ctxRef?.getContextUsage?.();
		if (u && u.percent != null) {
			const pct = Math.min(100, Math.round(u.percent));
			const label = ` ${pct}%`;
			const kk = `${Math.round((u.tokens ?? 0) / 1000)}k/${Math.round(u.contextWindow / 1000)}k`;
			const barw = Math.max(8, inner - label.length - kk.length - 6);
			const fill = Math.round((pct / 100) * barw);
			const col = pct >= 80 ? "203" : pct >= 50 ? "215" : "78";
			const bar = fg(col, "▕" + "█".repeat(fill)) + fg("238", "░".repeat(Math.max(0, barw - fill))) + fg(col, "▏");
			lines.push(row(dim("ctx ") + bar + fg(col, label) + " " + dim(kk), w));
		} else {
			lines.push(row(dim("ctx ▕░░░░░░░░▏ —"), w));
		}
	}

	if (!gatewayUp) {
		lines.push(row(fg("203", "✖ gateway offline"), w));
		lines.push(row(dim("supervise status → up"), w));
	} else {
		// The board `doing` slot is shared fleet-wide — show it ONCE, not
		// duplicated under every agent (that duplication was pure noise).
		const board = agents.find((a) => a.task && a.task !== "nothing")?.task ?? "";
		if (board) {
			lines.push(row(dim("board ") + fg("252", clip(board, inner - 6)), w));
		}
		lines.push(row("", w));

		// Stable accent per agent (by fleet position, not working-subset index)
		// so each agent keeps its color identity whether working or idle.
		const accentOf = (name: string) => ACCENTS[Math.max(0, agents.findIndex((x) => x.name === name)) % ACCENTS.length];

		working.forEach((a, i) => {
			const accent = accentOf(a.name);
			const spin = fg(accent, SPIN[(frame + i) % SPIN.length]);
			const head = `${fg(accent, avatar(a.name))} ${fg("46", "●")}${spin} ${bold(fg(accent, a.name))} ${dim("· " + a.role)}`;
			const tok = humanTokens(a.tokens);
			const gapw = Math.max(1, inner - vlen(head) - tok.length);
			lines.push(row(head + " ".repeat(gapw) + dim(tok), w));
			if (a.tools) {
				const tick = fg(accent, ["·  ", "·· ", "···"][frame % 3]);
				lines.push(row("  " + dim("⚙ ") + fg("252", clip(a.tools, inner - 8)) + " " + tick, w));
			}
			if (a.words) {
				lines.push(row("  " + dim("“" + clip(a.words, inner - 6) + "”"), w));
			}
		});

		// Idle agents: one line each — orange status dot with a slow breathing
		// pulse, name in the agent's accent. Present but calm, never dead-gray.
		agents
			.filter((a) => a.state !== "working")
			.forEach((a, i) => {
				const tok = humanTokens(a.tokens);
				const pulse = (frame + i * 2) % 6 < 3 ? fg("208", "●") : fg("130", "●");
				const head = `${fg(accentOf(a.name), avatar(a.name))} ${pulse} ${fg("250", a.name)} ${dim("· " + a.role)}`;
				const gapw = Math.max(1, inner - vlen(head) - tok.length);
				lines.push(row(head + " ".repeat(gapw) + dim(tok), w));
			});
	}

	// RUNNING: in-flight workflows — answers "is anything running?" at a
	// glance. Pinned to the BOTTOM of the full-height pane; the gap between
	// the team block and it is filled with background rows so the sidebar
	// reads as one solid column for the whole terminal height.
	const bottom: string[] = [rule(w)];
	if (runs.length === 0) {
		bottom.push(row(dim("no workflows running"), w));
	} else {
		bottom.push(row(bold(fg("117", "RUNNING")) + " " + fg("117", SPIN[frame % SPIN.length]), w));
		runs.forEach((r, i) => {
			const arrow = (frame + i) % 4 < 2 ? fg("46", "▶") : fg("29", "▶");
			bottom.push(row(`${arrow} ${fg("117", r.id)} ${clip(r.def, 9)} ${dim("step")} ${fg("252", r.step)}${r.task ? dim(" → " + r.task) : ""}`, w));
		});
	}
	bottom.push(rule(w));
	bottom.push(row(dim("/team hide"), w));

	const termRows = process.stdout.rows ?? 40;
	const filler = Math.max(0, termRows - lines.length - bottom.length);
	for (let i = 0; i < filler; i++) lines.push(row("", w));
	lines.push(...bottom);
	return lines;
}

export default function (pi: ExtensionAPI) {
	let tuiRef: any;
	let finish: ((r: unknown) => void) | undefined;
	let timer: ReturnType<typeof setInterval> | undefined;
	let spinTimer: ReturnType<typeof setInterval> | undefined;
	let active = false;

	function open(ctx: any): string {
		if (ctx.mode !== "tui") return "agent panel needs the interactive TUI";
		if (active) return "agent panel already active";
		active = true;
		ctx.ui
			.custom(
				(tui: any, _theme: any, _kb: any, done: (r: unknown) => void) => {
					tuiRef = tui;
					finish = done;
					return {
						render(width: number): string[] {
							return render(width);
						},
						invalidate() {},
					};
				},
				{
					overlay: true,
					// nonCapturing is LOAD-BEARING: custom() otherwise gives the
					// overlay keyboard focus, and since this panel auto-opens at
					// session_start it would swallow ALL typing — dead editor.
					overlayOptions: () => ({ width: PANE_WIDTH, anchor: "top-right", maxHeight: "100%", nonCapturing: true }),
					onHandle: (h: any) => {
						if (h.isFocused?.()) h.unfocus?.();
					},
				},
			)
			.finally(() => {
				active = false;
				tuiRef = undefined;
			});
		refresh();
		if (!timer) timer = setInterval(() => {
			refresh();
			tuiRef?.requestRender();
		}, REFRESH_MS);
		// animation frames: cheap (cache-only render + pi-tui diffing), no
		// process spawns. Unconditional — the idle breathing pulse and the
		// RUNNING ticker keep the panel alive even when no agent is mid-turn.
		if (!spinTimer)
			spinTimer = setInterval(() => {
				frame++;
				tuiRef?.requestRender();
			}, 350);
		return "agent team sidebar on (right) — /team toggles";
	}

	function close(): string {
		if (timer) {
			clearInterval(timer);
			timer = undefined;
		}
		if (spinTimer) {
			clearInterval(spinTimer);
			spinTimer = undefined;
		}
		finish?.(undefined);
		finish = undefined;
		active = false;
		return "agent team sidebar off";
	}

	pi.registerCommand("team", {
		description: "Toggle the AGENT TEAM sidebar (gateway fleet, GOAGENT-style)",
		handler: async (_args: string, ctx: any) => (active ? close() : open(ctx)),
	});

	pi.on("session_start", async (_e, ctx: any) => {
		ctxRef = ctx;
		if (ctx.mode === "tui" && !active) open(ctx);
	});
	pi.on("tool_execution_end", async (_e, ctx: any) => {
		ctxRef = ctx;
		tuiRef?.requestRender();
	});
	pi.on("session_shutdown", async () => {
		close();
	});
}
