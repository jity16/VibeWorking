CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL,
  context TEXT NOT NULL DEFAULT '',
  constraints TEXT NOT NULL DEFAULT '',
  archived INTEGER NOT NULL DEFAULT 0,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  original_request TEXT NOT NULL,
  current_prompt TEXT,
  adopted_prompt_version_id TEXT,
  status TEXT NOT NULL CHECK(status IN ('todo','in_progress','review','done')),
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS prompt_versions (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  content TEXT NOT NULL,
  source TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_sessions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
  provider TEXT NOT NULL,
  display_name TEXT NOT NULL,
  provider_session_id TEXT,
  process_id INTEGER,
  execution_status TEXT NOT NULL,
  connectivity_status TEXT NOT NULL,
  control_mode TEXT NOT NULL,
  attention TEXT NOT NULL,
  current_step TEXT,
  recent_activity TEXT,
  last_activity_at TEXT,
  terminal_bound INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  prompt_snapshot TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  final_output TEXT
);

CREATE TABLE IF NOT EXISTS agent_events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
  provider_event_id TEXT,
  event_type TEXT NOT NULL,
  source TEXT NOT NULL,
  payload TEXT NOT NULL,
  occurred_at TEXT NOT NULL,
  UNIQUE(session_id, provider_event_id)
);

CREATE TABLE IF NOT EXISTS terminal_bindings (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL UNIQUE REFERENCES agent_sessions(id) ON DELETE CASCADE,
  target_kind TEXT NOT NULL,
  target_value TEXT NOT NULL,
  process_id INTEGER,
  tty TEXT,
  cwd TEXT NOT NULL,
  command_fingerprint TEXT,
  verified_at TEXT
);

CREATE TABLE IF NOT EXISTS retry_jobs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  overload_event_key TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 5,
  next_attempt_at TEXT,
  total_deadline_at TEXT NOT NULL,
  status TEXT NOT NULL,
  last_error TEXT,
  UNIQUE(session_id, overload_event_key)
);

CREATE TABLE IF NOT EXISTS recaps (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL UNIQUE REFERENCES runs(id) ON DELETE CASCADE,
  outcome TEXT NOT NULL,
  summary TEXT,
  changes TEXT,
  verification TEXT,
  pending TEXT,
  source TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  id INTEGER PRIMARY KEY CHECK(id=1),
  base_url TEXT NOT NULL DEFAULT '',
  protocol TEXT NOT NULL DEFAULT 'responses',
  model TEXT NOT NULL DEFAULT '',
  timeout_seconds INTEGER NOT NULL DEFAULT 90,
  api_key_ref TEXT,
  updated_at TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_tasks_project_status ON tasks(project_id,status,sort_order);
CREATE INDEX IF NOT EXISTS idx_events_session_time ON agent_events(session_id,occurred_at);
CREATE INDEX IF NOT EXISTS idx_runs_task_time ON runs(task_id,started_at);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON agent_sessions(execution_status,attention);

INSERT OR IGNORE INTO settings(id,base_url,protocol,model,timeout_seconds,updated_at) VALUES(1,'','responses','',90,'');
