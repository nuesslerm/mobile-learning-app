//! The `schedules` and `schedules.watch` groups of the agent API. `domain` decides when a schedule is due, how a webhook request becomes a prompt
//! and whether a token is the schedule's; `ports` are what the group stands on (where schedules are kept, the time, fresh ids, and the agent group
//! that starts a turn); `service::Schedules` is the use cases and the numbered `schedules` stream; `group` answers the methods.
pub mod domain;
pub mod group;
pub mod ports;
pub mod service;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use group::{ScheduleGroup, ScheduleWatchGroup};
pub use ports::{Clock, Ids, Refusal, RunRequest, ScheduleStore, Started, Stored, TurnTrigger};
pub use service::{Schedules, WebhookOutcome, WebhookRequest};
