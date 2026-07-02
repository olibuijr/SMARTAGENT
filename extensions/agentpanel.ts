// agentpanel — GOAGENT-style AGENT TEAM sidebar for the pi TUI.
//
// A true right-side sidebar (like ../GOAGENT's 30-col lipgloss pane): a
// nonCapturing pi-tui overlay anchored top-right — chat keeps the left side
// and keyboard focus (same mechanism sa-browser proved). One block per
// gateway agent: ◉/○ state icon, accent-coloured name, faint role, its task,
// and a live last-activity snippet from the agent's own transcript.
// Data: `gateway agents` TSV (name, state, task, role, activity). Display
// only — no logic. Auto-activates in the TUI; `/team` toggles.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const GATEWAY = join(ROOT, "target", "release", "gateway");
const REFRESH_MS = 5000;
const PANE_WIDTH = 36;

const RESET = "\x1b[0m";
const dim = (s: string) => `\x1b[2m${s}${RESET}`;
const bold = (s: string) => `\x1b[1m${s}${RESET}`;
const color = (c: string, s: string) => `\x1b[38;5;${c}m${s}${RESET}`;
const ACCENTS = ["212", "80", "150", "215", "141", "117"];

type Agent = { name: string; state: string; task: string; role: string; tools: string; words: string };

// Data cache: refreshed every REFRESH_MS by a timer; render() must stay cheap
// because the spinner timer re-renders several times a second.
let cache: Agent[] = [];
let frame = 0;
const SPIN = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

function refreshAgents(): void {
	try {
		const out = execFileSync(GATEWAY, ["agents"], { timeout: 3000, encoding: "utf8" });
		cache = out
			.split("\n")
			.filter((l) => l.includes("\t"))
			.map((l) => {
				const [name, state, task, role, tools, words] = l.split("\t");
				return {
					name: name ?? "?",
					state: state ?? "idle",
					task: task ?? "",
					role: role ?? "Agent",
					tools: (tools ?? "").trim(),
					words: (words ?? "").trim(),
				};
			})
			.sort((a, b) => a.name.localeCompare(b.name));
	} catch {
		cache = [];
	}
}

const clip = (s: string, w: number) => (s.length > w ? s.slice(0, Math.max(0, w - 1)) + "…" : s);

function render(width: number): string[] {
	const w = Math.max(20, width - 2);
	const lines: string[] = ["", " " + bold("AGENT TEAM"), ""];
	if (cache.length === 0) {
		lines.push(" " + dim("no agents — gateway offline"));
		return lines;
	}
	cache.forEach((a, i) => {
		const accent = ACCENTS[i % ACCENTS.length];
		const working = a.state === "working";
		// green working / orange idle; working agents get a live spinner
		const dot = working ? color("46", "●") : color("208", "●");
		const spin = working ? " " + color(accent, SPIN[(frame + i) % SPIN.length]) : "  ";
		const name = working ? bold(color(accent, a.name)) : color("252", a.name);
		lines.push(` ${dot}${spin}${name} ${dim("· " + a.role)}`);
		const task = a.task && a.task !== "nothing" ? clip(a.task, w - 5) : "idle";
		lines.push("     " + (working ? color(accent, task) : dim(task)));
		if (a.tools) {
			// tool ticker: marching dots while working, static when idle
			const tick = working ? color(accent, ["·  ", "·· ", "···"][frame % 3]) : dim("···");
			lines.push("     " + dim("⚙ ") + clip(a.tools, w - 10) + " " + tick);
		}
		if (a.words) {
			lines.push("     " + dim("“" + clip(a.words, w - 8) + "”"));
		}
		lines.push("");
	});
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
					overlayOptions: () => ({ width: PANE_WIDTH, anchor: "top-right", nonCapturing: true }),
					onHandle: (h: any) => {
						if (h.isFocused?.()) h.unfocus?.();
					},
				},
			)
			.finally(() => {
				active = false;
				tuiRef = undefined;
			});
		refreshAgents();
		if (!timer) timer = setInterval(refreshAgents, REFRESH_MS);
		// spinner/ticker frames: cheap (cache-only render), no process spawns
		if (!spinTimer)
			spinTimer = setInterval(() => {
				frame++;
				if (cache.some((a) => a.state === "working")) tuiRef?.requestRender();
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
		if (ctx.mode === "tui" && !active) open(ctx);
	});
	pi.on("tool_execution_end", async () => {
		tuiRef?.requestRender();
	});
	pi.on("session_shutdown", async () => {
		close();
	});
}
