use serde_json::{json, Value};
use std::process::Command;

pub fn snapshot() -> Value {
    json!({
        "platform": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "codex": command_version("codex", &["--version"]),
        "claude": command_version("claude", &["--version"]),
        "tmux": command_version("tmux", &["-V"]),
        // Where each tool was found, and on what PATH: a Finder-launched bundle
        // starts with a minimal one, and this is how to see whether the login
        // shell probe recovered it.
        "resolved": json!({
            "codex": crate::environment::resolve("codex"),
            "claude": crate::environment::resolve("claude"),
            "tmux": crate::environment::resolve("tmux")
        }),
        "path": std::env::var("PATH").unwrap_or_default(),
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
