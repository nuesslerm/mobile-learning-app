use exec_proto::testing::{load, Fixture};
use std::path::Path;

/// The recorded exchanges of this family's actions.
pub fn fixtures() -> Vec<Fixture> {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}
