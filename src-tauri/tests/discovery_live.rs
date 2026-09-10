//! Runs discovery against the sessions actually present on this machine.
//!
//! Ignored by default because the result depends on local state:
//!
//! ```sh
//! cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
//! ```

use vibe_working_lib::db::Database;
use vibe_working_lib::discovery;
use vibe_working_lib::models::CreateProjectInput;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "depends on the Codex/Claude sessions stored on this machine"]
async fn finds_local_sessions_and_files_them_under_a_project() {
    let path = std::env::temp_dir().join(format!("vibe-working-discovery-test-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::open(path.clone()).unwrap();

    // A project rooted at this checkout should claim the sessions recorded here.
    let root = std::env::current_dir().unwrap().parent().unwrap().to_string_lossy().to_string();
    db.create_project(CreateProjectInput {
        name: "本仓库".into(),
        root_path: root.clone(),
        context: None,
        constraints: None,
    })
    .unwrap();

    let report = discovery::discover(&db).await.unwrap();
    println!("warnings: {:?}", report.warnings);
    let codex = report.sessions.iter().filter(|session| session.provider == "codex").count();
    let claude = report.sessions.iter().filter(|session| session.provider == "claude").count();
    let unassigned = report.sessions.iter().filter(|session| session.project_id.is_none()).count();
    println!("discovered {} sessions: {codex} codex, {claude} claude, {unassigned} unassigned", report.sessions.len());
    for session in report.sessions.iter().take(8) {
        println!(
            "  [{}] {} | project={:?} | {}",
            session.provider,
            session.cwd,
            session.project_name,
            session.title.chars().take(50).collect::<String>()
        );
    }

    assert!(!report.sessions.is_empty(), "this machine has stored sessions, so discovery should return some");
    assert!(codex > 0, "codex threads should be listed through the app-server");
    assert!(claude > 0, "claude transcripts should be read from ~/.claude/projects");
    assert!(
        report.sessions.iter().any(|session| session.project_name.as_deref() == Some("本仓库")),
        "sessions under {root} should be filed under the project rooted there"
    );
    assert!(
        report.sessions.iter().any(|session| session.project_id.is_none()),
        "sessions outside every project root should stay unassigned"
    );
    // Newest first.
    let mut sorted = report.sessions.clone();
    sorted.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    assert_eq!(sorted[0].session_id, report.sessions[0].session_id);

    drop(db);
    let _ = std::fs::remove_file(path);
}
