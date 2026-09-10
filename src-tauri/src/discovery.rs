//! Read-only discovery of Codex and Claude Code sessions that this app did not
//! start.
//!
//! Discovered rows are deliberately kept out of `agent_sessions`. A row in that
//! table means "a run this app owns", and carries control mode, attention and
//! retry budget that only make sense for something we launched. A session found
//! on disk has none of that: we cannot prove which terminal it belongs to, we
//! cannot stop it, and `thread/list` reports stored threads as `notLoaded`, so
//! we do not know whether it is still running. It is history, and is presented
//! as history.

use crate::db::Database;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::time::{sleep, timeout, Duration};
use tokio_tungstenite::{client_async, tungstenite::Message};

/// The server caps a page well below whatever `limit` asks for, so reaching a
/// useful depth means following `nextCursor`. Bounded: a machine here already
/// holds over a thousand rollouts and nobody scrolls that far.
const CODEX_PAGE: usize = 100;
const CODEX_PAGES: usize = 3;
const CODEX_DEADLINE: Duration = Duration::from_secs(20);
/// Enough lines to reach the first record carrying `cwd` without reading whole
/// transcripts, which run to megabytes.
const CLAUDE_HEAD_LINES: usize = 60;

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredSession {
    pub provider: String,
    pub session_id: String,
    pub cwd: String,
    pub title: String,
    pub preview: String,
    pub updated_at: String,
    /// `None` means no local project owns this directory — the UI files those
    /// under 未分类 rather than inventing a project for them.
    pub project_id: Option<String>,
    pub project_name: Option<String>,
}

/// Errors from one provider never hide the other provider's results: a missing
/// `codex` binary should not blank out the Claude list.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryReport {
    pub sessions: Vec<DiscoveredSession>,
    pub warnings: Vec<String>,
    /// True when Codex had more stored threads than the page budget allowed, so
    /// the list is the most recent ones rather than everything.
    pub truncated: bool,
}

pub async fn discover(db: &Database) -> Result<DiscoveryReport, String> {
    let projects = db
        .list_projects(true)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|project| (project.id, project.name, normalize(&project.root_path)))
        .collect::<Vec<_>>();

    let mut sessions = Vec::new();
    let mut warnings = Vec::new();
    let mut truncated = false;

    match timeout(CODEX_DEADLINE, discover_codex()).await {
        Ok(Ok((found, more))) => { sessions.extend(found); truncated = more; }
        Ok(Err(error)) => warnings.push(format!("Codex 会话未能读取：{error}")),
        Err(_) => warnings.push("Codex 会话读取超时".into()),
    }
    match discover_claude().await {
        Ok(found) => sessions.extend(found),
        Err(error) => warnings.push(format!("Claude Code 会话未能读取：{error}")),
    }

    for session in &mut sessions {
        if let Some((id, name)) = match_project(&projects, &session.cwd) {
            session.project_id = Some(id);
            session.project_name = Some(name);
        }
    }
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(DiscoveryReport { sessions, warnings, truncated })
}

/// Longest matching root wins, so a project nested inside another is not
/// swallowed by its parent.
fn match_project(projects: &[(String, String, String)], cwd: &str) -> Option<(String, String)> {
    let cwd = normalize(cwd);
    projects
        .iter()
        .filter(|(_, _, root)| !root.is_empty() && (cwd == *root || cwd.starts_with(&format!("{root}/"))))
        .max_by_key(|(_, _, root)| root.len())
        .map(|(id, name, _)| (id.clone(), name.clone()))
}

fn normalize(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    std::fs::canonicalize(trimmed)
        .map(|value| value.to_string_lossy().trim_end_matches('/').to_string())
        .unwrap_or_else(|_| trimmed.to_string())
}

fn seconds_to_rfc3339(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

// ---------------------------------------------------------------- Codex

/// Asks the installed app-server for its stored threads — the same interface the
/// official desktop client lists sessions with. Reading the rollout files
/// directly would mean reverse-engineering a format that is free to change.
async fn discover_codex() -> Result<(Vec<DiscoveredSession>, bool), String> {
    let socket = std::env::temp_dir().join(format!("vibe-working-discovery-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let codex = crate::environment::resolve("codex").ok_or_else(|| crate::environment::missing_program("codex"))?;
    let mut child = Command::new(codex)
        .args(["app-server", "--listen", &format!("unix://{}", socket.display())])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("无法启动 codex app-server：{error}"))?;

    let result = list_threads(&socket).await;
    let _ = child.kill().await;
    let _ = std::fs::remove_file(&socket);
    result
}

async fn list_threads(socket: &Path) -> Result<(Vec<DiscoveredSession>, bool), String> {
    let mut stream = None;
    for _ in 0..80 {
        if let Ok(connected) = tokio::net::UnixStream::connect(socket).await {
            stream = Some(connected);
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    let stream = stream.ok_or("codex app-server socket did not become ready")?;
    let (websocket, _) = client_async("ws://localhost", stream)
        .await
        .map_err(|error| error.to_string())?;
    let (mut writer, mut reader) = websocket.split();
    let frame = |value: Value| Message::Text(value.to_string().into());

    writer
        .send(frame(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "clientInfo": { "name": "vibe_working", "title": "Vibe Working", "version": "0.1.0" },
                "capabilities": { "experimentalApi": true }
            }
        })))
        .await
        .map_err(|error| error.to_string())?;
    let handshake = read_response(&mut reader, 1).await?;
    if let Some(error) = handshake.get("error") {
        return Err(error.to_string());
    }
    writer
        .send(frame(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} })))
        .await
        .map_err(|error| error.to_string())?;
    let mut sessions = Vec::new();
    let mut cursor: Option<String> = None;
    for page in 0..CODEX_PAGES {
        let id = 2 + page as u64;
        let mut params = json!({ "limit": CODEX_PAGE });
        if let Some(cursor) = &cursor {
            params["cursor"] = Value::String(cursor.clone());
        }
        writer
            .send(frame(json!({ "jsonrpc": "2.0", "id": id, "method": "thread/list", "params": params })))
            .await
            .map_err(|error| error.to_string())?;
        let listed = read_response(&mut reader, id).await?;
        if let Some(error) = listed.get("error") {
            return Err(error.to_string());
        }
        let threads = listed.pointer("/result/data").and_then(Value::as_array).cloned().unwrap_or_default();
        if threads.is_empty() {
            break;
        }
        sessions.extend(threads.iter().filter_map(thread_to_session));
        match listed.pointer("/result/nextCursor").and_then(Value::as_str) {
            Some(next) => cursor = Some(next.to_string()),
            None => { cursor = None; break; }
        }
    }
    Ok((sessions, cursor.is_some()))
}

fn thread_to_session(thread: &Value) -> Option<DiscoveredSession> {
    let id = thread.get("id").and_then(Value::as_str)?;
    let cwd = thread.get("cwd").and_then(Value::as_str)?;
    let preview = thread.get("preview").and_then(Value::as_str).unwrap_or_default();
    let name = thread.get("name").and_then(Value::as_str).filter(|value| !value.trim().is_empty());
    Some(DiscoveredSession {
        provider: "codex".into(),
        session_id: id.to_string(),
        cwd: cwd.to_string(),
        title: name.map(str::to_owned).unwrap_or_else(|| summarize(preview)),
        preview: summarize(preview),
        updated_at: seconds_to_rfc3339(thread.get("updatedAt").and_then(Value::as_i64).unwrap_or_default()),
        project_id: None,
        project_name: None,
    })
}

// --------------------------------------------------------------- Claude

/// Claude Code has no equivalent query interface, so this reads the transcript
/// store. The directory name is the working directory with separators replaced,
/// which is lossy and cannot be decoded back into a path — the `cwd` recorded
/// inside each transcript is the only trustworthy source.
async fn discover_claude() -> Result<Vec<DiscoveredSession>, String> {
    let Some(root) = dirs::home_dir().map(|home| home.join(".claude/projects")) else {
        return Ok(Vec::new());
    };
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    let mut directories = tokio::fs::read_dir(&root).await.map_err(|error| error.to_string())?;
    while let Ok(Some(directory)) = directories.next_entry().await {
        if !directory.path().is_dir() {
            continue;
        }
        let Ok(mut files) = tokio::fs::read_dir(directory.path()).await else { continue };
        while let Ok(Some(file)) = files.next_entry().await {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(session) = read_claude_transcript(&path).await {
                sessions.push(session);
            }
        }
    }
    Ok(sessions)
}

async fn read_claude_transcript(path: &PathBuf) -> Option<DiscoveredSession> {
    let file = tokio::fs::File::open(path).await.ok()?;
    let mut lines = tokio::io::BufReader::new(file).lines();
    let mut cwd = None;
    let mut timestamp = None;
    let mut preview = None;
    for _ in 0..CLAUDE_HEAD_LINES {
        let Ok(Some(line)) = lines.next_line().await else { break };
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        if cwd.is_none() {
            cwd = value.get("cwd").and_then(Value::as_str).map(str::to_owned);
        }
        if timestamp.is_none() {
            timestamp = value.get("timestamp").and_then(Value::as_str).map(str::to_owned);
        }
        if preview.is_none() && value.get("type").and_then(Value::as_str) == Some("user") {
            preview = first_user_text(&value);
        }
        if cwd.is_some() && timestamp.is_some() && preview.is_some() {
            break;
        }
    }
    let cwd = cwd?;
    let session_id = path.file_stem()?.to_string_lossy().to_string();
    let preview = summarize(preview.as_deref().unwrap_or_default());
    Some(DiscoveredSession {
        provider: "claude".into(),
        session_id,
        cwd,
        title: if preview.is_empty() { "Claude Code 会话".into() } else { preview.clone() },
        preview,
        updated_at: match timestamp {
            Some(value) => value,
            None => modified_at(path).await,
        },
        project_id: None,
        project_name: None,
    })
}

fn first_user_text(value: &Value) -> Option<String> {
    let content = value.pointer("/message/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    content.as_array()?.iter().find_map(|part| {
        (part.get("type").and_then(Value::as_str) == Some("text"))
            .then(|| part.get("text").and_then(Value::as_str).map(str::to_owned))
            .flatten()
    })
}

async fn modified_at(path: &PathBuf) -> String {
    tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|data| data.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| seconds_to_rfc3339(value.as_secs() as i64))
        .unwrap_or_default()
}

// ---------------------------------------------------------------- shared

fn summarize(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= 140 {
        return collapsed;
    }
    collapsed.chars().take(140).collect::<String>() + "…"
}

async fn read_response<S>(reader: &mut S, id: u64) -> Result<Value, String>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(message) = reader.next().await {
        let Ok(message) = message else { continue };
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            _ => continue,
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
        if value.get("id").and_then(Value::as_u64) == Some(id) {
            return Ok(value);
        }
    }
    Err(format!("app-server closed before answering request {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects() -> Vec<(String, String, String)> {
        vec![
            ("outer".into(), "Workspace".into(), "/Users/x/Workspace".into()),
            ("inner".into(), "Tools".into(), "/Users/x/Workspace/Tools".into()),
        ]
    }

    #[test]
    fn the_most_specific_project_claims_a_nested_directory() {
        assert_eq!(match_project(&projects(), "/Users/x/Workspace/Tools/App").unwrap().0, "inner");
        assert_eq!(match_project(&projects(), "/Users/x/Workspace/Other").unwrap().0, "outer");
    }

    #[test]
    fn a_sibling_prefix_is_not_a_match() {
        // "/Users/x/WorkspaceOther" starts with "/Users/x/Workspace" as a string
        // but is a different directory.
        assert!(match_project(&projects(), "/Users/x/WorkspaceOther").is_none());
    }

    #[test]
    fn an_unowned_directory_stays_unassigned() {
        assert!(match_project(&projects(), "/tmp/scratch").is_none());
    }

    #[test]
    fn summaries_collapse_whitespace_and_stay_bounded() {
        assert_eq!(summarize("  hello\n\nworld  "), "hello world");
        assert_eq!(summarize(&"x".repeat(500)).chars().count(), 141);
    }
}
