use serde_json::{json, Value};
use std::process::Command;

pub fn snapshot() -> Value {
    json!({
        "platform": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "codex": command_version("codex", &["--version"]),
        "claude": command_version("claude", &["--version"]),
        "tmux": command_version("tmux", &["-V"]),
        "terminal_app": cfg!(target_os = "macos"),
        "iTerm2": cfg!(target_os = "macos") && std::path::Path::new("/Applications/iTerm.app").exists()
    })
}

fn command_version(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program).args(args).output().ok().and_then(|output| {
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    })
}
