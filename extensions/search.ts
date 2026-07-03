/** search — pi extension over the pure-Rust SearXNG metasearch client binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { bin, makeRunner, untrusted } from "./lib/common.ts";

const run = makeRunner(
	bin("search"),
	60_000,
	/unreachable|refused|connect|required/i,
	"[hint: searxng may be down or unset — call supervise status/up, or check searx_instance]",
);

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "search",
		label: "Web Search",
		description: "Web search via a self-hosted SearXNG metasearch instance. Actions: 'query' returns ranked title/url/snippet results for terms (optionally filtered by engines or category); 'health' checks the instance is reachable. Use to find current information on the web.",
		parameters: {
			type: "object",
			properties: {
				pageno: { type: "number", description: "results page (1-based)" },
				action: { type: "string", enum: ["query", "health"], description: "Operation to perform" },
				terms: { type: "string", description: "search terms (query action)" },
				k: { type: "number", description: "max results (default 5)" },
				timeRange: { type: "string", enum: ["day","week","month","year"], description: "restrict to a recency window" },
				site: { type: "string", description: "restrict to one domain (site:)" },
				urlsOnly: { type: "boolean", description: "title+url only, skip snippets" },
				snippetChars: { type: "number", description: "cap each snippet (default 160)" },
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
				const args = ["query", p.terms ?? "", ...inst, "--k", String(p.k ?? 5)];
				if (p.engines) args.push("--engines", p.engines);
					if (p.timeRange) args.push("--time-range", p.timeRange);
					if (p.site) args.push("--site", p.site);
					if (p.urlsOnly) args.push("--urls-only");
					if (p.snippetChars != null) args.push("--snippet-chars", String(p.snippetChars));
				if (p.category) args.push("--category", p.category);
				if (p.pageno != null && p.pageno > 1) args.push("--pageno", String(p.pageno));
				out = untrusted("WEB SEARCH RESULTS", run(args));
			}
			return { content: [{ type: "text", text: out }] };
		},
	});
}
