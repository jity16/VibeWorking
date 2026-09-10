# Architecture

## Runtime boundaries

```text
React UI ──invoke/events──> Tauri command layer (Rust)
                              ├─ SQLite repository + migrations
                              ├─ PromptService / ProxyClient
                              ├─ SessionRegistry + state reducer
                              │    ├─ CodexAdapter (app-server Unix WS)
                              │    └─ ClaudeCodeAdapter (tmux + hooks)
                              ├─ TerminalAdapter (tmux / Terminal.app)
                              ├─ RetryController (persisted jobs)
                              └─ RecapService
```

The UI never owns business data or API keys. Tauri commands validate IDs and paths; agent processes receive argument arrays. SQLite is the source of truth. Keychain stores secret bytes under a service/account pair; the database stores only the reference.

## Domain model

- `Project`: name, root directory, context, constraints, archive flag.
- `Task`: project, title, original request, adopted prompt version, lifecycle status and sort position.
- `PromptVersion`: immutable text and source (`original`, `optimized`, `edited`); a Run stores a snapshot.
- `AgentSession`: durable provider session identity and capability snapshot.
- `Run`: one execution attempt for one immutable prompt snapshot; a session may contain many runs.
- `Turn`: provider turn, status and final output.
- `AgentEvent`: deduplicated raw event with source and sequence evidence.
- `TerminalBinding`: provider/session/process/pane/TTY identity; no PID-only lookup.
- `RetryJob`: one overload event, persisted budget and cancellation state.
- `Recap`: evidence-backed summary attached to one Run, unique by end event.

Execution, connectivity, control and attention are stored as independent dimensions. A reducer is pure and covered by unit tests; UI labels are derived from the reduced state rather than booleans.

## Recovery rules

On launch, rows with `running` execution are marked `unknown` until the adapter verifies process identity and replays/resumes the provider session. Retry jobs are loaded with their remaining budget; task/session changes cancel jobs before any send. Closing the window does not stop children; an explicit Stop command does.

## Implementation boundary for v1

Codex hosted runs use the app-server Unix socket and can be attached by `codex --remote`. Claude hosted runs use tmux and hooks. External sessions are discovery-only unless all identity evidence matches. iTerm2 is a future adapter; Terminal.app support is best-effort and never used as a generic focus/input target.
