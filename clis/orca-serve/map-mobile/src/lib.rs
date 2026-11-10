//! Orca methods a phone uses to fetch its web page from the host (`mobileWeb.bundle.*`) and to list the servers the host relays to
//! (`mobileRelay.hosts.*`). The values and error codes mirror `references/orca/src/main/runtime/rpc/methods` at the pinned commit.
mod bundle;
mod manifest;
mod relay;
mod window;

#[cfg(feature = "testing")]
pub mod testing;

pub use bundle::{BundleSource, MobileWebArea, NoBundle};
pub use relay::RelayArea;

pub const MANIFEST: &str = "mobileWeb.bundle.manifest";
pub const CHUNK: &str = "mobileWeb.bundle.chunk";
pub const RANGE: &str = "mobileWeb.bundle.range";
pub const HOSTS_LIST: &str = "mobileRelay.hosts.list";
pub const HOSTS_WORKTREES: &str = "mobileRelay.hosts.worktrees";

/// `MOBILE_WEB_BUNDLE_CAPABILITY`: advertised only while a bundle is held.
pub const WEB_BUNDLE_CAPABILITY: &str = "mobileWeb.bundle.v1";
/// `MOBILE_WEB_BUNDLE_CHUNK_BYTES`: the grid of the chunk and range windows.
pub const WINDOW_BYTES: usize = 48 * 1024;
