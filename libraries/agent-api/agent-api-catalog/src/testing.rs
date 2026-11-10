use agent_api_proto::testing::{load, Fixture};
use std::path::Path;

/// The recorded exchanges of this crate's methods.
pub fn fixtures() -> Vec<Fixture> {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}

/// The agent CLIs the executor of the recorded `agents.detect` exchanges has: program and the first line its `--version` prints. The
/// executor has no other, and its own home holds no skill; whoever replays the exchanges sets that up from this list.
pub const FIXTURE_CLIS: [(&str, &str); 2] = [("claude", "2.1.7 (Claude Code)"), ("codex", "codex-cli 0.9.1")];
