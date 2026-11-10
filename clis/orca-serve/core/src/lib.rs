//! The pure core of `orca-serve`: no I/O. The phone's wire (`references/orca/src/main/runtime/rpc`), the `Area` trait each `orca-map-*`
//! crate implements, the table the composition root assembles from them, and the dispatcher of one authenticated connection.
pub mod area;
pub mod connection;
pub mod ports;
pub mod table;
pub mod wire;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use area::{Area, Ctx, Kind, Method, NotOffered, Subscription, Subscriptions};
pub use connection::{Connection, Host};
pub use ports::{AgentApi, AgentError, HostInfo, Principals, ServiceStatus};
pub use wire::Failure;
