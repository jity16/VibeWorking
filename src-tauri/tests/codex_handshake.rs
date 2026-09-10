//! Live handshake against the Codex app-server installed on this machine.
//!
//! This is the only check that exercises the real transport the app depends on:
//! a WebSocket spoken over a Unix-domain socket. It stops before `turn/start`,
//! so it never submits anything to a model.
//!
//! Ignored by default because it needs the `codex` CLI:
//!
//! ```sh
//! cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
//! ```

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{client_async, tungstenite::Message};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the codex CLI on PATH"]
async fn app_server_speaks_websocket_over_a_unix_socket() {
    let socket = std::env::temp_dir().join(format!("vibe-working-handshake-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let cwd = std::env::temp_dir();

    let mut child = Command::new("codex")
        .args(["app-server", "--listen", &format!("unix://{}", socket.display())])
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("codex app-server did not start");

    let mut stream = None;
    for _ in 0..80 {
        if let Ok(connected) = tokio::net::UnixStream::connect(&socket).await {
            stream = Some(connected);
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    let stream = stream.expect("app-server socket never became ready");
    let (websocket, _) = client_async("ws://localhost", stream)
        .await
        .expect("app-server did not complete the WebSocket upgrade");
    let (mut writer, mut reader) = websocket.split();

    let frame = |value: Value| Message::Text(value.to_string().into());

    writer
        .send(frame(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "vibe_working", "title": "Vibe Working", "version": "0.1.0" },
                "capabilities": { "experimentalApi": true }
            }
        })))
        .await
        .expect("initialize could not be sent");
    let initialized = read_response(&mut reader, 1).await;
    assert!(initialized.get("error").is_none(), "initialize failed: {initialized}");

    writer
        .send(frame(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} })))
        .await
        .expect("initialized notification could not be sent");

    writer
        .send(frame(json!({ "jsonrpc": "2.0", "id": 2, "method": "thread/start", "params": { "cwd": cwd.to_string_lossy() } })))
        .await
        .expect("thread/start could not be sent");
    let thread = read_response(&mut reader, 2).await;
    assert!(thread.get("error").is_none(), "thread/start failed: {thread}");
    let thread_id = thread
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .expect("thread/start returned no /thread/id — the adapter reads exactly this pointer");
    assert!(!thread_id.is_empty());

    let _ = child.kill().await;
    let _ = std::fs::remove_file(&socket);
}

/// Reads until the response carrying `id` arrives, skipping notifications.
async fn read_response<S>(reader: &mut S, id: u64) -> Value
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = Duration::from_secs(30);
    timeout(deadline, async {
        while let Some(message) = reader.next().await {
            let Ok(message) = message else { continue };
            let text = match message {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                _ => continue,
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return value;
            }
        }
        panic!("app-server closed before answering request {id}");
    })
    .await
    .unwrap_or_else(|_| panic!("app-server did not answer request {id} within {deadline:?}"))
}
