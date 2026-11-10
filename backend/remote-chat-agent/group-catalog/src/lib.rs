//! The `agent.catalog` group of the agent API (`libraries/agent-api/agent-api-catalog`). `table` is the service's own knowledge of the drivers
//! (their models and modes); `service::Catalog` asks the EXECUTOR which agent CLIs it has (`cli_detect`) and which skills it holds
//! (`skill_scan`), keeping a detection for a short while; `group` answers the methods.
pub mod group;
pub mod ports;
pub mod service;
pub mod table;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use group::{CatalogGroup, SERVED};
pub use ports::{Clock, Refusal};
pub use service::Catalog;
