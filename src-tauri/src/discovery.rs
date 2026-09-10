//! Finds coding-agent sessions that are running right now inside tmux.
//!
//! Identity is the tmux pane, not the conversation. Linking a live pane to a
//! stored transcript would have to be guessed from working directory and
//! recency, and that guess is provably wrong here: one tmux session on this
//! machine holds two `codex` panes in the same directory. A pane is something we
//! can verify, focus, and watch disappear when it closes.
//!
//! Consequently nothing is read from `~/.codex/sessions` or
//! `~/.claude/projects`. Those hold finished conversations, and a finished
//! conversation is not a running session.

use crate::db::Database;
use serde::Serialize;
use std::collections::HashMap;
use tokio::process::Command;

/// Executable names, as reported by `ps comm`. `pane_current_command` is not
/// usable: Claude Code rewrites its process title, so tmux reports it as its
/// version string rather than as `claude`.
const AGENTS: [&str; 2] = ["codex", "claude"];
/// Guards against a cycle in a malformed process table.
const MAX_TREE_DEPTH: usize = 12;
const UNIT: &str = "\u{1f}";

#[derive(Debug, Clone, Serialize)]
pub struct LiveSession {
    pub provider: String,
    pub tmux_session: String,
    pub pane: String,
    pub pid: i64,
    pub cwd: String,
    /// Wall-clock age of the agent process, as `ps` reports it.
    pub uptime: String,
    pub attached: bool,
    /// `None` when no project root contains `cwd`; the UI files those under 未分类.
    pub project_id: Option<String>,
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveReport {
    pub sessions: Vec<LiveSession>,
    pub warnings: Vec<String>,
}

pub async fn live_sessions(db: &Database) -> Result<LiveReport, String> {
    let projects = db
        .list_projects(true)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|project| (project.id, project.name, normalize(&project.root_path)))
        .collect::<Vec<_>>();

    let mut warnings = Vec::new();
    let panes = match list_panes().await {
        Ok(panes) => panes,
        Err(error) => {
            // No tmux server running is the ordinary "nothing is going on" case,
            // not a failure worth reporting.
            if !error.is_empty() {
                warnings.push(error);
            }
            Vec::new()
        }
    };
    let processes = process_table().await.map_err(|error| format!("无法读取进程表：{error}"))?;
    let children = child_index(&processes);

    let mut sessions = Vec::new();
    for pane in panes {
        let Some((pid, provider)) = find_agent(pane.pid, &processes, &children) else { continue };
        let (id, name) = match match_project(&projects, &pane.cwd) {
            Some((id, name)) => (Some(id), Some(name)),
            None => (None, None),
        };
        sessions.push(LiveSession {
            provider,
            tmux_session: pane.session,
            pane: pane.pane,
            pid,
            cwd: pane.cwd,
            uptime: processes.get(&pid).map(|process| process.elapsed.clone()).unwrap_or_default(),
            attached: pane.attached,
            project_id: id,
            project_name: name,
        });
    }
    sessions.sort_by(|left, right| {
        left.project_name
            .is_none()
            .cmp(&right.project_name.is_none())
            .then_with(|| left.tmux_session.cmp(&right.tmux_session))
            .then_with(|| left.pane.cmp(&right.pane))
    });
    Ok(LiveReport { sessions, warnings })
}

// ------------------------------------------------------------ tmux panes

#[derive(Debug, Clone)]
struct Pane {
    session: String,
    pane: String,
    pid: i64,
    cwd: String,
    attached: bool,
}

async fn list_panes() -> Result<Vec<Pane>, String> {
    let tmux = crate::environment::resolve("tmux").ok_or_else(String::new)?;
    let format = ["#{session_name}", "#{pane_id}", "#{pane_pid}", "#{pane_current_path}", "#{session_attached}"].join(UNIT);
    let output = Command::new(tmux)
        .args(["list-panes", "-a", "-F", &format])
        .output()
        .await
        .map_err(|error| format!("无法列出 tmux 窗格：{error}"))?;
    if !output.status.success() {
        // `no server running` simply means there is nothing to show.
        return Err(String::new());
    }
    Ok(parse_panes(&String::from_utf8_lossy(&output.stdout)))
}

/// A unit separator keeps a working directory containing spaces intact.
fn parse_panes(text: &str) -> Vec<Pane> {
    text.lines()
        .filter_map(|line| {
            let fields = line.split(UNIT).collect::<Vec<_>>();
            if fields.len() < 5 {
                return None;
            }
            Some(Pane {
                session: fields[0].to_string(),
                pane: fields[1].to_string(),
                pid: fields[2].trim().parse().ok()?,
                cwd: fields[3].to_string(),
                attached: fields[4].trim() != "0",
            })
        })
        .collect()
}

// -------------------------------------------------------- process table

#[derive(Debug, Clone)]
struct Process {
    parent: i64,
    command: String,
    elapsed: String,
}

async fn process_table() -> Result<HashMap<i64, Process>, String> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,etime=,comm="])
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_processes(&String::from_utf8_lossy(&output.stdout)))
}

/// `comm` is requested last because an executable path may contain spaces:
/// everything after the third column belongs to it.
fn parse_processes(text: &str) -> HashMap<i64, Process> {
    let mut table = HashMap::new();
    for line in text.lines() {
        let Some((pid, parent, elapsed, command)) = split_columns(line) else { continue };
        let Ok(pid) = pid.parse::<i64>() else { continue };
        let Ok(parent) = parent.parse::<i64>() else { continue };
        table.insert(pid, Process { parent, command: command.to_string(), elapsed: elapsed.to_string() });
    }
    table
}

/// `ps` right-aligns its numeric columns with runs of spaces, so the first three
/// fields cannot be taken by splitting on a fixed count — the padding would be
/// counted as fields. Take three tokens, then keep the rest verbatim so a
/// command containing spaces survives.
fn split_columns(line: &str) -> Option<(&str, &str, &str, &str)> {
    let mut rest = line.trim_start();
    let mut fields = [""; 3];
    for slot in fields.iter_mut() {
        let end = rest.find(char::is_whitespace)?;
        *slot = &rest[..end];
        rest = rest[end..].trim_start();
    }
    Some((fields[0], fields[1], fields[2], rest.trim_end()))
}

fn child_index(processes: &HashMap<i64, Process>) -> HashMap<i64, Vec<i64>> {
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for (pid, process) in processes {
        children.entry(process.parent).or_default().push(*pid);
    }
    for list in children.values_mut() {
        list.sort_unstable();
    }
    children
}

/// A pane's own process is usually the shell; the agent runs underneath it.
fn find_agent(
    pane_pid: i64,
    processes: &HashMap<i64, Process>,
    children: &HashMap<i64, Vec<i64>>,
) -> Option<(i64, String)> {
    let mut frontier = vec![(pane_pid, 0usize)];
    let mut seen = Vec::new();
    while let Some((pid, depth)) = frontier.pop() {
        if depth > MAX_TREE_DEPTH || seen.contains(&pid) {
            continue;
        }
        seen.push(pid);
        if let Some(process) = processes.get(&pid) {
            let name = std::path::Path::new(&process.command)
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| process.command.clone());
            if let Some(agent) = AGENTS.iter().find(|agent| **agent == name) {
                return Some((pid, (*agent).to_string()));
            }
        }
        for child in children.get(&pid).into_iter().flatten() {
            frontier.push((*child, depth + 1));
        }
    }
    None
}

// -------------------------------------------------------------- projects

/// Longest matching root wins, so a project nested inside another is not
/// swallowed by its parent.
fn match_project(projects: &[(String, String, String)], cwd: &str) -> Option<(String, String)> {
    let cwd = normalize(cwd);
    projects
        .iter()
        .filter(|(_, _, root)| !root.is_empty() && (cwd == *root || cwd.starts_with(&format!("{root}/"))))
        .max_by_key(|(_, _, root)| root.len())
        .map(|(id, name, _)| (id.clone(), name.clone()))
}

fn normalize(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    std::fs::canonicalize(trimmed)
        .map(|value| value.to_string_lossy().trim_end_matches('/').to_string())
        .unwrap_or_else(|_| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects() -> Vec<(String, String, String)> {
        vec![
            ("outer".into(), "Workspace".into(), "/Users/x/Workspace".into()),
            ("inner".into(), "Tools".into(), "/Users/x/Workspace/Tools".into()),
        ]
    }

    #[test]
    fn the_most_specific_project_claims_a_nested_directory() {
        assert_eq!(match_project(&projects(), "/Users/x/Workspace/Tools/App").unwrap().0, "inner");
        assert_eq!(match_project(&projects(), "/Users/x/Workspace/Other").unwrap().0, "outer");
    }

    #[test]
    fn a_sibling_prefix_is_not_a_match() {
        assert!(match_project(&projects(), "/Users/x/WorkspaceOther").is_none());
    }

    #[test]
    fn an_unowned_directory_stays_unassigned() {
        assert!(match_project(&projects(), "/tmp/scratch").is_none());
    }

    #[test]
    fn panes_survive_a_working_directory_containing_spaces() {
        let line = ["work", "%3", "42", "/Users/x/My Project", "1"].join(UNIT);
        let panes = parse_panes(&line);
        assert_eq!(panes[0].cwd, "/Users/x/My Project");
        assert_eq!(panes[0].pid, 42);
        assert!(panes[0].attached);
    }

    #[test]
    fn a_detached_session_is_still_a_live_session() {
        let line = ["work", "%3", "42", "/tmp", "0"].join(UNIT);
        assert!(!parse_panes(&line)[0].attached);
    }

    #[test]
    fn the_agent_is_found_below_the_pane_shell() {
        // tmux reports the pane's shell; the agent runs underneath it.
        let processes = parse_processes("  100     1 02:10:33 -zsh\n  200   100 01:59:00 codex\n");
        let children = child_index(&processes);
        assert_eq!(find_agent(100, &processes, &children), Some((200, "codex".into())));
    }

    #[test]
    fn a_pane_without_an_agent_is_not_reported() {
        let processes = parse_processes("  100     1 02:10:33 -zsh\n  200   100 01:59:00 node\n");
        let children = child_index(&processes);
        assert!(find_agent(100, &processes, &children).is_none());
    }

    #[test]
    fn an_executable_path_with_spaces_still_resolves_to_its_name() {
        let processes = parse_processes("  100     1 02:10:33 /Applications/My Tools/claude\n");
        let children = child_index(&processes);
        assert_eq!(find_agent(100, &processes, &children), Some((100, "claude".into())));
    }

    #[test]
    fn the_padding_ps_uses_to_align_columns_is_not_read_as_fields() {
        let table = parse_processes("      1     0 31-04:31:40 launchd\n  98131  4210 31-04:31:40 codex\n");
        assert_eq!(table[&98131].parent, 4210);
        assert_eq!(table[&98131].command, "codex");
        assert_eq!(table[&98131].elapsed, "31-04:31:40");
    }

    #[test]
    fn a_parent_cycle_cannot_hang_the_scan() {
        let processes = parse_processes("  100   200 01:00:00 -zsh\n  200   100 01:00:00 -zsh\n");
        let children = child_index(&processes);
        assert!(find_agent(100, &processes, &children).is_none());
    }
}
