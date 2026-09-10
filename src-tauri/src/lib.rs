mod agents;
mod commands;
pub mod db;
pub mod discovery;
pub mod environment;
mod health;
pub mod models;
mod persistence;
mod proxy;
mod retry;
mod state;
pub mod terminal;

use agents::AgentManager;
use db::Database;

pub struct AppState {
    pub db: Database,
    pub agents: AgentManager,
}

pub fn run() {
    // Before anything spawns a process: a Finder-launched bundle starts with a
    // PATH that contains none of the agents this app drives.
    environment::install_login_path();
    tracing_subscriber::fmt()
        .with_env_filter("vibe_working=info")
        .with_target(false)
        .try_init()
        .ok();
    let db = Database::open_default().expect("Vibe Working database could not be opened");
    db.mark_sessions_unverified()
        .expect("Vibe Working database recovery failed");
    let agents = AgentManager::new(db.clone());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { db, agents })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::list_projects,
            commands::create_project,
            commands::update_project,
            commands::archive_project,
            commands::delete_project,
            commands::list_tasks,
            commands::create_task,
            commands::update_task,
            commands::update_task_status,
            commands::delete_task,
            commands::save_prompt_draft,
            commands::adopt_prompt,
            commands::list_prompt_versions,
            commands::optimize_prompt,
            commands::proxy_settings,
            commands::save_proxy_settings,
            commands::test_proxy_connection,
            commands::start_run,
            commands::list_sessions,
            commands::stop_session,
            commands::focus_session,
            commands::take_over_session,
            commands::answer_agent_request,
            commands::export_data,
            commands::restore_data,
            commands::task_detail,
            commands::app_health,
            commands::live_sessions,
            commands::focus_tmux_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vibe Working");
}
