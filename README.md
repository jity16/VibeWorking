# Vibe Working

Vibe Working is a local-first macOS desktop app for connecting projects, TODOs, prompts, and real Codex / Claude Code sessions. It keeps the task definition, immutable execution prompt, provider evidence, terminal binding, retry budget, and recap in one SQLite database.

## Development

Prerequisites: macOS, Node 20+, Rust, Tauri 2 CLI, and optional `codex`, `claude`, and `tmux` installations.

```sh
pnpm install
pnpm dev          # frontend only
pnpm tauri dev    # desktop app
pnpm test
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
```

The app stores data under `~/Library/Application Support/Vibe Working/`. API keys are stored in macOS Keychain; exports never include them. The first launch has no seeded or synthetic data.

## Scope and known limits

Codex uses the official app-server protocol when the installed CLI supports it. Claude Code uses a managed tmux session and hooks. External sessions are observable only when identity evidence can be verified. iTerm2 is not available on the development machine, so the first adapter targets Terminal.app/tmux and reports an unbound terminal rather than guessing.

See [research notes](docs/research.md) and [architecture](docs/architecture.md) for source links, compatibility decisions, and untested provider conditions.
