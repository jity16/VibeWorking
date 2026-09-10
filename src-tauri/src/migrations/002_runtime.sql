CREATE TABLE task_drafts (
  task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
  content TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE session_runtime (
  session_id TEXT PRIMARY KEY REFERENCES agent_sessions(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  socket_path TEXT,
  turn_id TEXT,
  auto_retry INTEGER NOT NULL DEFAULT 0,
  hook_offset INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE turns (
  id TEXT NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  final_output TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(run_id,id)
);
ALTER TABLE settings ADD COLUMN auth_method TEXT NOT NULL DEFAULT 'bearer';
CREATE UNIQUE INDEX idx_retry_active_session ON retry_jobs(session_id) WHERE status IN ('scheduled','sending','sent');
