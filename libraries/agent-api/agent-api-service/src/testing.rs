use agent_api_proto::testing::{load, Fixture};
use std::path::Path;

/// The recorded exchanges of this crate's methods.
pub fn fixtures() -> Vec<Fixture> {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}

/// The recorded exchanges of a sub-group kept apart (`tests/fixtures/<folder>/<method>.json`, folder `notifications` or `panes`): each file is
/// replayed on a service of its own, because what one leaves behind changes what another answers.
pub fn group_fixtures(folder: &str) -> Vec<Fixture> {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(folder))
}
