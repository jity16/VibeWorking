use crate::db::{DbError, Database};
use crate::models::{AgentSession, Recap, Run, StartRunInput, TerminalBinding};
use crate::state::{self, ProviderEvent};
use crate::retry::RetryController;
use crate::terminal::{self, TmuxTarget};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter};
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout, Duration};
use tokio_tungstenite::{client_async, tungstenite::Message};
use uuid::Uuid;
use rusqlite::types::Value as SqlValue;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent error: {0}")]
    Message(String),
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("terminal error: {0}")]
    Terminal(#[from] terminal::TerminalError),
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("RPC request timed out")]
    Timeout,
}

#[derive(Clone)]
pub struct AgentManager {
    pub db: Database,
    pub runtimes: Arc<Mutex<HashMap<String, Arc<Runtime>>>>,
}

pub struct Runtime {
    pub session_id: String,
    pub run_id: String,
    pub db: Database,
    pub app: AppHandle,
    pub rpc: Mutex<Option<RpcClient>>,
    pub target: Mutex<Option<TmuxTarget>>,
    pub pending_requests: Mutex<HashMap<String, Value>>,
    pub output: Mutex<String>,
    pub stopped: AtomicBool,
    pub control_gate: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct RpcClient {
    tx: mpsc::UnboundedSender<Message>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>,
    next_id: Arc<Mutex<u64>>,
}

impl AgentManager {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            runtimes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn sessions(&self) -> Result<Vec<AgentSession>, String> {
        self.db.list_sessions().map_err(|e| e.to_string())
    }

    pub async fn start_run(
        &self,
        app: AppHandle,
        input: StartRunInput,
    ) -> Result<AgentSession, String> {
        let task = self.db.get_task(&input.task_id).map_err(|e| e.to_string())?;
        let project = self.db.get_project(&task.project_id).map_err(|e| e.to_string())?;
        let prompt = task
            .current_prompt
            .clone()
            .unwrap_or_else(|| task.original_request.clone());
        if prompt.trim().is_empty() {
            return Err("task has no prompt".into());
        }
        let provider = input.provider.to_ascii_lowercase();
        if provider != "codex" && provider != "claude" {
            return Err("provider must be codex or claude".into());
        }
        if !std::path::Path::new(&project.root_path).is_dir() {
            return Err("project directory does not exist".into());
        }

        let session_id = Uuid::new_v4().to_string();
        let run_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let session = AgentSession {
            id: session_id.clone(),
            project_id: project.id.clone(),
            task_id: Some(task.id.clone()),
            provider: provider.clone(),
            display_name: format!(
                "{} · {}",
                if provider == "codex" { "Codex" } else { "Claude Code" },
                task.title
            ),
            provider_session_id: None,
            process_id: None,
            execution_status: "starting".into(),
            connectivity_status: "reconnecting".into(),
            control_mode: "automation".into(),
            attention: "none".into(),
            current_step: None,
            recent_activity: Some("Starting agent".into()),
            last_activity_at: Some(now.clone()),
            terminal_bound: false,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let run = Run {
            id: run_id.clone(),
            task_id: task.id.clone(),
            session_id: session_id.clone(),
            prompt_snapshot: prompt.clone(),
            status: "starting".into(),
            started_at: now,
            ended_at: None,
            final_output: None,
        };
        self.db.begin_run(&session, &run, input.auto_retry).map_err(|e| e.to_string())?;

        let runtime = Arc::new(Runtime {
            session_id: session_id.clone(),
            run_id,
            db: self.db.clone(),
            app: app.clone(),
            rpc: Mutex::new(None),
            target: Mutex::new(None),
            pending_requests: Mutex::new(HashMap::new()),
            output: Mutex::new(String::new()),
            stopped: AtomicBool::new(false),
            control_gate: tokio::sync::Mutex::new(()),
        });
        self.runtimes
            .lock()
            .map_err(|_| "runtime lock poisoned".to_string())?
            .insert(session_id, runtime.clone());
        let manager = self.clone();
        tokio::spawn(async move {
            let result = if provider == "codex" {
                manager
                    .start_codex(runtime.clone(), &project.root_path, &prompt)
                    .await
            } else {
                manager
                    .start_claude(runtime.clone(), &project.root_path, &prompt)
                    .await
            };
            if let Err(error) = result {
                let _ = manager.fail_runtime(&runtime, error.to_string()).await;
            }
        });
        Ok(session)
    }

    async fn start_codex(
        &self,
        runtime: Arc<Runtime>,
        cwd: &str,
        prompt: &str,
    ) -> Result<(), AgentError> {
        let socket_dir = self
            .db
            .path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("sockets");
        std::fs::create_dir_all(&socket_dir)
            .map_err(|e| AgentError::Message(e.to_string()))?;
        let socket = socket_dir.join(format!("{}.sock", runtime.session_id));
        let _ = std::fs::remove_file(&socket);
        let listen = format!("unix://{}", socket.display());
        self.db.set_runtime_field(&runtime.session_id, "socket_path", SqlValue::Text(socket.to_string_lossy().to_string()))?;
        let mut child = Command::new("codex")
            .args(["app-server", "--listen", &listen])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                AgentError::Message(format!("could not start codex app-server: {e}"))
            })?;
        let pid = child.id().map(|v| v as i64);
        self.set_process_id(&runtime.session_id, pid)?;
        let manager = self.clone();
        let wait_runtime = runtime.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            if !wait_runtime.stopped.load(Ordering::SeqCst) {
                let payload = json!({
                    "status": if status.map(|s| s.success()).unwrap_or(false) {
                        "success"
                    } else {
                        "failed"
                    }
                });
                let event = ProviderEvent::from_json("session_end", payload, "process", None);
                let _ = manager.handle_event(wait_runtime, event).await;
            }
        });

        let mut connected = false;
        for _ in 0..80 {
            if tokio::net::UnixStream::connect(&socket).await.is_ok() {
                connected = true;
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        if !connected {
            return Err(AgentError::Message(
                "codex app-server socket did not become ready".into(),
            ));
        }
        let client = RpcClient::connect(&socket, runtime.clone()).await?;
        runtime
            .rpc
            .lock()
            .map_err(|_| AgentError::Message("runtime lock poisoned".into()))?
            .replace(client.clone());
        let _ = client
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "vibe_working",
                        "title": "Vibe Working",
                        "version": "0.1.0"
                    },
                    "capabilities": { "experimentalApi": true }
                }),
            )
            .await?;
        client.notify("initialized", json!({}))?;
        let thread = client.request("thread/start", json!({ "cwd": cwd })).await?;
        let thread_id = thread
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Rpc("thread/start returned no thread id".into()))?
            .to_string();
        self.set_provider_session_id(&runtime.session_id, &thread_id)?;
        let target = terminal::tmux_create(
            &runtime.session_id,
            cwd,
            &vec!["codex".into(), "--remote".into(), listen],
        )
        .await?;
        self.bind_terminal(&runtime, &target, cwd, "codex").await?;
        let turn = client
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [{ "type": "text", "text": prompt }]
                }),
            )
            .await?;
        if let Some(turn_id) = turn.pointer("/turn/id").and_then(Value::as_str) {
            self.db.set_runtime_field(&runtime.session_id, "turn_id", SqlValue::Text(turn_id.into()))?;
        }
        Ok(())
    }

    async fn start_claude(
        &self,
        runtime: Arc<Runtime>,
        cwd: &str,
        prompt: &str,
    ) -> Result<(), AgentError> {
        let support = self
            .db
            .path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        let hook_dir = support.join("hooks");
        std::fs::create_dir_all(&hook_dir)
            .map_err(|e| AgentError::Message(e.to_string()))?;
        let hook_file = hook_dir.join(format!("{}.jsonl", runtime.session_id));
        let settings_file = hook_dir.join(format!("{}.settings.json", runtime.session_id));
        let exe = std::env::current_exe().map_err(|e| AgentError::Message(e.to_string()))?;
        write_claude_settings(&settings_file, &exe, &runtime.session_id)?;
        let session_uuid = Uuid::new_v4().to_string();
        let args = vec![
            "claude".into(),
            "--session-id".into(),
            session_uuid.clone(),
            "--name".into(),
            runtime.session_id.clone(),
            "--settings".into(),
            settings_file.to_string_lossy().to_string(),
            "--permission-mode".into(),
            "manual".into(),
        ];
        let hook_env = hook_file.to_string_lossy().to_string();
        let target = terminal::tmux_create_with_env(
            &runtime.session_id,
            cwd,
            &args,
            &[
                ("VIBE_WORKING_HOOK_FILE", hook_env.as_str()),
                ("VIBE_WORKING_SESSION_ID", runtime.session_id.as_str()),
            ],
        )
        .await?;
        self.bind_terminal(&runtime, &target, cwd, "claude").await?;
        let pane_pid = pane_pid(&target).await.ok().flatten();
        self.set_process_id(&runtime.session_id, pane_pid)?;
        self.set_provider_session_id(&runtime.session_id, &session_uuid)?;
        let manager = self.clone();
        let monitor_runtime = runtime.clone();
        tokio::spawn(async move {
            monitor_claude_hooks(manager, monitor_runtime, hook_file).await;
        });
        let _ = prompt;
        Ok(())
    }

    async fn bind_terminal(
        &self,
        runtime: &Arc<Runtime>,
        target: &TmuxTarget,
        cwd: &str,
        _command: &str,
    ) -> Result<(), AgentError> {
        runtime
            .target
            .lock()
            .map_err(|_| AgentError::Message("runtime lock poisoned".into()))?
            .replace(target.clone());
        let (pid, identity) = terminal::terminal_identity(target).await?;
        let binding = TerminalBinding {
            id: Uuid::new_v4().to_string(),
            session_id: runtime.session_id.clone(),
            target_kind: "tmux".into(),
            target_value: target.session.clone(),
            process_id: Some(pid),
            tty: None,
            cwd: cwd.into(),
            command_fingerprint: Some(identity),
            verified_at: Some(Utc::now().to_rfc3339()),
        };
        self.db.upsert_terminal_binding(&binding)?;
        let mut session = self.current_session(&runtime.session_id)?;
        session.terminal_bound = true;
        session.updated_at = Utc::now().to_rfc3339();
        self.db.update_session_state(&session)?;
        Ok(())
    }

    fn set_process_id(&self, id: &str, pid: Option<i64>) -> Result<(), AgentError> {
        let mut session = self.current_session(id)?;
        session.process_id = pid;
        session.updated_at = Utc::now().to_rfc3339();
        self.db.update_session_state(&session)?;
        Ok(())
    }

    fn set_provider_session_id(
        &self,
        id: &str,
        provider_id: &str,
    ) -> Result<(), AgentError> {
        let mut session = self.current_session(id)?;
        session.provider_session_id = Some(provider_id.into());
        session.connectivity_status = "connected".into();
        session.updated_at = Utc::now().to_rfc3339();
        self.db.update_session_state(&session)?;
        Ok(())
    }

    fn current_session(&self, id: &str) -> Result<AgentSession, AgentError> {
        self.db
            .list_sessions()?
            .into_iter()
            .find(|session| session.id == id)
            .ok_or_else(|| AgentError::Message("session not found".into()))
    }

    async fn fail_runtime(
        &self,
        runtime: &Arc<Runtime>,
        error: String,
    ) -> Result<(), AgentError> {
        let event = ProviderEvent::from_json("error", json!({ "message": error }), "process", None);
        self.handle_event(runtime.clone(), event).await
    }

    pub async fn stop(&self, session_id: &str) -> Result<(), String> {
        let runtime = self
            .runtimes
            .lock()
            .map_err(|_| "runtime lock poisoned".to_string())?
            .get(session_id)
            .cloned()
            .ok_or_else(|| "session is not active in this app instance".to_string())?;
        runtime.stopped.store(true, Ordering::SeqCst);
        let _control = runtime.control_gate.lock().await;
        self.db.cancel_retries(session_id).map_err(|error| error.to_string())?;
        let target = runtime
            .target
            .lock()
            .map_err(|_| "runtime lock poisoned".to_string())?
            .clone();
        if let Some(target) = target {
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", &target.session])
                .output()
                .await;
        }
        let event = ProviderEvent::from_json("stop", json!({ "status": "stopped" }), "user", None);
        self.handle_event(runtime.clone(), event)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn focus(&self, session_id: &str) -> Result<(), String> {
        let binding = self.db.terminal_binding(session_id).map_err(|error| error.to_string())?.ok_or("terminal is not bound")?;
        if !terminal::verify_binding(&binding).await.map_err(|error| error.to_string())? {
            return Err("terminal is no longer bound to this session".into());
        }
        let target = TmuxTarget { session: binding.target_value.clone(), pane: format!("{}:0.0", binding.target_value) };
        terminal::focus_terminal(&target)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn take_over(&self, session_id: &str) -> Result<AgentSession, String> {
        let runtime = self.runtimes.lock().map_err(|_| "runtime lock poisoned")?.get(session_id).cloned();
        let _control = if let Some(runtime) = &runtime { Some(runtime.control_gate.lock().await) } else { None };
        self.db.cancel_retries(session_id).map_err(|error| error.to_string())?;
        let mut session = self.current_session(session_id).map_err(|e| e.to_string())?;
        {
            session.control_mode = "human".into();
            session.updated_at = Utc::now().to_rfc3339();
            self.db
                .update_session_state(&session)
                .map_err(|e| e.to_string())?;
        }
        Ok(session)
    }

    pub async fn answer_request(
        &self,
        session_id: &str,
        request_id: String,
        result: Value,
    ) -> Result<(), String> {
        let runtime = self
            .runtimes
            .lock()
            .map_err(|_| "runtime lock poisoned".to_string())?
            .get(session_id)
            .cloned()
            .ok_or_else(|| "session is not active".to_string())?;
        let has = runtime
            .pending_requests
            .lock()
            .map_err(|_| "runtime lock poisoned".to_string())?
            .remove(&request_id)
            .is_some();
        if !has {
            return Err("request is no longer pending".into());
        }
        let client = runtime
            .rpc
            .lock()
            .map_err(|_| "runtime lock poisoned".to_string())?
            .clone()
            .ok_or_else(|| "control channel unavailable".to_string())?;
        client
            .respond(Value::String(request_id.clone()), result)
            .map_err(|e| e.to_string())?;
        let event = ProviderEvent::from_json(
            "serverRequest/resolved",
            json!({ "requestId": request_id }),
            "user",
            Some(request_id),
        );
        self.handle_event(runtime, event)
            .await
            .map_err(|e| e.to_string())
    }

    async fn handle_event(
        &self,
        runtime: Arc<Runtime>,
        event: ProviderEvent,
    ) -> Result<(), AgentError> {
        if event.event_type == "item/agentMessage/delta" {
            if let Some(delta) = event.payload.get("delta").and_then(Value::as_str) {
                runtime
                    .output
                    .lock()
                    .map_err(|_| AgentError::Message("runtime lock poisoned".into()))?
                    .push_str(delta);
            }
        }
        let mut session = self.current_session(&runtime.session_id)?;
        let mut reduced = state::reduce(&mut session, &event);
        if let Some(turn_id) = event.payload.pointer("/turn/id").and_then(Value::as_str) {
            self.db.set_runtime_field(&runtime.session_id,"turn_id",SqlValue::Text(turn_id.into()))?;
            self.db.save_turn(&runtime.run_id,turn_id,&session.execution_status,extract_final_text(&event.payload).as_deref())?;
        }
        let retry_allowed = state::is_recoverable_overload(&event) && session.provider == "codex" && session.control_mode == "automation" && session.attention == "none" && self.db.runtime_info(&runtime.session_id)?["auto_retry"] == true;
        if retry_allowed { session.execution_status = "backoff".into(); reduced.execution_status = "backoff".into(); }
        let agent_event = state::to_agent_event(&runtime.session_id, Some(&runtime.run_id), &event);
        let inserted = self.db.save_event_state(&agent_event, &session)?;
        if inserted {
            let _ = runtime.app.emit("agent-event", &reduced);
        }
        if inserted && retry_allowed {
            self.schedule_retry(runtime.clone(), &session, &event).await?;
        }
        if inserted && !retry_allowed && matches!(
            event.event_type.as_str(),
            "turn/completed" | "session_end" | "stop" | "error"
        ) {
            self.finish_run(&runtime, &session, &event).await?;
        }
        Ok(())
    }

    async fn schedule_retry(&self, runtime: Arc<Runtime>, session: &AgentSession, event: &ProviderEvent) -> Result<(), AgentError> {
        let config = self.db.runtime_info(&runtime.session_id)?;
        if config.get("auto_retry").and_then(Value::as_bool) != Some(true) { return Ok(()); }
        let run_id = config.get("run_id").and_then(Value::as_str).unwrap_or(&runtime.run_id);
        let key = event.provider_event_id.clone().unwrap_or_else(|| event.payload.to_string());
        let job = match self.db.retry_job(&runtime.session_id)? {
            Some(mut job) if job.status == "sent" && job.run_id == run_id => { RetryController::mark_backoff(&mut job,Utc::now(),"Model overloaded"); job },
            Some(_) => return Ok(()),
            None => RetryController::new_job(session, run_id, &key, Utc::now()),
        };
        self.db.save_retry(&job)?;
        let manager = self.clone();
        tokio::spawn(async move {
            if let Some(next) = job.next_attempt_at.as_deref().and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok()) {
                let delay = (next.with_timezone(&Utc) - Utc::now()).to_std().unwrap_or(Duration::from_secs(0));
                sleep(delay).await;
            }
            let _ = manager.try_retry(runtime, job.id).await;
        });
        Ok(())
    }

    async fn try_retry(&self, runtime: Arc<Runtime>, job_id: String) -> Result<(), AgentError> {
        let _control = runtime.control_gate.lock().await;
        let mut job = self.db.retry_job(&runtime.session_id)?.ok_or_else(|| AgentError::Message("retry job disappeared".into()))?;
        if job.id != job_id || job.status != "scheduled" || runtime.stopped.load(Ordering::SeqCst) { return Ok(()); }
        let mut session = self.current_session(&runtime.session_id)?;
        if job.attempts >= job.max_attempts || chrono::DateTime::parse_from_rfc3339(&job.total_deadline_at).map(|deadline| Utc::now() >= deadline).unwrap_or(true) {
            job.status="exhausted".into(); self.db.save_retry(&job)?;
            session.attention="recovery_failed".into(); session.execution_status="failed".into(); self.db.update_session_state(&session)?;
            return Ok(());
        }
        let binding = self.db.terminal_binding(&runtime.session_id)?.ok_or_else(|| AgentError::Message("terminal binding missing; manual recovery required".into()))?;
        if session.control_mode != "automation" || session.connectivity_status != "connected" || session.execution_status != "backoff" || session.attention != "none" || !terminal::verify_binding(&binding).await? { self.db.cancel_retries(&runtime.session_id)?; return Ok(()); }
        let thread_id = session.provider_session_id.clone().ok_or_else(|| AgentError::Message("provider session id missing".into()))?;
        let client = runtime.rpc.lock().map_err(|_| AgentError::Message("runtime lock poisoned".into()))?.clone().ok_or_else(|| AgentError::Message("control channel unavailable".into()))?;
        if !runtime.pending_requests.lock().map_err(|_| AgentError::Message("request lock poisoned".into()))?.is_empty() { return Ok(()); }
        let current = client.request("thread/read",json!({"threadId":thread_id,"includeTurns":true})).await?;
        if current.pointer("/thread/status/type").and_then(Value::as_str) != Some("idle") { self.db.cancel_retries(&runtime.session_id)?; return Ok(()); }
        job.status = "sending".into(); self.db.save_retry(&job)?;
        let result = client.request("turn/start", json!({"threadId":thread_id,"input":[{"type":"text","text":"continue"}]})).await;
        match result {
            Ok(_) => { RetryController::mark_sent(&mut job, Utc::now()); self.db.save_retry(&job)?; Ok(()) },
            Err(error) => { job.status="uncertain".into(); job.last_error=Some(error.to_string()); self.db.save_retry(&job)?; session.attention="recovery_failed".into();session.execution_status="unknown".into();self.db.update_session_state(&session)?; Err(error) }
        }
    }

    async fn finish_run(
        &self,
        runtime: &Arc<Runtime>,
        session: &AgentSession,
        event: &ProviderEvent,
    ) -> Result<(), AgentError> {
        let output = runtime
            .output
            .lock()
            .map_err(|_| AgentError::Message("runtime lock poisoned".into()))?
            .clone();
        let status = match session.execution_status.as_str() {
            "completed" => "completed",
            "stopped" => "stopped",
            "failed" => "failed",
            _ => "unknown",
        };
        let mut run = self.db.get_run(&runtime.run_id)?;
        run.status = status.into();
        run.ended_at = Some(Utc::now().to_rfc3339());
        if run.final_output.is_none() {
            run.final_output = if output.is_empty() {
                extract_final_text(&event.payload)
            } else {
                Some(output)
            };
        }
        self.db.update_run(&run)?;
        if status == "completed" {
            if let Some(task_id) = &session.task_id {
                let _ = self.db.update_task_status(task_id, "review");
            }
        }
        let recap = Recap {
            id: Uuid::new_v4().to_string(),
            run_id: runtime.run_id.clone(),
            outcome: status.into(),
            summary: run.final_output.clone(),
            changes: None,
            verification: None,
            pending: if status == "completed" {
                Some("Review the changed files and mark the task complete when verified.".into())
            } else {
                None
            },
            source: "agent_final_output".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        self.db.insert_recap_if_absent(&recap)?;
        Ok(())
    }
}

impl RpcClient {
    async fn connect(socket: &PathBuf, runtime: Arc<Runtime>) -> Result<Self, AgentError> {
        let stream = tokio::net::UnixStream::connect(socket)
            .await
            .map_err(|e| AgentError::Rpc(e.to_string()))?;
        let (ws, _) = client_async("ws://localhost", stream)
            .await
            .map_err(|e| AgentError::Rpc(e.to_string()))?;
        let (mut writer, mut reader) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let pending: Arc<
            Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
        > = Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1));
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if writer.send(message).await.is_err() {
                    break;
                }
            }
        });
        let read_client = RpcClient {
            tx: tx.clone(),
            pending: pending.clone(),
            next_id: next_id.clone(),
        };
        let reader_client = read_client.clone();
        tokio::spawn(async move {
            while let Some(message) = reader.next().await {
                let Ok(message) = message else { break };
                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                    _ => continue,
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if let Some(id) = value.get("id") {
                    let key = id.to_string();
                    let sender = reader_client
                        .pending
                        .lock()
                        .ok()
                        .and_then(|mut map| map.remove(&key));
                    if let Some(sender) = sender {
                        if let Some(error) = value.get("error") {
                            let _ = sender.send(Err(error.to_string()));
                        } else {
                            let _ = sender
                                .send(Ok(value.get("result").cloned().unwrap_or(Value::Null)));
                        }
                        continue;
                    }
                    if value.get("method").is_some() {
                        if let Some(method) = value.get("method").and_then(Value::as_str) {
                            let event = ProviderEvent::from_json(
                                method,
                                value.get("params").cloned().unwrap_or(Value::Null),
                                "app-server",
                                Some(id.to_string()),
                            );
                            let _ = handle_server_request_or_event(
                                runtime.clone(),
                                reader_client.clone(),
                                id.clone(),
                                event,
                            )
                            .await;
                        }
                    }
                } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                    let event = ProviderEvent::from_json(
                        method,
                        value.get("params").cloned().unwrap_or(Value::Null),
                        "app-server",
                        value
                            .pointer("/params/id")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    );
                    let manager = AgentManager {
                        db: runtime.db.clone(),
                        runtimes: Arc::new(Mutex::new(HashMap::new())),
                    };
                    let _ = manager.handle_event(runtime.clone(), event).await;
                }
            }
            let manager = AgentManager {
                db: runtime.db.clone(),
                runtimes: Arc::new(Mutex::new(HashMap::new())),
            };
            let _ = manager
                .handle_event(
                    runtime,
                    ProviderEvent::from_json(
                        "connection/lost",
                        json!({}),
                        "app-server",
                        None,
                    ),
                )
                .await;
        });
        Ok(read_client)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, AgentError> {
        let id = {
            let mut next = self
                .next_id
                .lock()
                .map_err(|_| AgentError::Rpc("RPC lock poisoned".into()))?;
            let id = *next;
            *next += 1;
            id
        };
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| AgentError::Rpc("RPC lock poisoned".into()))?
            .insert(id.to_string(), sender);
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        match timeout(Duration::from_secs(45), receiver).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(AgentError::Rpc(error)),
            _ => {
                let _ = self
                    .pending
                    .lock()
                    .map(|mut map| map.remove(&id.to_string()));
                Err(AgentError::Timeout)
            }
        }
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), AgentError> {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn respond(&self, id: Value, result: Value) -> Result<(), AgentError> {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    fn send(&self, value: Value) -> Result<(), AgentError> {
        self.tx
            .send(Message::Text(value.to_string().into()))
            .map_err(|_| AgentError::Rpc("RPC connection closed".into()))
    }
}

async fn handle_server_request_or_event(
    runtime: Arc<Runtime>,
    client: RpcClient,
    id: Value,
    event: ProviderEvent,
) -> Result<(), AgentError> {
    let method = event.event_type.clone();
    if method.contains("requestApproval")
        || method.contains("requestUserInput")
        || method == "mcpServer/elicitation/request"
    {
        runtime
            .pending_requests
            .lock()
            .map_err(|_| AgentError::Message("runtime lock poisoned".into()))?
            .insert(id.to_string(), event.payload.clone());
    }
    let manager = AgentManager {
        db: runtime.db.clone(),
        runtimes: Arc::new(Mutex::new(HashMap::new())),
    };
    manager.handle_event(runtime.clone(), event).await?;
    let _ = client; // explicit UI answer owns the response
    Ok(())
}

async fn monitor_claude_hooks(manager: AgentManager, runtime: Arc<Runtime>, path: PathBuf) {
    let mut offset = 0usize;
    loop {
        if runtime.stopped.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(bytes) = tokio::fs::read(&path).await {
            if bytes.len() > offset {
                let chunk = &bytes[offset..];
                offset = bytes.len();
                for line in String::from_utf8_lossy(chunk).lines() {
                    if let Ok(value) = serde_json::from_str::<Value>(line) {
                        let hook = value.get("hook").and_then(Value::as_str).unwrap_or("unknown");
                        let payload = value.get("payload").cloned().unwrap_or(Value::Null);
                        let event_type = match hook {
                            "Stop" => "stop",
                            "SessionEnd" => "session_end",
                            "Notification" => notification_event(&payload),
                            "PreToolUse" => "tool_started",
                            "PostToolUse" => "tool_completed",
                            "UserPromptSubmit" => "turn/started",
                            "SessionStart" => "session_started",
                            _ => "claude_event",
                        };
                        let event = ProviderEvent::from_json(
                            event_type,
                            payload,
                            "structured",
                            value
                                .get("event_id")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        );
                        let _ = manager.handle_event(runtime.clone(), event).await;
                    }
                }
            }
        }
        let target = runtime.target.lock().ok().and_then(|target| target.clone());
        if let Some(target) = target {
            if !terminal::tmux_exists(&target).await {
                let _ = manager
                    .handle_event(
                        runtime.clone(),
                        ProviderEvent::from_json(
                            "session_end",
                            json!({ "status": "failed" }),
                            "process",
                            None,
                        ),
                    )
                    .await;
                break;
            }
        }
        sleep(Duration::from_millis(350)).await;
    }
}

fn notification_event(payload: &Value) -> &'static str {
    match payload
        .get("notification_type")
        .or_else(|| payload.get("type"))
        .and_then(Value::as_str)
    {
        Some("permission_prompt") | Some("approval_required") => "approval_required",
        Some("idle_prompt") | Some("elicitation_dialog") | Some("input_required") => {
            "input_required"
        }
        _ => "claude_notification",
    }
}

fn write_claude_settings(
    path: &PathBuf,
    exe: &PathBuf,
    session_id: &str,
) -> Result<(), AgentError> {
    let command = format!(
        "{} hook --session-id {} --event",
        shell_quote(&exe.to_string_lossy()),
        shell_quote(session_id)
    );
    let mut hooks = serde_json::Map::new();
    for name in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "Notification",
        "Stop",
        "SessionEnd",
    ] {
        hooks.insert(
            name.to_string(),
            json!([{
                "matcher": "*",
                "hooks": [{
                    "type": "command",
                    "command": format!("{} {}", command, name),
                    "timeout": 3
                }]
            }]),
        );
    }
    let value = json!({ "hooks": hooks });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&value)
            .map_err(|e| AgentError::Message(e.to_string()))?,
    )
    .map_err(|e| AgentError::Message(e.to_string()))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

async fn pane_pid(target: &TmuxTarget) -> Result<Option<i64>, AgentError> {
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            &target.pane,
            "#{pane_pid}",
        ])
        .output()
        .await
        .map_err(|e| AgentError::Message(e.to_string()))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().parse().ok())
}

fn extract_final_text(payload: &Value) -> Option<String> {
    if let Some(text) = payload
        .pointer("/turn/items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().rev().find_map(|item| {
                if item.get("type").and_then(Value::as_str) == Some("agentMessage") {
                    item.get("text").and_then(Value::as_str).map(str::to_owned)
                } else {
                    None
                }
            })
        })
    {
        return Some(text);
    }
    payload.get("text").and_then(Value::as_str).map(str::to_owned)
}
