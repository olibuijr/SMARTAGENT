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

type Agent = { name: string; state: string; task: string; role: string; activity: string };

function agents(): Agent[] {
	try {
		const out = execFileSync(GATEWAY, ["agents"], { timeout: 3000, encoding: "utf8" });
		return out
			.split("\n")
			.filter((l) => l.includes("\t"))
			.map((l) => {
				const [name, state, task, role, activity] = l.split("\t");
				return {
					name: name ?? "?",
					state: state ?? "idle",
					task: task ?? "",
					role: role ?? "Agent",
					activity: (activity ?? "").trim(),
				};
			})
			.sort((a, b) => a.name.localeCompare(b.name));
	} catch {
		return [];
	}
}

const clip = (s: string, w: number) => (s.length > w ? s.slice(0, Math.max(0, w - 1)) + "…" : s);

function render(width: number): string[] {
	const w = Math.max(20, width - 2);
	const list = agents();
	const lines: string[] = ["", " " + bold("AGENT TEAM"), ""];
	if (list.length === 0) {
		lines.push(" " + dim("no agents — gateway offline"));
		return lines;
	}
	list.forEach((a, i) => {
		const accent = ACCENTS[i % ACCENTS.length];
		const working = a.state === "working";
		// state dot: green = working, orange = idle
		const icon = working ? color("46", "●") : color("208", "●");
		const name = working ? bold(color(accent, a.name)) : color("252", a.name);
		lines.push(` ${icon} ${name} ${dim("· " + a.role)}`);
		const task = a.task && a.task !== "nothing" ? clip(a.task, w - 4) : "idle";
		lines.push("    " + (working ? color(accent, task) : dim(task)));
		if (a.activity) {
			lines.push("    " + dim(clip(a.activity, w - 4)));
		}
		lines.push("");
	});
	return lines;
}

export default function (pi: ExtensionAPI) {
	let tuiRef: any;
	let finish: ((r: unknown) => void) | undefined;
	let timer: ReturnType<typeof setInterval> | undefined;
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
					overlayOptions: () => ({ width: PANE_WIDTH, anchor: "top-right" }),
				},
			)
			.finally(() => {
				active = false;
				tuiRef = undefined;
			});
		if (!timer) timer = setInterval(() => tuiRef?.requestRender(), REFRESH_MS);
		return "agent team sidebar on (right) — /team toggles";
	}

	function close(): string {
		if (timer) {
			clearInterval(timer);
			timer = undefined;
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
