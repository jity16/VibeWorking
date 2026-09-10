use crate::models::*;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("record not found")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("database path error: {0}")]
    Path(String),
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Clone)]
pub struct Database {
    pub path: PathBuf,
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open_default() -> DbResult<Self> {
        let base = dirs::data_dir().ok_or_else(|| DbError::Path("Application Support directory unavailable".into()))?;
        let app_dir = base.join("Vibe Working");
        std::fs::create_dir_all(&app_dir).map_err(|e| DbError::Path(e.to_string()))?;
        Self::open(app_dir.join("vibe-working.sqlite3"))
    }

    pub fn open(path: PathBuf) -> DbResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| DbError::Path(e.to_string()))?;
        }
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000i64)?;
        let database = Self { path, conn: Arc::new(Mutex::new(conn)) };
        database.migrate()?;
        Ok(database)
    }

    pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        let conn = self.conn.lock().map_err(|_| DbError::Invalid("database lock poisoned".into()))?;
        f(&conn)
    }

    fn migrate(&self) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
            )?;
            let applied: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )?;
            if applied > 2 { return Err(DbError::Invalid("database was created by a newer app version".into())); }
            for (version, sql) in [(1, include_str!("migrations/001_initial.sql")), (2, include_str!("migrations/002_runtime.sql"))] {
                if applied < version {
                    let transaction = conn.unchecked_transaction()?;
                    transaction.execute_batch(sql)?;
                    transaction.execute("INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)", params![version, now()])?;
                    transaction.commit()?;
                }
            }
            Ok(())
        })
    }

    pub fn bootstrap(&self) -> DbResult<BootstrapData> {
        Ok(BootstrapData {
            projects: self.list_projects(true)?,
            tasks: self.list_tasks(None)?,
            sessions: self.list_sessions()?,
            settings: self.proxy_settings()?,
        })
    }

    pub fn list_projects(&self, include_archived: bool) -> DbResult<Vec<Project>> {
        self.with_conn(|conn| {
            let sql = if include_archived {
                "SELECT id,name,root_path,context,constraints,archived,sort_order,created_at,updated_at FROM projects ORDER BY sort_order, name"
            } else {
                "SELECT id,name,root_path,context,constraints,archived,sort_order,created_at,updated_at FROM projects WHERE archived=0 ORDER BY sort_order, name"
            };
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map([], project_from_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        })
    }

    pub fn create_project(&self, input: CreateProjectInput) -> DbResult<Project> {
        let name = clean_required(&input.name, "project name")?;
        let root = clean_required(&input.root_path, "project path")?;
        let id = Uuid::new_v4().to_string();
        let now = now();
        self.with_conn(|conn| {
            let next: i64 = conn.query_row("SELECT COALESCE(MAX(sort_order), -1) + 1 FROM projects", [], |r| r.get(0))?;
            conn.execute(
                "INSERT INTO projects(id,name,root_path,context,constraints,archived,sort_order,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,0,?6,?7,?7)",
                params![id, name, root, input.context.unwrap_or_default(), input.constraints.unwrap_or_default(), next, now],
            )?;
            self.get_project_locked(conn, &id)
        })
    }

    pub fn update_project(&self, input: UpdateProjectInput) -> DbResult<Project> {
        let name = clean_required(&input.name, "project name")?;
        let root = clean_required(&input.root_path, "project path")?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE projects SET name=?1,root_path=?2,context=?3,constraints=?4,archived=?5,updated_at=?6 WHERE id=?7",
                params![name, root, input.context, input.constraints, input.archived as i64, now(), input.id],
            )?;
            if changed == 0 { return Err(DbError::NotFound); }
            self.get_project_locked(conn, &input.id)
        })
    }

    pub fn archive_project(&self, id: &str, archived: bool) -> DbResult<()> {
        self.with_conn(|conn| {
            let changed = conn.execute("UPDATE projects SET archived=?1,updated_at=?2 WHERE id=?3", params![archived as i64, now(), id])?;
            if changed == 0 { Err(DbError::NotFound) } else { Ok(()) }
        })
    }

    pub fn delete_project(&self, id: &str) -> DbResult<()> {
        self.with_conn(|conn| {
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE id=?1", [id], |r| r.get(0))?;
            if count == 0 { return Err(DbError::NotFound); }
            ensure_no_live_sessions(conn, "project_id", id)?;
            conn.execute("DELETE FROM projects WHERE id=?1", [id])?;
            Ok(())
        })
    }

    pub fn list_tasks(&self, project_id: Option<&str>) -> DbResult<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT t.id,t.project_id,t.title,t.original_request,t.current_prompt,t.adopted_prompt_version_id,t.status,t.sort_order,t.created_at,t.updated_at,
                        r.status, c.summary
                 FROM tasks t
                 LEFT JOIN runs r ON r.id=(SELECT id FROM runs WHERE task_id=t.id ORDER BY started_at DESC LIMIT 1)
                 LEFT JOIN recaps c ON c.run_id=r.id
                 WHERE (?1 IS NULL OR t.project_id=?1)
                 ORDER BY t.status='done', t.sort_order, t.updated_at DESC",
            )?;
            let rows = stmt.query_map([project_id], task_from_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        })
    }

    pub fn create_task(&self, input: CreateTaskInput) -> DbResult<Task> {
        let title = clean_required(&input.title, "task title")?;
        let request = clean_required(&input.original_request, "task request")?;
        let id = Uuid::new_v4().to_string();
        let now = now();
        self.with_conn(|conn| {
            ensure_project(conn, &input.project_id)?;
            let next: i64 = conn.query_row("SELECT COALESCE(MAX(sort_order), -1) + 1 FROM tasks WHERE project_id=?1", [&input.project_id], |r| r.get(0))?;
            conn.execute(
                "INSERT INTO tasks(id,project_id,title,original_request,status,sort_order,created_at,updated_at) VALUES(?1,?2,?3,?4,'todo',?5,?6,?6)",
                params![id, input.project_id, title, request, next, now],
            )?;
            self.get_task_locked(conn, &id)
        })
    }

    pub fn update_task(&self, input: UpdateTaskInput) -> DbResult<Task> {
        let title = clean_required(&input.title, "task title")?;
        let request = clean_required(&input.original_request, "task request")?;
        validate_task_status(&input.status)?;
        self.with_conn(|conn| {
            ensure_project(conn, &input.project_id)?;
            let previous = self.get_task_locked(conn, &input.id)?;
            if previous.project_id != input.project_id { ensure_no_live_sessions(conn, "task_id", &input.id)?; }
            let changed = conn.execute(
                "UPDATE tasks SET project_id=?1,title=?2,original_request=?3,status=?4,updated_at=?5 WHERE id=?6",
                params![input.project_id, title, request, input.status, now(), input.id],
            )?;
            if changed == 0 { return Err(DbError::NotFound); }
            self.get_task_locked(conn, &input.id)
        })
    }

    pub fn update_task_status(&self, id: &str, status: &str) -> DbResult<Task> {
        validate_task_status(status)?;
        self.with_conn(|conn| {
            let changed = conn.execute("UPDATE tasks SET status=?1,updated_at=?2 WHERE id=?3", params![status, now(), id])?;
            if changed == 0 { return Err(DbError::NotFound); }
            self.get_task_locked(conn, id)
        })
    }

    pub fn delete_task(&self, id: &str) -> DbResult<()> {
        self.with_conn(|conn| {
            ensure_no_live_sessions(conn, "task_id", id)?;
            let changed = conn.execute("DELETE FROM tasks WHERE id=?1", [id])?;
            if changed == 0 { Err(DbError::NotFound) } else { Ok(()) }
        })
    }

    pub fn save_prompt_version(&self, task_id: &str, content: &str, source: &str) -> DbResult<PromptVersion> {
        let content = clean_required(content, "prompt")?;
        let id = Uuid::new_v4().to_string();
        let now = now();
        self.with_conn(|conn| {
            ensure_task(conn, task_id)?;
            let transaction = conn.unchecked_transaction()?;
            transaction.execute("INSERT INTO prompt_versions(id,task_id,content,source,created_at) VALUES(?1,?2,?3,?4,?5)", params![id, task_id, content, source, now])?;
            transaction.execute("UPDATE tasks SET current_prompt=?1,adopted_prompt_version_id=?2,updated_at=?3 WHERE id=?4", params![content, id, now, task_id])?;
            transaction.execute("DELETE FROM task_drafts WHERE task_id=?1", [task_id])?;
            transaction.commit()?;
            conn.query_row("SELECT id,task_id,content,source,created_at FROM prompt_versions WHERE id=?1", [&id], prompt_from_row).map_err(DbError::from)
        })
    }

    pub fn list_prompt_versions(&self, task_id: &str) -> DbResult<Vec<PromptVersion>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id,task_id,content,source,created_at FROM prompt_versions WHERE task_id=?1 ORDER BY created_at DESC")?;
            let rows = stmt.query_map([task_id], prompt_from_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        })
    }

    pub fn get_task(&self, id: &str) -> DbResult<Task> {
        self.with_conn(|conn| self.get_task_locked(conn, id))
    }

    pub fn get_project(&self, id: &str) -> DbResult<Project> {
        self.with_conn(|conn| self.get_project_locked(conn, id))
    }

    fn get_project_locked(&self, conn: &Connection, id: &str) -> DbResult<Project> {
        conn.query_row("SELECT id,name,root_path,context,constraints,archived,sort_order,created_at,updated_at FROM projects WHERE id=?1", [id], project_from_row).optional()?.ok_or(DbError::NotFound)
    }

    fn get_task_locked(&self, conn: &Connection, id: &str) -> DbResult<Task> {
        conn.query_row(
            "SELECT t.id,t.project_id,t.title,t.original_request,t.current_prompt,t.adopted_prompt_version_id,t.status,t.sort_order,t.created_at,t.updated_at,r.status,c.summary FROM tasks t LEFT JOIN runs r ON r.id=(SELECT id FROM runs WHERE task_id=t.id ORDER BY started_at DESC LIMIT 1) LEFT JOIN recaps c ON c.run_id=r.id WHERE t.id=?1",
            [id], task_from_row,
        ).optional()?.ok_or(DbError::NotFound)
    }

    pub fn list_sessions(&self) -> DbResult<Vec<AgentSession>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id,project_id,task_id,provider,display_name,provider_session_id,process_id,execution_status,connectivity_status,control_mode,attention,current_step,recent_activity,last_activity_at,terminal_bound,created_at,updated_at FROM agent_sessions ORDER BY updated_at DESC")?;
            let rows = stmt.query_map([], session_from_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        })
    }

    pub fn mark_sessions_unverified(&self) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE agent_sessions SET execution_status=CASE WHEN execution_status IN ('running','starting','waiting_input','backoff') THEN 'unknown' ELSE execution_status END, connectivity_status=CASE WHEN connectivity_status='connected' THEN 'disconnected' ELSE connectivity_status END, attention=CASE WHEN execution_status IN ('running','starting','waiting_input','backoff') THEN 'unverified' ELSE attention END, updated_at=?1 WHERE execution_status IN ('running','starting','waiting_input','backoff') OR connectivity_status='connected'",
                [now()],
            )?;
            Ok(())
        })
    }

    pub fn set_task_prompt_draft(&self, task_id: &str, content: &str) -> DbResult<Task> {
        if content.len() > 200_000 { return Err(DbError::Invalid("prompt is too long".into())); }
        self.with_conn(|conn| {
            ensure_task(conn, task_id)?;
            conn.execute("INSERT INTO task_drafts(task_id,content,updated_at) VALUES(?1,?2,?3) ON CONFLICT(task_id) DO UPDATE SET content=excluded.content,updated_at=excluded.updated_at", params![task_id, content, now()])?;
            self.get_task_locked(conn, task_id)
        })
    }

    pub fn insert_session(&self, session: &AgentSession) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute("INSERT INTO agent_sessions(id,project_id,task_id,provider,display_name,provider_session_id,process_id,execution_status,connectivity_status,control_mode,attention,current_step,recent_activity,last_activity_at,terminal_bound,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)", params![session.id,session.project_id,session.task_id,session.provider,session.display_name,session.provider_session_id,session.process_id,session.execution_status,session.connectivity_status,session.control_mode,session.attention,session.current_step,session.recent_activity,session.last_activity_at,session.terminal_bound as i64,session.created_at,session.updated_at])?;
            Ok(())
        })
    }

    pub fn update_session_state(&self, session: &AgentSession) -> DbResult<()> {
        self.with_conn(|conn| {
            let changed = conn.execute("UPDATE agent_sessions SET provider_session_id=?1,process_id=?2,execution_status=?3,connectivity_status=?4,control_mode=?5,attention=?6,current_step=?7,recent_activity=?8,last_activity_at=?9,terminal_bound=?10,updated_at=?11 WHERE id=?12", params![session.provider_session_id,session.process_id,session.execution_status,session.connectivity_status,session.control_mode,session.attention,session.current_step,session.recent_activity,session.last_activity_at,session.terminal_bound as i64,session.updated_at,session.id])?;
            if changed == 0 { Err(DbError::NotFound) } else { Ok(()) }
        })
    }

    pub fn insert_run(&self, run: &Run) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute("INSERT INTO runs(id,task_id,session_id,prompt_snapshot,status,started_at,ended_at,final_output) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![run.id,run.task_id,run.session_id,run.prompt_snapshot,run.status,run.started_at,run.ended_at,run.final_output])?;
            Ok(())
        })
    }

    pub fn update_run(&self, run: &Run) -> DbResult<()> {
        self.with_conn(|conn| {
            let changed = conn.execute("UPDATE runs SET status=?1,ended_at=?2,final_output=?3 WHERE id=?4", params![run.status,run.ended_at,run.final_output,run.id])?;
            if changed == 0 { Err(DbError::NotFound) } else { Ok(()) }
        })
    }

    pub fn get_run(&self, id: &str) -> DbResult<Run> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id,task_id,session_id,prompt_snapshot,status,started_at,ended_at,final_output FROM runs WHERE id=?1",
                [id],
                |row| Ok(Run { id: row.get(0)?, task_id: row.get(1)?, session_id: row.get(2)?, prompt_snapshot: row.get(3)?, status: row.get(4)?, started_at: row.get(5)?, ended_at: row.get(6)?, final_output: row.get(7)? }),
            ).optional()?.ok_or(DbError::NotFound)
        })
    }

    pub fn insert_event(&self, event: &AgentEvent) -> DbResult<bool> {
        self.with_conn(|conn| {
            let changed = conn.execute("INSERT OR IGNORE INTO agent_events(id,session_id,run_id,provider_event_id,event_type,source,payload,occurred_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![event.id,event.session_id,event.run_id,event.provider_event_id,event.event_type,event.source,event.payload,event.occurred_at])?;
            Ok(changed > 0)
        })
    }

    pub fn upsert_terminal_binding(&self, binding: &TerminalBinding) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute("INSERT INTO terminal_bindings(id,session_id,target_kind,target_value,process_id,tty,cwd,command_fingerprint,verified_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(session_id) DO UPDATE SET target_kind=excluded.target_kind,target_value=excluded.target_value,process_id=excluded.process_id,tty=excluded.tty,cwd=excluded.cwd,command_fingerprint=excluded.command_fingerprint,verified_at=excluded.verified_at", params![binding.id,binding.session_id,binding.target_kind,binding.target_value,binding.process_id,binding.tty,binding.cwd,binding.command_fingerprint,binding.verified_at])?;
            Ok(())
        })
    }

    pub fn insert_recap_if_absent(&self, recap: &Recap) -> DbResult<bool> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "INSERT OR IGNORE INTO recaps(id,run_id,outcome,summary,changes,verification,pending,source,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![recap.id, recap.run_id, recap.outcome, recap.summary, recap.changes, recap.verification, recap.pending, recap.source, recap.created_at],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn proxy_settings(&self) -> DbResult<ProxySettings> {
        self.with_conn(|conn| {
            let row = conn.query_row("SELECT base_url,protocol,model,timeout_seconds,api_key_ref FROM settings WHERE id=1", [], |r| Ok(ProxySettings { base_url: r.get(0)?, protocol: r.get(1)?, model: r.get(2)?, timeout_seconds: r.get::<_, i64>(3)? as u64, api_key_ref: r.get(4)?, has_api_key: false }))?;
            Ok(row)
        })
    }

    pub fn save_proxy_settings(&self, settings: &ProxySettings) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute("INSERT INTO settings(id,base_url,protocol,model,timeout_seconds,api_key_ref,updated_at) VALUES(1,?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET base_url=excluded.base_url,protocol=excluded.protocol,model=excluded.model,timeout_seconds=excluded.timeout_seconds,api_key_ref=excluded.api_key_ref,updated_at=excluded.updated_at", params![settings.base_url,settings.protocol,settings.model,settings.timeout_seconds as i64,settings.api_key_ref,now()])?;
            Ok(())
        })
    }

    pub fn export_json(&self) -> DbResult<String> {
        #[derive(Serialize)]
        struct Export<'a> { projects: &'a [Project], tasks: Vec<Task>, sessions: Vec<AgentSession>, settings: ProxySettings }
        let projects = self.list_projects(true)?;
        let tasks = self.list_tasks(None)?;
        let sessions = self.list_sessions()?;
        let mut settings = self.proxy_settings()?;
        settings.api_key_ref = None;
        settings.has_api_key = false;
        serde_json::to_string_pretty(&Export { projects: &projects, tasks, sessions, settings }).map_err(|e| DbError::Invalid(e.to_string()))
    }
}

fn now() -> String { Utc::now().to_rfc3339() }

fn clean_required(value: &str, field: &str) -> DbResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { return Err(DbError::Invalid(format!("{field} cannot be empty"))); }
    if trimmed.len() > 200_000 { return Err(DbError::Invalid(format!("{field} is too long"))); }
    Ok(trimmed.to_string())
}

fn ensure_project(conn: &Connection, id: &str) -> DbResult<()> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)", [id], |r| r.get(0))?;
    if exists { Ok(()) } else { Err(DbError::Invalid("project does not exist".into())) }
}

fn ensure_task(conn: &Connection, id: &str) -> DbResult<()> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)", [id], |r| r.get(0))?;
    if exists { Ok(()) } else { Err(DbError::Invalid("task does not exist".into())) }
}

pub(crate) fn ensure_no_live_sessions(conn: &Connection, field: &str, id: &str) -> DbResult<()> {
    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM agent_sessions WHERE {field}=?1 AND (execution_status NOT IN ('completed','failed','stopped') OR terminal_bound=1)"), [id], |row| row.get(0))?;
    if count > 0 { return Err(DbError::Invalid("请先停止并解除关联的 Agent 会话，再移动或删除记录".into())); }
    Ok(())
}

fn validate_task_status(status: &str) -> DbResult<()> {
    match status { "todo" | "in_progress" | "review" | "done" => Ok(()), _ => Err(DbError::Invalid("unknown task status".into())) }
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project { id: row.get(0)?, name: row.get(1)?, root_path: row.get(2)?, context: row.get(3)?, constraints: row.get(4)?, archived: row.get::<_, i64>(5)? != 0, sort_order: row.get(6)?, created_at: row.get(7)?, updated_at: row.get(8)? })
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task { id: row.get(0)?, project_id: row.get(1)?, title: row.get(2)?, original_request: row.get(3)?, current_prompt: row.get(4)?, adopted_prompt_version_id: row.get(5)?, status: row.get(6)?, sort_order: row.get(7)?, created_at: row.get(8)?, updated_at: row.get(9)?, latest_run_status: row.get(10)?, latest_recap: row.get(11)? })
}

fn prompt_from_row(row: &Row<'_>) -> rusqlite::Result<PromptVersion> {
    Ok(PromptVersion { id: row.get(0)?, task_id: row.get(1)?, content: row.get(2)?, source: row.get(3)?, created_at: row.get(4)? })
}

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<AgentSession> {
    Ok(AgentSession { id: row.get(0)?, project_id: row.get(1)?, task_id: row.get(2)?, provider: row.get(3)?, display_name: row.get(4)?, provider_session_id: row.get(5)?, process_id: row.get(6)?, execution_status: row.get(7)?, connectivity_status: row.get(8)?, control_mode: row.get(9)?, attention: row.get(10)?, current_step: row.get(11)?, recent_activity: row.get(12)?, last_activity_at: row.get(13)?, terminal_bound: row.get::<_, i64>(14)? != 0, created_at: row.get(15)?, updated_at: row.get(16)? })
}
