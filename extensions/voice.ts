/** voice — pi extension over the pure-Rust STT/TTS bridge binary. */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, "target", "release", "voice");

function run(args: string[]): string {
	try { return execFileSync(BIN, args, { encoding: "utf8", timeout: 120_000, cwd: ROOT }).trim(); }
	catch (e: any) { return `error: ${e.stderr?.toString().trim() || e.message}`; }
}

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "voice",
		label: "Voice",
		description: "Speech bridge (Pipecat concept). 'stt' transcribes a WAV file to text; 'tts' synthesizes text to an audio file; 'probe' shows configured endpoints. Models are external (OpenAI-compatible).",
		parameters: {
			type: "object",
			properties: {
				action: { type: "string", enum: ["stt", "tts", "probe"] },
				file: { type: "string", description: "input WAV path (stt)" },
				text: { type: "string", description: "text to speak (tts)" },
				out: { type: "string", description: "output audio path (tts)" },
				voice: { type: "string", description: "voice name (tts)" },
			},
			required: ["action"],
		} as any,
		async execute(_id: string, p: any) {
			const out = p.action === "stt"
				? run(["stt", "--file", p.file ?? ""])
				: p.action === "tts"
					? run(["tts", "--text", p.text ?? "", "--out", p.out ?? "out.mp3", ...(p.voice ? ["--voice", p.voice] : [])])
					: run(["probe"]);
			return { content: [{ type: "text", text: out }] };
		},
	});
}
