//! The `preview`, `preview.sessions` and `preview.watch` groups of the agent API. `preview` is each method of the dev server family asked of the
//! executor that owns the workspace (`PreviewGroup`: the service keeps no process, no path and no port). `sessions::Previews` is the preview tabs
//! of the threads and the numbered `preview` stream of their changes; the tabs live in the service's memory, because the app draws the page, and
//! a restart is a new `epoch` that tells a caller to ask again. `group` answers the methods.
pub mod group;
pub mod ports;
pub mod sessions;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use group::{PreviewGroup, PreviewSessionGroup, PreviewWatchGroup};
pub use ports::{Clock, Ids};
pub use sessions::{Failure, Previews};
