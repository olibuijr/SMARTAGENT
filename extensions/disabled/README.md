# Disabled extensions

Extensions here are NOT loaded by `./pi` (the launcher globs only
`extensions/*.ts`, non-recursively). Their crates still build and ship — the
tool is just not advertised to the agent because a runtime dependency is
missing.

## voice.ts

The `voice` crate (STT/TTS bridge) is complete and tested, but titan has no
speech server deployed (`voice_stt_url`/`voice_tts_url` in
`config/smartagent.conf` point at the embedder, which has no audio endpoints).
Advertising it would tell the agent it can hear/speak when it can't.

**Re-enable:** deploy an OpenAI-compatible STT (e.g. whisper.cpp server) and TTS
(e.g. piper) on titan, point the two `voice_*` URLs at them, then
`git mv extensions/disabled/voice.ts extensions/voice.ts` and bump the smoke
gate back to 19 crate tools in `build.sh`.
