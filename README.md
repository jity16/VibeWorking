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
pnpm tauri build   # produces src-tauri/target/release/bundle/macos/Vibe Working.app
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored   # live codex app-server handshake
```

The ignored test starts the installed `codex app-server` on a Unix socket and
runs the real `initialize` / `thread/start` handshake. It stops before
`turn/start`, so it never submits anything to a model.

The app stores data under `~/Library/Application Support/Vibe Working/`. API keys are stored in macOS Keychain; exports never include them. The first launch has no seeded or synthetic data.

## Scope and known limits

Codex uses the official app-server protocol when the installed CLI supports it. Claude Code uses a managed tmux session and hooks, and receives the task prompt as process argv. External sessions are observable only when identity evidence can be verified. iTerm2 is not available on the development machine, so the first adapter targets Terminal.app/tmux and reports an unbound terminal rather than guessing.

Automatic continuation after a capacity failure is Codex-only: it is sent over the app-server RPC channel, where delivery is confirmed. There is deliberately no `tmux send-keys` path, because typing into a pane cannot be confirmed to have reached the agent. Live model calls, a real overload retry, and an approval round trip are still untested — they need network credentials.

See [research notes](docs/research.md) and [architecture](docs/architecture.md) for source links, compatibility decisions, and untested provider conditions.
