/**
 * secrets — pi extension glue over the pure-Rust policy-gated secret store.
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "secrets");
const STORE = join(ROOT, "data", "secrets");

function run(args: string[]): string {
	try {
		return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim();
	} catch (e: any) {
		return `error: ${e.stderr?.toString().trim() || e.message}`;
	}
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "secrets",
		label: "Secrets",
		description:
			"Policy-gated, audited secret store (deny by default). Actions: 'set' stores a secret, " +
			"'get' reads one as a caller (policy-checked + audited), 'list' shows names, " +
			"'audit' shows the access log, 'policy-allow' grants a caller access to a name (* = all). " +
			"The agent's caller identity is 'pi'.",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["set", "get", "list", "audit", "policy-allow"] },
				name: { type: "string", description: "secret name (set/get/policy-allow)" },
				value: { type: "string", description: "secret value (set)" },
				caller: { type: "string", description: "caller identity (get default 'pi'; policy-allow required)" },
			},
			required: ["action"],
		} as any,

		async execute(_id: string, p: any) {
			const base = [p.action, "--store", STORE];
			const out =
				p.action === "set"
					? run([...base, "--name", p.name ?? "", "--value", p.value ?? ""])
					: p.action === "get"
						? run([...base, "--name", p.name ?? "", "--as", p.caller ?? "pi"])
						: p.action === "policy-allow"
							? run([...base, "--caller", p.caller ?? "", "--name", p.name ?? ""])
							: run(base);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
