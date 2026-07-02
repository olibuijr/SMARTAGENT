//! voice CLI: stt / tts / probe

use httpc::args::flag;
use std::process::ExitCode;
use voice::api;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => { println!("{out}"); ExitCode::SUCCESS }
        Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
    }
}

fn cfg(key: &str, env: &str, flag: Option<&str>) -> Option<String> {
    semdb::config::Config::load().resolve(key, env, flag)
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("stt") => {
            let file = flag(args, "--file").ok_or("--file audio.wav required")?;
            let url = cfg("voice_stt_url", "VOICE_STT_URL", flag(args, "--url").as_deref())
                .ok_or("no voice_stt_url in config/smartagent.conf")?;
            let model = flag(args, "--model").unwrap_or_else(|| "whisper-1".into());
            let wav = std::fs::read(&file).map_err(|e| format!("read {file}: {e}"))?;
            api::transcribe(&url, &model, &wav)
        }
        Some("tts") => {
            let text = flag(args, "--text").ok_or("--text required")?;
            let out = flag(args, "--out").ok_or("--out FILE required")?;
            let url = cfg("voice_tts_url", "VOICE_TTS_URL", flag(args, "--url").as_deref())
                .ok_or("no voice_tts_url in config/smartagent.conf")?;
            let model = flag(args, "--model").unwrap_or_else(|| "tts-1".into());
            let voice = flag(args, "--voice").unwrap_or_else(|| "alloy".into());
            let audio = api::synthesize(&url, &model, &voice, &text)?;
            std::fs::write(&out, &audio).map_err(|e| e.to_string())?;
            Ok(format!("wrote {} bytes → {out}", audio.len()))
        }
        Some("probe") => {
            let stt = cfg("voice_stt_url", "VOICE_STT_URL", None).unwrap_or_else(|| "(unset)".into());
            let tts = cfg("voice_tts_url", "VOICE_TTS_URL", None).unwrap_or_else(|| "(unset)".into());
            Ok(format!("stt: {stt}\ntts: {tts}"))
        }
        _ => Ok(HELP.trim().into()),
    }
}


const HELP: &str = r#"
voice — STT/TTS bridge (Pipecat concept, external models)

USAGE:
  voice stt   --file audio.wav [--url U] [--model whisper-1]
  voice tts   --text '...' --out out.mp3 [--url U] [--voice alloy] [--model tts-1]
  voice probe

Endpoints from config/smartagent.conf (voice_stt_url, voice_tts_url).
"#;
