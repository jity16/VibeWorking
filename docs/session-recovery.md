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

## Interface direction

Use a compact native workstation layout: 216px project sidebar, grouped task/session list, unified detail inspector. Use the system macOS sans face for controls and SF Mono for paths and event metadata. Palette: canvas `#f7f8fa`, surface `#ffffff`, text `#202733`, muted `#697586`, action `#365f91`, attention `#98601c`; dark equivalents follow system preference. The distinctive element is a quiet task-to-execution trail in the inspector, backed only by saved prompt versions and actual runs. No statistics dashboard, welcome panels or fabricated activity.

Review: decorative display typography and a hero would obstruct this task manager. Native typography and dense, readable rows follow the user's explicit desktop constraints. Only actual pending human decisions receive an attention marker.
