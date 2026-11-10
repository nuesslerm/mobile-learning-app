//! The doubles of the flow tests: the shared test kit, which runs `wf-harness` (or `WF_BIN`) with stubs first on its PATH, the
//! doubles this crate ships (`wf_harness::testing`), and the fixtures of this crate.
pub use wf_harness::testing::*;
pub use wf_testkit::*;

use std::path::PathBuf;

/// The fixtures the tests read (`clis/wf-harness/tests/flow/fixtures`).
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/flow/fixtures")
}
