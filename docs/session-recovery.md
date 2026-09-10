# Session recovery — 2026-09-10

Source: `01a08922-ef85-7143-ac33-a0cc70e6ea3d`, local rollout and original user request.

The original objective remains a real macOS application linking Project → Task → adopted Prompt → Codex/Claude session → human takeover → recap. Preserve the Tauri 2 / React / Rust / SQLite implementation and all existing data. Never modify local networking, VPN, DNS, routes, firewall, or global agent configuration.

Recovered starting state: research, architecture and requirements documents; 2,616 lines of Rust; no frontend source, dependency lock for JavaScript, or Git commits. Rust compilation fails because the application icon is missing. The existing adapters are incomplete and have not passed a real end-to-end integration.

## Continuation plan

1. Repair transactional persistence, drafts versus adopted prompts, history, migrations, backup/restore and deletion guards.
2. Validate installed CLI protocol schemas; repair session/turn correlation, terminal identity, approval handling and restart reconciliation.
3. Connect the persisted capacity retry policy only to verified input channels; stop on uncertain delivery or human takeover.
4. Complete separate streaming proxy protocols, cancellation and explicit completion detection.
5. Build the project/TODO/Agents/settings UI against the real backend.
6. Run targeted tests, CLI smoke checks and macOS packaging; document measured coverage and remaining limitations.

## Continuation status — 2026-09-10

Steps 1–5 are implemented and building. Step 6 is done for the parts that do not
need model credentials. What closing it out changed:

- A Claude run now actually receives the task prompt. It was previously started
  as a bare interactive session (`let _ = prompt`), so the Project → Task →
  Prompt chain ended at the terminal. The prompt is passed as process argv after
  `--`, which is verifiable and survives multi-line text and leading dashes.
- `RetryController::decide` is the only place that decides whether a queued
  continuation may be sent. `try_retry` previously duplicated the conditions
  inline while `decide` sat unused, so the persisted policy and the code acting
  on it could drift. `RetryDecision` now distinguishes `Send` from
  `Wait`, and `try_retry` re-evaluates the whole policy after every wait instead
  of trusting the snapshot it slept on.
- The app-server event reader shares the real session registry. It used to build
  a throwaway `AgentManager` with an empty runtime map, so anything routed
  through the reader could not see live sessions. Finished runs are now dropped
  from the registry, so Stop reports "not active" honestly rather than trying to
  kill a dead tmux session.
- `applyPatchApproval` / `execCommandApproval` (the server-request spellings the
  installed CLI still emits) are recorded as pending approvals and reduce to
  `approval_required`, instead of falling through to the default arm.
- Removed the unused state enums, `tmux_send`, `tmux_capture` and
  `ProxyError::Cancelled`. Typing into a pane cannot be confirmed to have
  reached the agent, so keeping the helpers around invited a future caller to
  use them for exactly the delivery this design refuses to trust.
- Backup import is reachable from the UI; `restore_data` existed with no way to
  call it.
- Icons are generated and `bundle.icon` is populated, so `pnpm tauri build`
  produces a real `Vibe Working.app`.

Remaining and untested: live model calls, a real overload-triggered retry, and
an approval round trip. All three need network credentials.

## Interface direction

Use a compact native workstation layout: 216px project sidebar, grouped task/session list, unified detail inspector. Use the system macOS sans face for controls and SF Mono for paths and event metadata. Palette: canvas `#f7f8fa`, surface `#ffffff`, text `#202733`, muted `#697586`, action `#365f91`, attention `#98601c`; dark equivalents follow system preference. The distinctive element is a quiet task-to-execution trail in the inspector, backed only by saved prompt versions and actual runs. No statistics dashboard, welcome panels or fabricated activity.

Review: decorative display typography and a hero would obstruct this task manager. Native typography and dense, readable rows follow the user's explicit desktop constraints. Only actual pending human decisions receive an attention marker.
