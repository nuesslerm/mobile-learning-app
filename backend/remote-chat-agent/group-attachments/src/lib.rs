//! The `attachments` group of the agent API (`libraries/agent-api/agent-api-attachments`). `ports` are what it stands on (the blob store on the
//! service volume, the clock, fresh ids); `service::Attachments` is the use cases, among them writing an attachment into a workspace through
//! the executor that owns it; `group` answers the methods.
pub mod group;
pub mod ports;
pub mod service;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use group::{AttachmentsGroup, SERVED};
pub use ports::{BlobStore, Clock, Ids, Put, Refusal};
pub use service::Attachments;
