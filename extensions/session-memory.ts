/**
 * session-memory — gives the agent continuity across pi sessions.
 *
 * On shutdown it stores the session's opening intent (the first user message)
 * as an episodic memory, so the next session can recall "last time you were
 * working on X". Recall-at-start is done by the ./pi launcher (it injects
 * `memory recent` into the initial context), which is safer than rewriting the
 * live message stream from a context hook.
 *
 * Type-only pi imports + node builtins only (runtime imports fail silently).
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "memory");
const MEM_DIR = join(ROOT, "data", "memory");

function textOf(msg: any): string {
	if (!msg) return "";
	const c = msg.content;
	if (typeof c === "string") return c;
	if (Array.isArray(c)) {
		return c.map((b: any) => (typeof b === "string" ? b : b?.text ?? "")).join(" ").trim();
	}
	return "";
}

export default function (pi: ExtensionAPI) {
	let firstUserIntent = "";

	// Capture the first user message of the session as its intent.
	pi.on("message_end", (e: any) => {
		if (firstUserIntent) return;
		if (e?.message?.role === "user") {
			const t = textOf(e.message).slice(0, 240);
			if (t) firstUserIntent = t;
		}
	});

	// On shutdown, persist that intent to episodic memory (fire-and-forget;
	// never throw out of a shutdown handler).
	pi.on("session_shutdown", () => {
		if (!firstUserIntent) return;
		try {
			execFileSync(BIN, ["remember", "--dir", MEM_DIR, "--tier", "episodic", "--text", `Session intent: ${firstUserIntent.slice(0, 280)}`], {
				encoding: "utf8",
				timeout: 15_000,
				cwd: ROOT,
			});
		} catch {
			// A failed embed/store must not block shutdown.
		}
	});
}
