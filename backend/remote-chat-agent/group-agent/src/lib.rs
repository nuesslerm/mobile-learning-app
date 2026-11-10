//! The groups that run and keep a thread (plan section 3): `agent.threads` and the M4 sub-groups `agent.requests`, `agent.queue` and `agent.lifecycle`.
//! The crate owns its port (`ThreadEngine`, double `testing::MemoryThreads`); the composition root gives it the engine, executors, spawner and MCP address.
pub mod checkpoint;
pub mod group;
pub mod lifecycle;
pub mod neutral;
pub mod port;
pub mod queue;
pub mod refusal;
pub mod requests;
pub mod runner;
pub mod staging;
pub mod turns;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use group::{AgentThreadsGroup, SERVED};
pub use lifecycle::AgentLifecycleGroup;
pub use port::{Command, Decided, EngineError, Follow, NewThread, Page, Refused, Subscription, ThreadEngine};
pub use queue::AgentQueueGroup;
pub use requests::AgentRequestsGroup;
pub use runner::{McpEndpoint, Runner};
pub use staging::AttachmentStaging;
