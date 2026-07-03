/** orchestrate — pi extension over the pure-Rust subagent fan-out binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { bin, makeRunner } from "./lib/common.ts";


const run = makeRunner(bin("orchestrate"), 30000);

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "orchestrate",
		label: "Orchestrate",
		description: "Fan out N parallel headless pi subagents (LangGraph send/supervisor). 'run' spawns N agents from a prompt template ({i} placeholder); 'list' shows past runs. Each agent runs in its own workspace; returns a summary of exit codes and timings.",
		parameters: {
			type: "object",
			properties: {
				max_parallel: { type: "number", description: "concurrent agent cap per wave (default 4)" },
				retries: { type: "number", description: "re-run failed/timed-out agents up to N times" },
				action: { type: "string", enum: ["run", "list", "out"] },
				agents: { type: "number", description: "number of parallel agents (run)" },
				prompt: { type: "string", description: "prompt template, {i} substituted per agent (run)" },
				timeout: { type: "number", description: "per-agent timeout seconds (default 300)" },
				runId: { type: "string", description: "run id to collect output from (out)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const out = p.action === "run"
				? run(["run", "--agents", String(p.agents ?? 1), "--prompt", p.prompt ?? "", "--timeout", String(p.timeout ?? 300),
					"--max-parallel", String(p.max_parallel ?? 4), ...(p.retries != null ? ["--retries", String(p.retries)] : [])])
				: p.action === "out"
					? run(["out", p.runId ?? ""])
					: run(["list"]);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
