//! Tiny demo binary: `httpc <get|post> <url> [json-body]`

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match (args.first().map(String::as_str), args.get(1)) {
        (Some("get"), Some(url)) => httpc::get(url).map(|r| {
            format!("{}\n{}", r.status, r.text().unwrap_or_default())
        }),
        (Some("post"), Some(url)) => {
            let body = args.get(2).cloned().unwrap_or_else(|| "{}".into());
            httpc::post_json(url, &body).map(|v| format!("{v:?}"))
        }
        _ => Err("usage: httpc <get|post> <url> [json-body]".into()),
    };
    match result {
        Ok(out) => {
            println!("{out}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
