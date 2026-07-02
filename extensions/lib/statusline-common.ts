import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
export const BIN = (name: string) => join(ROOT, "target", "release", name);
export const TASKS_DB = join(ROOT, "data", "tasks.semdb");
export const WORKFLOW_DB = join(ROOT, "data", "workflow.semdb");

export type StatusLine = "workspace" | "data" | "infra";
export type StatusSegment = { key: string; args: string[]; line: StatusLine };

export const STATUS_SEGMENTS: StatusSegment[] = [
	{ key: "codegraph", args: ["statusline", join(ROOT, "data", "codegraph.json")], line: "workspace" },
	{ key: "codeindex", args: ["statusline"], line: "workspace" },
	{ key: "tasks", args: ["statusline", "--db", TASKS_DB], line: "workspace" },
	{ key: "workflow", args: ["statusline", "--root", ROOT, "--db", WORKFLOW_DB], line: "workspace" },
	{ key: "memory", args: ["statusline", "--dir", join(ROOT, "data", "memory")], line: "data" },
	{ key: "rag", args: ["statusline", join(ROOT, "data", "rag.semdb")], line: "data" },
	{ key: "schedule", args: ["statusline"], line: "data" },
	{ key: "evals", args: ["statusline", "--db", join(ROOT, "data", "evals.semdb")], line: "data" },
	{ key: "orchestrate", args: ["statusline"], line: "data" },
	{ key: "gateway", args: ["statusline"], line: "workspace" },
	{ key: "supervise", args: ["statusline"], line: "infra" },
	{ key: "sandbox", args: ["statusline"], line: "infra" },
	{ key: "secrets", args: ["statusline", "--store", join(ROOT, "data", "secrets")], line: "infra" },
	{ key: "browser", args: ["statusline"], line: "infra" },
	{ key: "search", args: ["statusline"], line: "infra" },
	{ key: "hooks", args: ["statusline", "--root", ROOT], line: "infra" },
];

export function parseLevel(raw: string): { level: string; text: string } {
	const cut = raw.indexOf("|");
	return cut > 0 ? { level: raw.slice(0, cut), text: raw.slice(cut + 1) } : { level: "err", text: raw };
}

export const statusTag = (level: string) => (level === "ok" ? "✓" : level === "warn" ? "⚠" : "✗");

// Live layout state shared between extensions in-process: overlays that
// reserve horizontal space (the agentpanel sidebar) register their width here
// so width-aware widgets (statusline) lay out for the REMAINING columns
// instead of the full terminal.
export const layout = { sidebarCols: 0 };
export const availableColumns = (): number => Math.max(40, (process.stdout.columns || 120) - layout.sidebarCols);
