//! Reproduces the environment a Finder-launched bundle actually gets.
//!
//! This lives in its own integration binary because it mutates `PATH` for the
//! whole process. Ignored by default:
//!
//! ```sh
//! cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
//! ```

use vibe_working_lib::environment;

/// launchd hands a GUI process this and nothing else — no shell configuration is
/// read, so none of the agent binaries are reachable.
const LAUNCHD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

#[test]
#[ignore = "mutates PATH; expects codex and tmux installed on this machine"]
fn recovers_the_agent_binaries_from_a_launchd_path() {
    std::env::set_var("PATH", LAUNCHD_PATH);
    assert!(
        environment::resolve("codex").is_none(),
        "precondition: codex must not be reachable on the bare launchd PATH, or this proves nothing"
    );

    environment::install_login_path();

    assert!(environment::resolve("codex").is_some(), "codex should be reachable after recovering the login shell PATH");
    assert!(environment::resolve("tmux").is_some(), "tmux should be reachable after recovering the login shell PATH");
    // Nothing the process already had may be dropped.
    let path = std::env::var("PATH").unwrap();
    for entry in LAUNCHD_PATH.split(':') {
        assert!(path.split(':').any(|value| value == entry), "{entry} was dropped from PATH");
    }
}
