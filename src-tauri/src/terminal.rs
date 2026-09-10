use crate::models::TerminalBinding;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("tmux is unavailable")]
    TmuxUnavailable,
    #[error("terminal command failed: {0}")]
    Command(String),
    #[error("terminal binding could not be verified")]
    Unverified,
    #[error("terminal integration is unsupported on this platform")]
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct TmuxTarget {
    pub session: String,
    pub pane: String,
}

pub fn safe_session_name(session_id: &str) -> String {
    let compact = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(24)
        .collect::<String>();
    format!("vw-{}", if compact.is_empty() { "session" } else { &compact })
}

pub async fn tmux_create(session_id: &str, cwd: &str, argv: &[String]) -> Result<TmuxTarget, TerminalError> {
    tmux_create_with_env(session_id, cwd, argv, &[]).await
}

pub async fn tmux_create_with_env(
    session_id: &str,
    cwd: &str,
    argv: &[String],
    env: &[(&str, &str)],
) -> Result<TmuxTarget, TerminalError> {
    if !Path::new("/opt/homebrew/bin/tmux").exists()
        && !Path::new("/usr/bin/tmux").exists()
        && which("tmux").await.is_err()
    {
        return Err(TerminalError::TmuxUnavailable);
    }
    if argv.is_empty() {
        return Err(TerminalError::Command("empty process argv".into()));
    }
    let name = safe_session_name(session_id);
    let mut command = Command::new("tmux");
    command.args(["new-session", "-d", "-s", &name, "-c", cwd]);
    for (key, value) in env {
        command.args(["-e", &format!("{key}={value}")]);
    }
    command.arg("--").args(argv);
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let output = command
        .output()
        .await
        .map_err(|e| TerminalError::Command(e.to_string()))?;
    if !output.status.success() {
        return Err(TerminalError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let _ = Command::new("tmux")
        .args(["set-option", "-t", &name, "set-titles", "on"])
        .output()
        .await;
    let title = format!("Vibe Working: {name}");
    let _ = Command::new("tmux")
        .args(["set-option", "-t", &name, "set-titles-string", &title])
        .output()
        .await;
    let pane = format!("{name}:0.0");
    Ok(TmuxTarget { session: name, pane })
}

pub async fn tmux_exists(target: &TmuxTarget) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", &target.session])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// Deliberately absent: send-keys / capture-pane helpers. Typing into a pane
// cannot be confirmed to have reached the agent, so no automated path may use
// it — the prompt is delivered as process argv and continuations go over the
// app-server RPC channel only.

pub async fn verify_binding(binding: &TerminalBinding) -> Result<bool, TerminalError> {
    if binding.target_kind != "tmux" {
        return Err(TerminalError::Unsupported);
    }
    let target = TmuxTarget {
        session: binding.target_value.clone(),
        pane: format!("{}:0.0", binding.target_value),
    };
    if !tmux_exists(&target).await {
        return Ok(false);
    }
    let (_, identity) = terminal_identity(&target).await?;
    Ok(binding.command_fingerprint.as_deref() == Some(identity.as_str()))
}

pub async fn terminal_identity(target: &TmuxTarget) -> Result<(i64,String), TerminalError> {
    let output = Command::new("tmux").args(["display-message","-p","-t",&target.pane,"#{pane_pid}|#{session_id}|#{pane_id}|#{pane_start_command}|#{pane_dead}"]).output().await.map_err(|error| TerminalError::Command(error.to_string()))?;
    let identity = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || identity.ends_with("|1") { return Err(TerminalError::Unverified); }
    let pid = identity.split('|').next().and_then(|value| value.parse::<i64>().ok()).ok_or(TerminalError::Unverified)?;
    let started = Command::new("/bin/ps").args(["-p",&pid.to_string(),"-o","lstart="]).output().await.map_err(|error| TerminalError::Command(error.to_string()))?;
    if !started.status.success() { return Err(TerminalError::Unverified); }
    Ok((pid,fingerprint(&format!("{identity}|{}",String::from_utf8_lossy(&started.stdout).trim()))))
}

pub fn fingerprint(command: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.as_bytes());
    hex::encode(hasher.finalize())
}

pub async fn focus_terminal(target: &TmuxTarget) -> Result<(), TerminalError> {
    if !tmux_exists(target).await {
        return Err(TerminalError::Unverified);
    }
    #[cfg(target_os = "macos")]
    {
        let title = format!("Vibe Working: {}", target.session);
        let script = r#"
on run argv
  set targetTitle to item 1 of argv
  set attachCommand to item 2 of argv
  tell application "Terminal"
    activate
    set foundTab to false
    repeat with w in windows
      repeat with t in tabs of w
        try
          if (custom title of t) is targetTitle then
            set selected tab of w to t
            set index of w to 1
            set foundTab to true
            exit repeat
          end if
        end try
      end repeat
      if foundTab then exit repeat
    end repeat
    if not foundTab then
      set newTab to do script attachCommand
      delay 0.4
      try
        set custom title of newTab to targetTitle
      end try
    end if
  end tell
end run
"#;
        let attach = format!("tmux attach-session -t {}", shell_quote(&target.session));
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", script, &title, &attach])
            .output()
            .await
            .map_err(|e| TerminalError::Command(e.to_string()))?;
        if !output.status.success() {
            return Err(TerminalError::Command(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        Err(TerminalError::Unsupported)
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

async fn which(program: &str) -> Result<(), ()> {
    Command::new("/usr/bin/which")
        .arg(program)
        .output()
        .await
        .map_err(|_| ())
        .and_then(|o| if o.status.success() { Ok(()) } else { Err(()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_names_are_safe_and_stable() {
        assert_eq!(safe_session_name("abc-123"), "vw-abc123");
        assert!(!safe_session_name("x' ; rm -rf /").contains('\''));
    }
    #[test]
    fn fingerprint_is_deterministic() {
        assert_eq!(fingerprint("claude"), fingerprint("claude"));
    }
}
