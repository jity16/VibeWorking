use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub context: String,
    pub constraints: String,
    pub archived: bool,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub original_request: String,
    pub current_prompt: Option<String>,
    pub adopted_prompt_version_id: Option<String>,
    pub status: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub latest_run_status: Option<String>,
    pub latest_recap: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVersion {
    pub id: String,
    pub task_id: String,
    pub content: String,
    pub source: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub prompt_snapshot: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub final_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub provider: String,
    pub display_name: String,
    pub provider_session_id: Option<String>,
    pub process_id: Option<i64>,
    pub execution_status: String,
    pub connectivity_status: String,
    pub control_mode: String,
    pub attention: String,
    pub current_step: Option<String>,
    pub recent_activity: Option<String>,
    pub last_activity_at: Option<String>,
    pub terminal_bound: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub provider_event_id: Option<String>,
    pub event_type: String,
    pub source: String,
    pub payload: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalBinding {
    pub id: String,
    pub session_id: String,
    pub target_kind: String,
    pub target_value: String,
    pub process_id: Option<i64>,
    pub tty: Option<String>,
    pub cwd: String,
    pub command_fingerprint: Option<String>,
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryJob {
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub overload_event_key: String,
    pub attempts: i64,
    pub max_attempts: i64,
    pub next_attempt_at: Option<String>,
    pub total_deadline_at: String,
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recap {
    pub id: String,
    pub run_id: String,
    pub outcome: String,
    pub summary: Option<String>,
    pub changes: Option<String>,
    pub verification: Option<String>,
    pub pending: Option<String>,
    pub source: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxySettings {
    pub base_url: String,
    pub protocol: String,
    pub model: String,
    pub timeout_seconds: u64,
    pub api_key_ref: Option<String>,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapData {
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub sessions: Vec<AgentSession>,
    pub settings: ProxySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectInput {
    pub name: String,
    pub root_path: String,
    pub context: Option<String>,
    pub constraints: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProjectInput {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub context: String,
    pub constraints: String,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskInput {
    pub project_id: String,
    pub title: String,
    pub original_request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskInput {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub original_request: String,
    pub current_prompt: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizePromptInput {
    pub task_id: String,
    pub include_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveProxySettingsInput {
    pub base_url: String,
    pub protocol: String,
    pub model: String,
    pub timeout_seconds: u64,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRunInput {
    pub task_id: String,
    pub provider: String,
    pub auto_retry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub session_id: String,
    pub event_type: String,
    pub execution_status: String,
    pub connectivity_status: String,
    pub control_mode: String,
    pub attention: String,
    pub message: Option<String>,
}
