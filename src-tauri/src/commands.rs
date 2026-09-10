use crate::models::*;
use crate::proxy;
use serde_json::Value;
use tauri::State;

#[tauri::command]
pub fn bootstrap(state: State<'_, crate::AppState>) -> Result<BootstrapData, String> {
    state.db.bootstrap().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_projects(
    state: State<'_, crate::AppState>,
    include_archived: bool,
) -> Result<Vec<Project>, String> {
    state
        .db
        .list_projects(include_archived)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project(
    state: State<'_, crate::AppState>,
    input: CreateProjectInput,
) -> Result<Project, String> {
    state.db.create_project(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_project(
    state: State<'_, crate::AppState>,
    input: UpdateProjectInput,
) -> Result<Project, String> {
    state.db.update_project(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_project(
    state: State<'_, crate::AppState>,
    id: String,
    archived: bool,
) -> Result<(), String> {
    state
        .db
        .archive_project(&id, archived)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project(state: State<'_, crate::AppState>, id: String) -> Result<(), String> {
    state.db.delete_project(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_tasks(
    state: State<'_, crate::AppState>,
    project_id: Option<String>,
) -> Result<Vec<Task>, String> {
    state
        .db
        .list_tasks(project_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_task(
    state: State<'_, crate::AppState>,
    input: CreateTaskInput,
) -> Result<Task, String> {
    state.db.create_task(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_task(
    state: State<'_, crate::AppState>,
    input: UpdateTaskInput,
) -> Result<Task, String> {
    state.db.update_task(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_task_status(
    state: State<'_, crate::AppState>,
    id: String,
    status: String,
) -> Result<Task, String> {
    state
        .db
        .update_task_status(&id, &status)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_task(state: State<'_, crate::AppState>, id: String) -> Result<(), String> {
    state.db.delete_task(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_prompt_draft(
    state: State<'_, crate::AppState>,
    task_id: String,
    content: String,
) -> Result<Task, String> {
    state
        .db
        .set_task_prompt_draft(&task_id, &content)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn adopt_prompt(
    state: State<'_, crate::AppState>,
    task_id: String,
    content: String,
) -> Result<PromptVersion, String> {
    state
        .db
        .save_prompt_version(&task_id, &content, "adopted")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_prompt_versions(
    state: State<'_, crate::AppState>,
    task_id: String,
) -> Result<Vec<PromptVersion>, String> {
    state
        .db
        .list_prompt_versions(&task_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn optimize_prompt(
    state: State<'_, crate::AppState>,
    input: OptimizePromptInput,
) -> Result<String, String> {
    proxy::optimize_prompt(&state.db, input).await
}

#[tauri::command]
pub fn proxy_settings(state: State<'_, crate::AppState>) -> Result<ProxySettings, String> {
    state.db.proxy_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_proxy_settings(
    state: State<'_, crate::AppState>,
    input: SaveProxySettingsInput,
) -> Result<ProxySettings, String> {
    proxy::save_settings(&state.db, input)
}

#[tauri::command]
pub async fn test_proxy_connection(
    state: State<'_, crate::AppState>,
) -> Result<String, String> {
    proxy::test_connection(&state.db).await
}

#[tauri::command]
pub async fn start_run(
    app: tauri::AppHandle,
    state: State<'_, crate::AppState>,
    input: StartRunInput,
) -> Result<AgentSession, String> {
    state.agents.start_run(app, input).await
}

#[tauri::command]
pub fn list_sessions(state: State<'_, crate::AppState>) -> Result<Vec<AgentSession>, String> {
    state.agents.sessions()
}

#[tauri::command]
pub async fn stop_session(
    state: State<'_, crate::AppState>,
    session_id: String,
) -> Result<(), String> {
    state.agents.stop(&session_id).await
}

#[tauri::command]
pub async fn focus_session(
    state: State<'_, crate::AppState>,
    session_id: String,
) -> Result<(), String> {
    state.agents.focus(&session_id).await
}

#[tauri::command]
pub async fn take_over_session(
    state: State<'_, crate::AppState>,
    session_id: String,
) -> Result<AgentSession, String> {
    state.agents.take_over(&session_id).await
}

#[tauri::command]
pub async fn answer_agent_request(
    state: State<'_, crate::AppState>,
    session_id: String,
    request_id: String,
    result: Value,
) -> Result<(), String> {
    state
        .agents
        .answer_request(&session_id, request_id, result)
        .await
}

#[tauri::command]
pub fn export_data(state: State<'_, crate::AppState>) -> Result<String, String> {
    state.db.backup_json().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_data(state: State<'_, crate::AppState>, content: String) -> Result<(), String> {
    state.db.restore_json(&content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn task_detail(state: State<'_, crate::AppState>, task_id: Option<String>, session_id: Option<String>) -> Result<Value, String> {
    state.db.detail(task_id.as_deref(), session_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn app_health() -> Result<Value, String> {
    Ok(crate::health::snapshot())
}

#[tauri::command]
pub async fn live_sessions(
    state: State<'_, crate::AppState>,
) -> Result<crate::discovery::LiveReport, String> {
    crate::discovery::live_sessions(&state.db).await
}

/// Brings the terminal showing a live pane to the front. The pane is identified
/// by tmux, so this is the one action we can offer without owning the session.
#[tauri::command]
pub async fn focus_tmux_session(session: String, pane: String) -> Result<(), String> {
    crate::terminal::focus_pane(&session, &pane).await.map_err(|error| error.to_string())
}
