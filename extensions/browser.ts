/** browser — pi extension over the pure-Rust CDP client (Browser Use port). */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { bin, makeRunner, untrusted } from "./lib/common.ts";

const run = makeRunner(
	bin("browser"),
	60_000,
	/unreachable|refused|connect/i,
	"[hint: chromium may be down — call supervise (status, then up)]",
);

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "browser",
		label: "Browser",
		description: "Drive a real Chrome over the DevTools Protocol (Browser Use port). Actions: 'open' navigates to a URL; 'click' clicks a CSS selector; 'type' fills an input (selector + text); 'back' goes back; each returns a fresh compact snapshot (title, visible text, links). 'probe' checks the DevTools connection. Requires Chrome started with --remote-debugging-port=9222.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["open", "click", "type", "back", "wait", "scroll", "attr", "probe"] },
				url: { type: "string", description: "URL to open (open action)" },
				selector: { type: "string", description: "CSS selector (click/type actions)" },
				text: { type: "string", description: "text to type (type action)" },
				enter: { type: "boolean", description: "press Enter after typing (submit forms)" },
				name: { type: "string", description: "attribute name (or text|value) for attr" },
				timeoutMs: { type: "number", description: "wait timeout ms (default 10000)" },
				by: { type: "number", description: "scroll by px (scroll; omit to scroll to bottom)" },
				to: { type: "string", description: "scroll to this selector (scroll)" },
				quiet: { type: "boolean", description: "click/type: return only status, no page snapshot (cheap for intermediate steps)" },
				maxText: { type: "number", description: "snapshot body char cap (default 4000)" },
				maxLinks: { type: "number", description: "snapshot link cap (default 40)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const tail: string[] = [];
			if (p.quiet) tail.push("--quiet");
			if (p.maxText != null) tail.push("--max-text", String(p.maxText));
			if (p.maxLinks != null) tail.push("--max-links", String(p.maxLinks));
			let out: string;
			switch (p.action) {
				case "open": out = run(["open", p.url ?? "", ...tail]); break;
				case "click": out = run(["click", p.selector ?? "", ...tail]); break;
				case "type": out = run(["type", p.selector ?? "", p.text ?? "", ...(p.enter ? ["--enter"] : []), ...tail]); break;
				case "back": out = run(["back", ...tail]); break;
				case "wait": out = run(["wait", p.selector ?? "", "--timeout-ms", String(p.timeoutMs ?? 10000), ...tail]); break;
				case "scroll": out = run(["scroll", ...(p.to ? ["--to", p.to] : []), ...(p.by != null ? ["--by", String(p.by)] : []), ...tail]); break;
				case "attr": return { content: [{ type: "text", text: run(["attr", p.selector ?? "", p.name ?? "text"]) }] };
				default: return { content: [{ type: "text", text: run(["probe"]) }] };
			}
			return { content: [{ type: "text", text: untrusted("WEB PAGE", out) }] };
		},
	});
}
