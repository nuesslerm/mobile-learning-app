use agent_api_proto::testing::{load, Fixture};
use std::path::Path;

/// The recorded exchanges of this crate's methods.
pub fn fixtures() -> Vec<Fixture> {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}
