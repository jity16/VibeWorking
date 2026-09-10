use crate::db::{Database, DbError, DbResult};
use crate::models::{AgentEvent, AgentSession, RetryJob, Run, TerminalBinding};
use chrono::Utc;
use rusqlite::{params, types::Value as SqlValue, OptionalExtension};
use serde_json::{json, Value};

const TABLES: &[&str] = &["projects", "tasks", "prompt_versions", "task_drafts", "agent_sessions", "runs", "turns", "agent_events", "terminal_bindings", "retry_jobs", "recaps", "settings"];

impl Database {
    pub fn begin_run(&self, session: &AgentSession, run: &Run, auto_retry: bool) -> DbResult<()> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            crate::db::ensure_no_live_sessions(&transaction, "task_id", &run.task_id)?;
            transaction.execute("INSERT INTO agent_sessions(id,project_id,task_id,provider,display_name,execution_status,connectivity_status,control_mode,attention,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'starting','reconnecting','automation','none',?6,?6)", params![session.id, session.project_id, session.task_id, session.provider, session.display_name, session.created_at])?;
            transaction.execute("INSERT INTO runs(id,task_id,session_id,prompt_snapshot,status,started_at) VALUES(?1,?2,?3,?4,'starting',?5)", params![run.id, run.task_id, run.session_id, run.prompt_snapshot, run.started_at])?;
            transaction.execute("INSERT INTO session_runtime(session_id,run_id,auto_retry) VALUES(?1,?2,?3)", params![session.id,run.id,auto_retry])?;
            transaction.execute("UPDATE tasks SET status='in_progress',updated_at=?1 WHERE id=?2", params![session.created_at,run.task_id])?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn save_event_state(&self, event: &AgentEvent, session: &AgentSession) -> DbResult<bool> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let changed = transaction.execute("INSERT OR IGNORE INTO agent_events(id,session_id,run_id,provider_event_id,event_type,source,payload,occurred_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![event.id,event.session_id,event.run_id,event.provider_event_id,event.event_type,event.source,event.payload,event.occurred_at])?;
            if changed == 0 { return Ok(false); }
            transaction.execute("UPDATE agent_sessions SET execution_status=?1,connectivity_status=?2,control_mode=?3,attention=?4,current_step=?5,recent_activity=?6,last_activity_at=?7,updated_at=?8 WHERE id=?9", params![session.execution_status,session.connectivity_status,session.control_mode,session.attention,session.current_step,session.recent_activity,session.last_activity_at,session.updated_at,session.id])?;
            transaction.execute("DELETE FROM agent_events WHERE session_id=?1 AND id NOT IN (SELECT id FROM agent_events WHERE session_id=?1 ORDER BY rowid DESC LIMIT 2000)", [&session.id])?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub fn terminal_binding(&self, session_id: &str) -> DbResult<Option<TerminalBinding>> {
        self.with_conn(|conn| Ok(conn.query_row("SELECT id,session_id,target_kind,target_value,process_id,tty,cwd,command_fingerprint,verified_at FROM terminal_bindings WHERE session_id=?1", [session_id], |row| Ok(TerminalBinding { id:row.get(0)?,session_id:row.get(1)?,target_kind:row.get(2)?,target_value:row.get(3)?,process_id:row.get(4)?,tty:row.get(5)?,cwd:row.get(6)?,command_fingerprint:row.get(7)?,verified_at:row.get(8)? })).optional()?))
    }

    pub fn runtime_info(&self, session_id: &str) -> DbResult<Value> {
        self.with_conn(|conn| Ok(conn.query_row("SELECT run_id,socket_path,turn_id,auto_retry,hook_offset FROM session_runtime WHERE session_id=?1", [session_id], |row| Ok(json!({"run_id":row.get::<_,String>(0)?,"socket_path":row.get::<_,Option<String>>(1)?,"turn_id":row.get::<_,Option<String>>(2)?,"auto_retry":row.get::<_,bool>(3)?,"hook_offset":row.get::<_,i64>(4)?}))).optional()?.unwrap_or(Value::Null)))
    }

    pub fn set_runtime_field(&self, session_id: &str, field: &str, value: SqlValue) -> DbResult<()> {
        if !["socket_path","turn_id","auto_retry","hook_offset"].contains(&field) { return Err(DbError::Invalid("invalid runtime field".into())); }
        self.with_conn(|conn| { conn.execute(&format!("UPDATE session_runtime SET {field}=?1 WHERE session_id=?2"), params![value,session_id])?; Ok(()) })
    }

    pub fn save_turn(&self, run_id: &str, turn_id: &str, status: &str, output: Option<&str>) -> DbResult<()> {
        self.with_conn(|conn| { conn.execute("INSERT INTO turns(id,run_id,status,final_output,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(run_id,id) DO UPDATE SET status=excluded.status,final_output=COALESCE(excluded.final_output,turns.final_output),updated_at=excluded.updated_at", params![turn_id,run_id,status,output,Utc::now().to_rfc3339()])?; Ok(()) })
    }

    pub fn detail(&self, task_id: Option<&str>, session_id: Option<&str>) -> DbResult<Value> {
        self.with_conn(|conn| {
            let mut result = json!({});
            for table in ["runs","turns","agent_events","recaps","retry_jobs"] {
                let condition = match table {
                    "runs" => "(?1 IS NULL OR task_id=?1) AND (?2 IS NULL OR session_id=?2)".to_string(),
                    _ => "run_id IN (SELECT id FROM runs WHERE (?1 IS NULL OR task_id=?1) AND (?2 IS NULL OR session_id=?2))".to_string()
                };
                let mut statement = conn.prepare(&format!("SELECT * FROM {table} WHERE {condition} ORDER BY rowid DESC LIMIT 200"))?;
                result[table] = Value::Array(query_values(&mut statement, params![task_id,session_id])?);
            }
            if let Some(task_id) = task_id {
                result["draft"] = conn.query_row("SELECT content FROM task_drafts WHERE task_id=?1", [task_id], |row| row.get::<_,String>(0)).optional()?.map(Value::String).unwrap_or(Value::Null);
            }
            Ok(result)
        })
    }

    pub fn reorder(&self, kind: &str, ids: &[String]) -> DbResult<()> {
        if !["projects","tasks"].contains(&kind) { return Err(DbError::Invalid("invalid sort target".into())); }
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            for (position,id) in ids.iter().enumerate() {
                if transaction.execute(&format!("UPDATE {kind} SET sort_order=?1 WHERE id=?2"), params![position as i64,id])? != 1 { return Err(DbError::NotFound); }
            }
            transaction.commit()?; Ok(())
        })
    }

    pub fn cancel_retries(&self, session_id: &str) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute("UPDATE retry_jobs SET status='cancelled',next_attempt_at=NULL WHERE session_id=?1 AND status IN ('scheduled','sent','sending')", [session_id])?;
            conn.execute("UPDATE session_runtime SET auto_retry=0 WHERE session_id=?1", [session_id])?;
            Ok(())
        })
    }

    pub fn retry_job(&self, session_id: &str) -> DbResult<Option<RetryJob>> {
        self.with_conn(|conn| Ok(conn.query_row("SELECT id,session_id,run_id,overload_event_key,attempts,max_attempts,next_attempt_at,total_deadline_at,status,last_error FROM retry_jobs WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1", [session_id], |row| Ok(RetryJob { id:row.get(0)?,session_id:row.get(1)?,run_id:row.get(2)?,overload_event_key:row.get(3)?,attempts:row.get(4)?,max_attempts:row.get(5)?,next_attempt_at:row.get(6)?,total_deadline_at:row.get(7)?,status:row.get(8)?,last_error:row.get(9)? })).optional()?))
    }

    pub fn save_retry(&self, job: &RetryJob) -> DbResult<()> {
        self.with_conn(|conn| { conn.execute("INSERT INTO retry_jobs(id,session_id,run_id,overload_event_key,attempts,max_attempts,next_attempt_at,total_deadline_at,status,last_error) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET attempts=excluded.attempts,next_attempt_at=excluded.next_attempt_at,status=excluded.status,last_error=excluded.last_error", params![job.id,job.session_id,job.run_id,job.overload_event_key,job.attempts,job.max_attempts,job.next_attempt_at,job.total_deadline_at,job.status,job.last_error])?; Ok(()) })
    }

    pub fn backup_json(&self) -> DbResult<String> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let mut tables = json!({});
            for table in TABLES {
                let mut statement = transaction.prepare(&format!("SELECT * FROM {table}"))?;
                let mut rows = query_values(&mut statement, [])?;
                if *table == "settings" { for row in &mut rows { row["api_key_ref"] = Value::Null; } }
                tables[*table] = Value::Array(rows);
            }
            transaction.commit()?;
            serde_json::to_string_pretty(&json!({"format":"vibe-working","version":2,"tables":tables})).map_err(|error| DbError::Invalid(error.to_string()))
        })
    }

    pub fn restore_json(&self, content: &str) -> DbResult<()> {
        if content.len() > 64 * 1024 * 1024 { return Err(DbError::Invalid("backup exceeds 64 MB".into())); }
        let backup: Value = serde_json::from_str(content).map_err(|error| DbError::Invalid(error.to_string()))?;
        if backup["format"] != "vibe-working" || backup["version"] != 2 { return Err(DbError::Invalid("unsupported backup format".into())); }
        self.with_conn(|conn| {
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;
            if count != 0 { return Err(DbError::Invalid("恢复需要空的工作库；请先导出当前数据并删除项目".into())); }
            let transaction = conn.unchecked_transaction()?;
            for table in TABLES {
                let rows = backup["tables"][*table].as_array().ok_or_else(|| DbError::Invalid(format!("missing table {table}")))?;
                let mut schema = transaction.prepare(&format!("PRAGMA table_info({table})"))?;
                let columns = schema.query_map([], |row| row.get::<_,String>(1))?.collect::<Result<Vec<_>,_>>()?;
                for row in rows {
                    let mut values = Vec::new();
                    for column in &columns {
                        let value = if *table == "settings" && column == "api_key_ref" { &Value::Null } else { row.get(column).ok_or_else(|| DbError::Invalid(format!("missing {table}.{column}")))? };
                        values.push(match value { Value::Null => SqlValue::Null, Value::String(text) => SqlValue::Text(text.clone()), Value::Number(number) => SqlValue::Integer(number.as_i64().ok_or_else(|| DbError::Invalid("invalid number".into()))?), _ => return Err(DbError::Invalid("invalid backup field".into())) });
                    }
                    let placeholders = vec!["?"; columns.len()].join(",");
                    let sql = format!("INSERT {} INTO {table}({}) VALUES({placeholders})", if *table == "settings" {"OR REPLACE"} else {""}, columns.join(","));
                    transaction.execute(&sql, rusqlite::params_from_iter(values))?;
                }
            }
            transaction.execute("UPDATE agent_sessions SET connectivity_status='disconnected',control_mode='human',terminal_bound=0,attention='unverified',execution_status=CASE WHEN execution_status IN ('completed','failed','stopped') THEN execution_status ELSE 'unknown' END", [])?;
            transaction.execute("DELETE FROM terminal_bindings", [])?;
            transaction.execute("UPDATE retry_jobs SET status='cancelled',next_attempt_at=NULL", [])?;
            transaction.commit()?; Ok(())
        })
    }
}

fn query_values(statement: &mut rusqlite::Statement<'_>, parameters: impl rusqlite::Params) -> DbResult<Vec<Value>> {
    let columns = statement.column_names().iter().map(|name| name.to_string()).collect::<Vec<_>>();
    let rows = statement.query_map(parameters, |row| {
        let mut object = serde_json::Map::new();
        for (index,column) in columns.iter().enumerate() {
            let value = match row.get::<_,SqlValue>(index)? { SqlValue::Null => Value::Null, SqlValue::Integer(number) => json!(number), SqlValue::Real(number) => json!(number), SqlValue::Text(text) => Value::String(text), SqlValue::Blob(_) => Value::Null };
            object.insert(column.clone(), value);
        }
        Ok(Value::Object(object))
    })?;
    Ok(rows.collect::<Result<Vec<_>,_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateProjectInput, CreateTaskInput};
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf { std::env::temp_dir().join(format!("vibe-working-{name}-{}.sqlite", std::process::id())) }

    #[test]
    fn backup_round_trip_excludes_secrets_and_keeps_prompt_history() {
        let first_path = temp_path("first"); let second_path = temp_path("second");
        let _ = std::fs::remove_file(&first_path); let _ = std::fs::remove_file(&second_path);
        let first = Database::open(first_path.clone()).unwrap();
        let project = first.create_project(CreateProjectInput{name:"Test".into(),root_path:".".into(),context:None,constraints:None}).unwrap();
        let task = first.create_task(CreateTaskInput{project_id:project.id.clone(),title:"Prompt".into(),original_request:"Do the work".into()}).unwrap();
        first.save_prompt_version(&task.id,"immutable prompt","adopted").unwrap();
        let backup = first.backup_json().unwrap(); assert!(!backup.contains("api_key_ref\":\"")); assert!(backup.contains("immutable prompt"));
        let second = Database::open(second_path.clone()).unwrap(); second.restore_json(&backup).unwrap();
        assert_eq!(second.list_projects(false).unwrap().len(),1); assert_eq!(second.list_tasks(None).unwrap()[0].current_prompt.as_deref(),Some("immutable prompt"));
        drop(first); drop(second); let _ = std::fs::remove_file(first_path); let _ = std::fs::remove_file(second_path);
    }
}
