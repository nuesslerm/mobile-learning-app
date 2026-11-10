//! The thread view of remote-chat-agent without its transport: `domain` (the event-sourced rules, pure), `application` (the engine,
//! the reactor and the ingestion loop, which reach agents and disks only through ports). The service's flow tests drive it through
//! the service's router.

pub mod application;
pub mod domain;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
