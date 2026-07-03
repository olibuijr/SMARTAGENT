/**
 * sa-browser — visual browser pane for the pi TUI.
 *
 * Thin glue over target/release/sa-browser (all pixel work — CDP screenshot,
 * zlib/PNG decode, half-block truecolor art — happens in Rust). When
 * activated, shows a persistent right-side overlay pane (width 50%, anchored
 * top-right, nonCapturing so chat keeps focus and the left half of the
 * viewport): address bar + loading status on top, page art below.
 *
 * Also exposes browser-style click/type actions; when the pane is active they
 * repaint it after the interaction so the visual surface stays current.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFile } from "node:child_process";
import { bin, bold, dim, makeRunner, ROOT, stripAnsi, untrusted } from "./lib/common.ts";

const BIN = bin("sa-browser");
const REFRESH_MS = 5000;
const runSync = makeRunner(
	BIN,
	60_000,
	/unreachable|refused|connect/i,
	"[hint: chromium may be down — call supervise (status, then up)]",
);

function runAsync(args: string[]): Promise<string> {
	return new Promise((resolve) => {
		execFile(BIN, args, { encoding: "utf8", timeout: 60_000, cwd: ROOT, maxBuffer: 16 * 1024 * 1024 }, (e, stdout, stderr) => {
			if (e) {
				const msg = (stderr || e.message || "").toString().trim().replace(/^error:\s*/, "");
				if (/unreachable|refused|connect/i.test(msg)) resolve(`error: ${msg}\n[hint: chromium may be down — call supervise (status, then up)]`);
				else resolve(`error: ${msg}`);
			} else resolve(stdout.trimEnd());
		});
	});
}

// ── pane state (display only; content comes from the binary) ──────────────
let handle: any; // OverlayHandle
let finish: ((v: unknown) => void) | undefined;
let timer: ReturnType<typeof setInterval> | undefined;
let tuiRef: any;
let artLines: string[] = [];
let addressUrl = "";
let addressTitle = "";
let loadState = "idle"; // idle | loading | complete | interactive | error:<msg>
let inFlight = false;
let paneCols = 78;
let deviceMode = "tablet"; // tablet (default) | none — passed to the binary
let pixelMode = "braille"; // braille (2x4 px/cell, default) | sextant | quad | half

function headerLines(width: number): string[] {
	const inner = Math.max(10, width - 2);
	const spin = loadState === "loading" ? "⏳" : loadState.startsWith("error") ? "✖" : "●";
	const state = loadState === "loading" ? "loading…" : loadState.startsWith("error") ? loadState : `${loadState}`;
	const url = addressUrl || "(no page)";
	const bar = ` ${spin} ${url}`.slice(0, inner);
	const sub = `   ${addressTitle}`.slice(0, inner);
	return [
		bold(`┌${"─".repeat(inner)}┐`.slice(0, width)),
		bold("│") + bar.padEnd(inner) + bold("│"),
		dim("│") + dim(sub.padEnd(inner)) + dim("│"),
		bold(`└${"─".repeat(inner)}┘`.slice(0, width)) + " " + dim(state),
	];
}

async function repaint(navigateUrl?: string) {
	if (inFlight) return;
	inFlight = true;
	if (navigateUrl) {
		addressUrl = navigateUrl;
		loadState = "loading";
	}
	tuiRef?.requestRender();
	// Fill the pane to the bottom: header is 4 rows, leave 1 row slack.
	const rows = Math.max(8, (process.stdout.rows ?? 40) - 5);
	const args = ["pane", "--cols", String(paneCols), "--rows", String(rows), "--device", deviceMode, "--pixels", pixelMode];
	if (navigateUrl) args.push("--url", navigateUrl);
	const out = await runAsync(args);
	if (out.startsWith("error")) {
		loadState = `error: ${stripAnsi(out).split("\n")[0].slice(0, 60)}`;
		artLines = [dim(stripAnsi(out))];
	} else {
		const nl = out.indexOf("\n");
		const header = nl === -1 ? out : out.slice(0, nl);
		const [url = "", title = "", ready = ""] = header.split("\t");
		addressUrl = url;
		addressTitle = title;
		loadState = ready || "complete";
		artLines = nl === -1 ? [] : out.slice(nl + 1).split("\n");
	}
	inFlight = false;
	tuiRef?.requestRender();
}

function deactivate() {
	if (timer) clearInterval(timer);
	timer = undefined;
	handle?.hide();
	handle = undefined;
	finish?.(undefined);
	finish = undefined;
	artLines = [];
	loadState = "idle";
}

function activate(ctx: any, url?: string): string {
	if (ctx.mode !== "tui") {
		return "sa-browser pane needs the interactive TUI; use action 'open' or 'snapshot' for text output here.";
	}
	if (handle) {
		if (url) void repaint(url);
		return "sa-browser pane already active" + (url ? `; loading ${url}` : "");
	}
	tuiRef = undefined;
	ctx.ui
		.custom(
			(tui: any, _theme: any, _keybindings: any, done: (r: unknown) => void) => {
				tuiRef = tui;
				finish = done; // resolves the custom() promise at deactivate
				return {
					render(width: number): string[] {
						const cols = Math.max(10, width - 2);
						if (cols !== paneCols) {
							paneCols = cols;
							void repaint(); // re-render art at the new width
						}
						return [...headerLines(width), ...artLines];
					},
					invalidate() {},
				};
			},
			{
				overlay: true,
				overlayOptions: () => ({
					width: "50%",
					anchor: "top-right",
					maxHeight: "100%",
					nonCapturing: true,
				}),
				onHandle: (h: any) => {
					handle = h;
					if (h.isFocused?.()) h.unfocus?.(); // chat keeps the keyboard
				},
			},
		)
		.then(() => {})
		.catch(() => {});
	timer = setInterval(() => void repaint(), REFRESH_MS);
	void repaint(url);
	return "sa-browser pane activated (right 50%, chat keeps the left)" + (url ? `; loading ${url}` : "");
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "sa-browser",
		label: "SA Browser",
		description:
			"Visual browser: drives real Chrome over CDP and renders the page as high-DPI terminal art in a right-side TUI pane (chat stays left). Actions: 'activate' opens the pane (optional url), 'deactivate' closes it, 'open' navigates and returns the DOM snapshot, 'click' clicks a CSS selector/link, 'type' fills an input/textarea (optionally enter), 'snapshot' returns the current page's DOM snapshot, 'status' returns url/title/readyState, 'probe' checks DevTools. The pane shows an address bar and loading status and refreshes itself.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["activate", "deactivate", "open", "click", "type", "snapshot", "status", "probe"] },
				url: { type: "string", description: "URL to load (activate/open)" },
				selector: { type: "string", description: "CSS selector for click/type" },
				text: { type: "string", description: "Text to fill for type" },
				enter: { type: "boolean", description: "Submit/press Enter after type" },
				device: { type: "string", enum: ["tablet", "none"], description: "pane viewport emulation (default tablet: 768px responsive layout)" },
				pixels: { type: "string", enum: ["braille", "sextant", "quad", "half"], description: "pane pixel density (default braille: 2x4 px/cell; sextant = 2x3; half = color-true 1x2)" },
				maxText: { type: "number", description: "snapshot body char cap (default 4000)" },
				maxLinks: { type: "number", description: "snapshot link cap (default 40)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any, _signal: any, _onUpdate: any, ctx: any) {
			const tail: string[] = [];
			if (p.maxText != null) tail.push("--max-text", String(p.maxText));
			if (p.maxLinks != null) tail.push("--max-links", String(p.maxLinks));
			if (p.device === "tablet" || p.device === "none") deviceMode = p.device;
			if (p.pixels === "braille" || p.pixels === "sextant" || p.pixels === "quad" || p.pixels === "half") pixelMode = p.pixels;
			switch (p.action) {
				case "activate":
					return { content: [{ type: "text", text: activate(ctx, p.url) }] };
				case "deactivate":
					deactivate();
					return { content: [{ type: "text", text: "sa-browser pane deactivated" }] };
				case "open": {
					if (handle) {
						// Pane repaint carries the navigation; snapshot after it settles.
						await repaint(p.url ?? "");
					} else {
						const nav = runSync(["open", p.url ?? "", ...tail]);
						return { content: [{ type: "text", text: untrusted("WEB PAGE", plain(nav)) }] };
					}
					const snap = runSync(["snapshot", ...tail]);
					return { content: [{ type: "text", text: untrusted("WEB PAGE", plain(snap)) }] };
				}
				case "click": {
					const r = plain(runSync(["click", p.selector ?? ""]));
					if (handle) void repaint();
					return { content: [{ type: "text", text: r }] };
				}
				case "type": {
					const args = ["type", p.selector ?? "", p.text ?? ""];
					if (p.enter) args.push("--enter");
					const r = plain(runSync(args));
					if (handle) void repaint();
					return { content: [{ type: "text", text: r }] };
				}
				case "snapshot":
					return { content: [{ type: "text", text: untrusted("WEB PAGE", plain(runSync(["snapshot", ...tail]))) }] };
				case "status": {
					const s = plain(runSync(["status"]));
					if (handle) void repaint();
					return { content: [{ type: "text", text: s }] };
				}
				default:
					return { content: [{ type: "text", text: plain(runSync(["probe"])) }] };
			}
		},
	});

	// User-side toggle without a model round-trip.
	pi.registerCommand("sab", {
		description: "Toggle/control the sa-browser pane (usage: /sab [url] | /sab click <selector> | /sab type <selector> <text>)",
		handler: async (args: string, ctx: any) => {
			const raw = args.trim();
			if (raw.startsWith("click ")) {
				void runAsync(["click", raw.slice(6).trim()]).then(() => repaint());
				return;
			}
			if (raw.startsWith("type ")) {
				const rest = raw.slice(5).trim();
				const [selector = "", ...textParts] = rest.split(/\s+/);
				void runAsync(["type", selector, textParts.join(" ")]).then(() => repaint());
				return;
			}
			const url = raw || undefined;
			if (handle && !url) deactivate();
			else activate(ctx, url);
		},
	});

	pi.on("session_shutdown", async () => deactivate());
}
