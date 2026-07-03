/**
 * secrets — pi extension glue over the pure-Rust policy-gated secret store.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";
import { bin, makeRunner, ROOT } from "./lib/common.ts";

const STORE = join(ROOT, "data", "secrets");

const run = makeRunner(bin("secrets"), 120000);

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "secrets",
		label: "Secrets",
		description:
			"Policy-gated, audited secret store (deny by default). Actions: 'set' stores a secret, " +
			"'get' reads one as caller 'pi' (policy-checked + audited), 'list' shows names, " +
			"'audit' shows the access log. Granting access (policy-allow) is admin-only and " +
			"deliberately NOT available here — a human must run it out-of-band.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["set", "get", "list", "audit"] },
				name: { type: "string", description: "secret name (set/get)" },
				value: { type: "string", description: "secret value (set)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const base = [p.action, "--store", STORE];
			const out =
				p.action === "set"
					? run([...base, "--name", p.name ?? "", "--value", p.value ?? ""])
					: p.action === "get"
						? run([...base, "--name", p.name ?? "", "--as", "pi"])
						: run(base);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
