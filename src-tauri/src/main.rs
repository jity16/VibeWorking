use std::io::{Read, Write};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) == Some("hook") {
        run_hook(&args);
        return;
    }
    vibe_working_lib::run();
}

fn run_hook(args: &[String]) {
    let Some(session_id) = args.get(3) else { return };
    let Some(event) = args.get(5) else { return };
    let Some(expected) = std::env::var_os("VIBE_WORKING_SESSION_ID") else { return };
    if expected != session_id.as_str() {
        return;
    }
    let Some(path) = std::env::var_os("VIBE_WORKING_HOOK_FILE") else { return };
    let mut input = Vec::new();
    if std::io::stdin().take(512 * 1024).read_to_end(&mut input).is_err() {
        return;
    }
    let payload = serde_json::from_slice::<serde_json::Value>(&input).unwrap_or_else(|_| {
        serde_json::Value::String(String::from_utf8_lossy(&input).to_string())
    });
    let record = serde_json::json!({
        "event_id": uuid::Uuid::new_v4().to_string(),
        "session_id": session_id,
        "hook": event,
        "payload": payload
    });
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}", record);
        let _ = file.flush();
    }
}
