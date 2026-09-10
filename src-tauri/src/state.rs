use crate::models::{AgentEvent, AgentSession, EventEnvelope};
use chrono::Utc;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Execution {
    Idle,
    Starting,
    Running,
    WaitingInput,
    Backoff,
    Completed,
    Failed,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connectivity { Connected, Reconnecting, Disconnected }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control { Automation, Human }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attention { None, InputRequired, ApprovalRequired, RecoveryFailed, Unverified }

#[derive(Debug, Clone)]
pub struct ProviderEvent {
    pub event_type: String,
    pub payload: Value,
    pub source: String,
    pub provider_event_id: Option<String>,
}

impl ProviderEvent {
    pub fn from_json(event_type: impl Into<String>, payload: Value, source: impl Into<String>, id: Option<String>) -> Self {
        Self { event_type: event_type.into(), payload, source: source.into(), provider_event_id: id }
    }
}

pub fn reduce(session: &mut AgentSession, event: &ProviderEvent) -> EventEnvelope {
    let mut message = None;
    match event.event_type.as_str() {
        "connection/open" => session.connectivity_status = "connected".into(),
        "connection/lost" => {
            session.connectivity_status = "disconnected".into();
            if matches!(session.execution_status.as_str(), "running" | "starting" | "waiting_input") { session.execution_status = "unknown".into(); }
            session.attention = "unverified".into();
            message = Some("Agent connection lost; result is unverified".into());
        }
        "turn/started" | "session_started" => {
            session.execution_status = "running".into();
            session.connectivity_status = "connected".into();
            session.attention = "none".into();
            message = Some("Turn started".into());
        }
        "turn/plan/updated" => {
            if let Some(plan) = event.payload.get("plan").and_then(Value::as_array) {
                let active = plan.iter().find(|step| step.get("status").and_then(Value::as_str) == Some("inProgress"));
                session.current_step = active.and_then(|step| step.get("step").and_then(Value::as_str)).map(str::to_owned);
            }
        }
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" | "item/permissions/requestApproval" | "approval_required" => {
            session.execution_status = "waiting_input".into();
            session.attention = "approval_required".into();
            message = Some("Approval required".into());
        }
        "item/tool/requestUserInput" | "tool/requestUserInput" | "input_required" | "notification_prompt" => {
            session.execution_status = "waiting_input".into();
            session.attention = "input_required".into();
            message = Some("Agent is waiting for your input".into());
        }
        "serverRequest/resolved" | "attention/resolved" => {
            if session.execution_status == "waiting_input" {
                session.execution_status = "running".into();
            }
            session.attention = "none".into();
        }
        "turn/completed" | "session_end" | "stop" => {
            let status = event.payload.get("turn").and_then(|v| v.get("status")).and_then(Value::as_str).or_else(|| event.payload.get("status").and_then(Value::as_str));
            match status {
                Some("completed") | Some("success") | Some("succeeded") => { session.execution_status = "completed".into(); session.attention = "none".into(); message = Some("Run finished; review the result".into()); }
                Some("interrupted") | Some("stopped") | Some("cancelled") => { session.execution_status = "stopped".into(); session.attention = "none".into(); message = Some("Run stopped".into()); }
                Some("failed") | Some("error") | Some("failure") => { session.execution_status = "failed".into(); session.attention = "none".into(); message = Some("Run failed".into()); }
                _ => { session.execution_status = "unknown".into(); session.attention = "unverified".into(); message = Some("Run ended without a verified status".into()); }
            }
        }
        "overload" => {
            session.execution_status = "backoff".into();
            session.attention = "none".into();
            message = Some("Waiting before a safe retry".into());
        }
        "error" | "api_error" => {
            session.execution_status = "failed".into();
            session.attention = "none".into();
            message = Some(event.payload.get("message").and_then(Value::as_str).unwrap_or("Agent reported an error").to_string());
        }
        _ => {
            if let Some(text) = event.payload.get("message").and_then(Value::as_str) { message = Some(text.chars().take(160).collect()); }
        }
    }
    session.recent_activity = message.clone().or_else(|| Some(event.event_type.clone()));
    session.last_activity_at = Some(Utc::now().to_rfc3339());
    session.updated_at = Utc::now().to_rfc3339();
    EventEnvelope { session_id: session.id.clone(), event_type: event.event_type.clone(), execution_status: session.execution_status.clone(), connectivity_status: session.connectivity_status.clone(), control_mode: session.control_mode.clone(), attention: session.attention.clone(), message }
}

pub fn is_recoverable_overload(event: &ProviderEvent) -> bool {
    if event.source != "structured" && event.source != "app-server" { return false; }
    if event.event_type != "turn/completed" || event.payload.pointer("/turn/status").and_then(Value::as_str) != Some("failed") { return false; }
    event.payload.pointer("/turn/error/codexErrorInfo").and_then(Value::as_str) == Some("serverOverloaded")
}

pub fn to_agent_event(session_id: &str, run_id: Option<&str>, event: &ProviderEvent) -> AgentEvent {
    let payload = event.payload.to_string();
    let provider_event_id = event.provider_event_id.clone().or_else(|| Some(format!("{}:{}", event.event_type, payload)));
    AgentEvent { id: uuid::Uuid::new_v4().to_string(), session_id: session_id.to_string(), run_id: run_id.map(str::to_owned), provider_event_id, event_type: event.event_type.clone(), source: event.source.clone(), payload, occurred_at: Utc::now().to_rfc3339() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentSession;

    fn session() -> AgentSession {
        AgentSession { id: "s".into(), project_id: "p".into(), task_id: Some("t".into()), provider: "codex".into(), display_name: "Codex".into(), provider_session_id: None, process_id: None, execution_status: "starting".into(), connectivity_status: "reconnecting".into(), control_mode: "automation".into(), attention: "none".into(), current_step: None, recent_activity: None, last_activity_at: None, terminal_bound: false, created_at: "".into(), updated_at: "".into() }
    }

    #[test]
    fn approval_is_waiting_and_distinct_from_takeover() {
        let mut s = session();
        let e = ProviderEvent::from_json("item/commandExecution/requestApproval", serde_json::json!({"reason":"write"}), "app-server", None);
        reduce(&mut s, &e);
        assert_eq!(s.execution_status, "waiting_input");
        assert_eq!(s.attention, "approval_required");
        assert_eq!(s.control_mode, "automation");
    }

    #[test]
    fn stop_is_not_task_completion() {
        let mut s = session();
        let e = ProviderEvent::from_json("turn/completed", serde_json::json!({"turn":{"status":"interrupted"}}), "app-server", None);
        reduce(&mut s, &e);
        assert_eq!(s.execution_status, "stopped");
    }

    #[test]
    fn plain_log_text_cannot_trigger_retry() {
        let e = ProviderEvent::from_json("log", serde_json::json!({"text":"a comment says model at capacity"}), "log", None);
        assert!(!is_recoverable_overload(&e));
    }
}
