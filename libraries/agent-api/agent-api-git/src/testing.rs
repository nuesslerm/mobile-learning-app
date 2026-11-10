use agent_api_proto::testing::{load, Fixture};
use std::path::Path;

/// The recorded exchanges of this crate's methods.
pub fn fixtures() -> Vec<Fixture> {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}

/// The recorded exchanges of the `git.ai` sub-group (`tests/fixtures/ai`): replayed on a service whose model is a double that answers from them.
pub fn ai_fixtures() -> Vec<Fixture> {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ai"))
}
