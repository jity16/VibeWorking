//! Recovers the user's real `PATH` for a GUI process.
//!
//! A bundle launched from Finder inherits `/usr/bin:/bin:/usr/sbin:/sbin` —
//! launchd does not read shell configuration. Every agent this app drives lives
//! outside that set (`codex` and `tmux` under Homebrew, `claude` under
//! `~/.local/bin`), so without this the packaged app cannot start anything,
//! while `cargo test` and `pnpm tauri dev` work fine because they inherit a
//! shell. Hard-coding directories is not enough: the locations are per-user.

use std::time::Duration;

/// Wraps the value so a noisy rc file printing to stdout cannot be mistaken for
/// the answer.
const MARKER: &str = "__VIBE_WORKING_PATH__";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Directories worth trying when the shell probe fails or is unavailable. Used
/// as a supplement, never as a replacement for what the shell reports.
fn fallback_directories() -> Vec<String> {
    let mut directories = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ];
    if let Some(home) = dirs::home_dir() {
        for relative in [".local/bin", ".bun/bin", ".cargo/bin", ".volta/bin"] {
            directories.push(home.join(relative).to_string_lossy().to_string());
        }
    }
    directories
}

/// Extends this process's `PATH` with the login shell's, so spawned agents and
/// every child they start inherit it too.
pub fn install_login_path() {
    let current = std::env::var("PATH").unwrap_or_default();
    let discovered = login_shell_path().unwrap_or_default();
    let merged = merge_paths(&current, &discovered, &fallback_directories());
    if merged != current {
        std::env::set_var("PATH", &merged);
    }
}

/// Runs the login shell and asks it for its `PATH`. Interactive (`-i`) as well
/// as login (`-l`), because `zsh` puts `PATH` edits in `.zshrc` as often as in
/// `.zprofile`. Bounded: a shell that hangs must not hang app startup.
fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let script = format!("printf '{MARKER}%s{MARKER}' \"$PATH\"");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let output = std::process::Command::new(&shell)
            .args(["-ilc", &script])
            .stdin(std::process::Stdio::null())
            .output();
        let _ = sender.send(output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).to_string()));
    });
    // The thread is detached: on timeout the probe result is simply ignored.
    receiver.recv_timeout(PROBE_TIMEOUT).ok().flatten().and_then(|text| extract(&text))
}

fn extract(text: &str) -> Option<String> {
    let start = text.find(MARKER)? + MARKER.len();
    let rest = &text[start..];
    let end = rest.find(MARKER)?;
    let value = rest[..end].trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Existing entries keep their order and priority; anything new is appended.
/// Nothing is ever removed, so a deliberately restricted `PATH` is respected.
fn merge_paths(current: &str, discovered: &str, fallbacks: &[String]) -> String {
    let mut ordered: Vec<String> = Vec::new();
    let mut push = |entry: &str| {
        let entry = entry.trim();
        if entry.is_empty() || ordered.iter().any(|existing| existing == entry) {
            return;
        }
        ordered.push(entry.to_string());
    };
    for entry in current.split(':') {
        push(entry);
    }
    for entry in discovered.split(':') {
        push(entry);
    }
    for entry in fallbacks {
        if std::path::Path::new(entry).is_dir() {
            push(entry);
        }
    }
    ordered.join(":")
}

/// Absolute path of `program` on the current `PATH`, for reporting where a tool
/// was found and for failing with something more useful than "os error 2".
pub fn resolve(program: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.to_string_lossy().to_string())
}

pub fn missing_program(program: &str) -> String {
    format!("找不到可执行文件 `{program}`。它不在应用启动时可见的 PATH 中：{}", std::env::var("PATH").unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_value_survives_a_noisy_shell() {
        let noisy = format!("welcome banner\n{MARKER}/a:/b{MARKER}\nmotd trailer");
        assert_eq!(extract(&noisy).unwrap(), "/a:/b");
    }

    #[test]
    fn output_without_the_marker_is_rejected_rather_than_guessed() {
        assert!(extract("/usr/bin:/bin").is_none());
        assert!(extract(&format!("{MARKER}   {MARKER}")).is_none());
    }

    #[test]
    fn merging_appends_new_entries_and_keeps_existing_priority() {
        let merged = merge_paths("/usr/bin:/bin", "/opt/homebrew/bin:/usr/bin", &[]);
        assert_eq!(merged, "/usr/bin:/bin:/opt/homebrew/bin");
    }

    #[test]
    fn merging_never_drops_an_entry_the_process_already_had() {
        let merged = merge_paths("/restricted", "", &[]);
        assert!(merged.split(':').any(|entry| entry == "/restricted"));
    }

    #[test]
    fn fallbacks_are_only_added_when_they_exist() {
        let merged = merge_paths("/usr/bin", "", &["/definitely/not/here".into(), "/usr".into()]);
        assert!(!merged.contains("/definitely/not/here"));
        assert!(merged.split(':').any(|entry| entry == "/usr"));
    }
}
