//! Scans the tmux sessions actually running on this machine.
//!
//! Ignored by default because the result depends on what is running:
//!
//! ```sh
//! cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
//! ```

use vibe_working_lib::db::Database;
use vibe_working_lib::discovery;
use vibe_working_lib::environment;
use vibe_working_lib::models::CreateProjectInput;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "depends on the tmux sessions running on this machine"]
async fn finds_running_agents_and_files_them_under_a_project() {
    environment::install_login_path();
    let path = std::env::temp_dir().join(format!("vibe-working-live-test-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::open(path.clone()).unwrap();

    let report = discovery::live_sessions(&db).await.unwrap();
    println!("warnings: {:?}", report.warnings);
    println!("{} live agent panes", report.sessions.len());
    for session in &report.sessions {
        println!(
            "  [{}] {}:{} pid={} up={} attached={} project={:?} cwd={}",
            session.provider, session.tmux_session, session.pane, session.pid, session.uptime, session.attached, session.project_name, session.cwd
        );
    }
    assert!(!report.sessions.is_empty(), "this machine is running agents inside tmux");
    assert!(
        report.sessions.iter().all(|session| session.project_id.is_none()),
        "with no projects registered, everything must stay unassigned"
    );

    // Registering a project over one of those directories must claim it, and
    // must not claim any of the others.
    let claimed = report.sessions[0].cwd.clone();
    db.create_project(CreateProjectInput { name: "被认领".into(), root_path: claimed.clone(), context: None, constraints: None }).unwrap();
    let after = discovery::live_sessions(&db).await.unwrap();
    let claimed_rows = after.sessions.iter().filter(|session| session.project_name.as_deref() == Some("被认领")).count();
    assert!(claimed_rows > 0, "the project rooted at {claimed} should claim its panes");
    assert!(
        after.sessions.iter().filter(|session| !session.cwd.starts_with(&claimed)).all(|session| session.project_id.is_none()),
        "a project must not claim panes outside its root"
    );

    drop(db);
    let _ = std::fs::remove_file(path);
}
