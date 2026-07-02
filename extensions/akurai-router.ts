// AkurAI Router — dynamic model discovery for pi.
// Registers the `akurai-router` provider by fetching the live model list from
// https://akurai-router.olibuijr.com/v1/models on startup, so new models the
// router exposes appear in pi automatically (no manual model list to maintain).
//
// Routing quirk handled here: codex/* models speak the OpenAI *Responses* API
// (/v1/responses); claude/* and opencode-go/* speak Chat Completions
// (/v1/chat/completions). The per-model `api` field encodes that.
//
// The API key lives in ~/.pi/agent/akurai-router.key (chmod 600), never in this
// file. Discovery reads it directly; request-time auth resolves it via `!cat`.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const BASE_URL = "https://akurai-router.olibuijr.com/v1";
const KEY_FILE = process.env.PI_CODING_AGENT_DIR
	? join(process.env.PI_CODING_AGENT_DIR, "akurai-router.key")
	: join(homedir(), ".pi", "agent", "akurai-router.key");

type RouterModel = { id: string };
type RouterModelList = { data: RouterModel[] };

// Per-model context windows for opencode-go upstreams (from pi's opencode-go.models.ts).
// The router dynamically discovers models but doesn't report context windows,
// so we maintain this lookup keyed by the model name after the "opencode-go/" prefix.
// Fallback: 256K for unknown opencode-go models.
const OPENCODE_CONTEXT_WINDOWS: Record<string, number> = {
  "deepseek-v4-flash": 1_000_000,
  "deepseek-v4-pro": 1_000_000,
  "glm-5.1": 202_752,
  "glm-5.2": 1_000_000,
  "kimi-k2.6": 262_144,
  "kimi-k2.7-code": 262_144,
  "mimo-v2.5": 1_000_000,
  "mimo-v2.5-pro": 1_048_576,
  "minimax-m2.7": 204_800,
  "minimax-m3": 1_000_000,
  "qwen3.6-plus": 1_000_000,
  "qwen3.7-max": 1_000_000,
  "qwen3.7-plus": 1_000_000,
};

export default async function (pi: ExtensionAPI) {
  // Respect offline mode — don't block startup on a network call.
  if (process.env.PI_OFFLINE) return;

  let apiKey = "";
  try {
    apiKey = readFileSync(KEY_FILE, "utf8").trim();
  } catch {
    return; // no key file -> nothing to register
  }
  if (!apiKey) return;

  let ids: string[] = [];
  try {
    const res = await fetch(`${BASE_URL}/models`, {
      headers: { Authorization: `Bearer ${apiKey}` },
    });
    if (!res.ok) return; // fail open: skip rather than break pi startup
    const payload = (await res.json()) as RouterModelList;
    ids = (payload.data ?? []).map((m) => m.id).filter(Boolean);
  } catch {
    return; // router unreachable -> skip silently
  }

  const models = ids
    .filter((id) => !id.startsWith("embeddings/")) // chat models only
    .map((id) => {
      const isCodex = id.startsWith("codex/");
      const isClaude = id.startsWith("claude/");
      // opencode-go fronts non-OpenAI upstreams (deepseek/glm/kimi/minimax/qwen/mimo)
      // that reject the OpenAI `developer` role and `reasoning_effort` param.
      const isOpencode = id.startsWith("opencode-go/");
      return {
        id,
        name: id,
        // codex routes through the Responses API; everything else Chat Completions.
        api: isCodex ? "openai-responses" : "openai-completions",
        // codex + claude are thinking models; opencode-go upstreams use non-OpenAI
        // thinking formats, so advertise them as plain chat to stay compatible.
        reasoning: !isOpencode,
        input: (isCodex || isClaude ? ["text", "image"] : ["text"]) as (
          | "text"
          | "image"
        )[],
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        contextWindow: isCodex
          ? 400000
          : isClaude
            ? 200000
            : isOpencode
              ? (OPENCODE_CONTEXT_WINDOWS[id.slice("opencode-go/".length)] ?? 256000)
              : 256000,
        maxTokens: 32000,
        // Chat-Completions upstreams here only accept the `system` role, not
        // OpenAI's `developer` role. codex (Responses API) handles it natively.
        ...(isCodex
          ? {}
          : {
              compat: {
                supportsDeveloperRole: false,
                ...(isOpencode ? { supportsReasoningEffort: false } : {}),
              },
            }),
      };
    });

  if (models.length === 0) return;

  pi.registerProvider("akurai-router", {
    name: "AkurAI Router",
    baseUrl: BASE_URL,
    // Resolve the key from the local 600 file at request time; never inline it.
    apiKey: `!cat ${KEY_FILE}`,
    api: "openai-completions",
    models,
  });
}
