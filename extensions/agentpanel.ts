// agentpanel — GOAGENT-style "who is doing what" panel, ported to the pi TUI.
//
// GOAGENT (a standalone Bubbletea app) draws a 30-col LEFT sidebar with a block
// per team member: ◉/○ icon, coloured name, faint role, live activity. pi's
// widget API only offers aboveEditor|belowEditor (no left panel), so this is
// the faithful port: an aboveEditor panel, one row per gateway-hosted agent,
// with the same glyphs and colour language. Data comes from `gateway agents`
// (falls back gracefully when the gateway is down). Pure display; no logic.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const GATEWAY = join(ROOT, "target", "release", "gateway");
const REFRESH_MS = 5000;

const RESET = "\x1b[0m";
const dim = (s: string) => `\x1b[2m${s}${RESET}`;
const bold = (s: string) => `\x1b[1m${s}${RESET}`;
const color = (c: string, s: string) => `\x1b[38;5;${c}m${s}${RESET}`;

// Per-agent accent colours (256-palette), cycled by index — mirrors GOAGENT.
const ACCENTS = ["212", "80", "150", "215", "141", "117"];

type Agent = { name: string; state: string; task: string; role: string };

function agents(): Agent[] {
	try {
		const out = execFileSync(GATEWAY, ["agents"], { timeout: 3000, encoding: "utf8" });
		// one line per agent: name<TAB>state<TAB>task<TAB>role
		return out
			.split("\n")
			.filter((l) => l.includes("\t"))
			.map((l) => {
				const [name, state, task, role] = l.split("\t");
				return { name, state: state || "idle", task: task || "", role: role || "agent" };
			});
	} catch {
		return [];
	}
}

function render(list: Agent[]): string[] {
	if (list.length === 0) return [dim("  no agents — gateway offline")];
	const lines: string[] = [bold("AGENT TEAM")];
	list.forEach((a, i) => {
		const accent = ACCENTS[i % ACCENTS.length];
		const working = a.state === "working";
		const icon = working ? color(accent, "◉") : dim("○");
		const name = working ? bold(color(accent, a.name)) : color("252", a.name);
		const task = a.task ? (working ? color(accent, a.task) : dim(a.task)) : dim("idle");
		lines.push(`${icon} ${name} ${dim("·")} ${a.role}`);
		lines.push(`   ${task}`);
	});
	return lines;
}

export default function (pi: ExtensionAPI) {
	let ui: any;
	let timer: ReturnType<typeof setInterval> | undefined;
	const paint = () => ui?.setWidget("smartagent-agentpanel", render(agents()), { placement: "aboveEditor" });

	pi.on("session_start", async (_event, ctx) => {
		ui = ctx.ui;
		paint();
		if (!timer) timer = setInterval(paint, REFRESH_MS);
	});
	pi.on("tool_execution_end", async (_event, ctx) => {
		ui = ctx.ui;
		paint();
	});
	pi.on("session_shutdown", async () => {
		if (timer) clearInterval(timer);
	});
}
