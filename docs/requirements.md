# First release checklist

## Vertical slices

1. SQLite schema/migrations, project/task CRUD, immutable prompt snapshots.
2. Proxy settings in Keychain plus separate Responses/Chat Completions streaming clients.
3. Codex app-server and Claude tmux adapters with event reduction and terminal bindings.
4. Agent list/detail, human attention, explicit takeover and stop actions.
5. Persisted capacity retry controller and evidence-backed recap.
6. Backup/export, restart recovery, tests and macOS build instructions.

## Deliberate defaults

- App name: Vibe Working.
- New successful Run moves its Task to `review`, never directly to `done`.
- Automatic continue is disabled for observation-only or unverified sessions.
- Default retry budget: five attempts, 30 seconds initial delay, exponential backoff with jitter, five-minute per-attempt cap and fifteen-minute total window.
- Close window leaves child agents alive; Stop is explicit.
- All UI copy is sentence case and localized initially in Chinese/English-neutral short labels.
