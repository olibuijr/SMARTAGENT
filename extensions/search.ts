/** search — pi extension over the pure-Rust SearXNG metasearch client binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "search");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 60_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "search",
		label: "Web Search",
		description: "Web search via a self-hosted SearXNG metasearch instance. Actions: 'query' returns ranked title/url/snippet results for terms (optionally filtered by engines or category); 'health' checks the instance is reachable. Use to find current information on the web.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["query", "health"], description: "Operation to perform" },
				terms: { type: "string", description: "search terms (query action)" },
				k: { type: "number", description: "max results (default 10)" },
				engines: { type: "string", description: "comma-separated engines to restrict to" },
				category: { type: "string", description: "category filter: general|news|it" },
				instance: { type: "string", description: "SearXNG instance URL (default from SEARX_INSTANCE env)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			// Instance resolution (flag → env → config) happens in the binary.
			const inst = p.instance ? ["--instance", p.instance] : [];
			let out: string;
			if (p.action === "health") {
				out = run(["health", ...inst]);
			} else {
				const args = ["query", p.terms ?? "", ...inst, "--k", String(p.k ?? 10)];
				if (p.engines) args.push("--engines", p.engines);
				if (p.category) args.push("--category", p.category);
				out = run(args);
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
