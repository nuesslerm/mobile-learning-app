//! What the schedules groups stand on. The service's adapters implement them; `testing` has the doubles.
use agent_api_schedules::TaskView;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A schedule as kept: its view and the digest of its webhook token (never the token).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub task: TaskView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_digest: Option<String>,
}

/// Where the schedules live: the adapter keeps them across restarts.
pub trait ScheduleStore: Send + Sync {
    /// Every schedule, oldest first.
    fn load(&self) -> Result<Vec<Stored>, String>;
    /// Replaces what is kept with `all`, whole or not at all.
    fn save(&self, all: &[Stored]) -> Result<(), String>;
}

pub trait Clock: Send + Sync {
    /// Milliseconds since the epoch.
    fn now_ms(&self) -> u64;
}

pub trait Ids: Send + Sync {
    /// A fresh id that starts with `prefix`.
    fn id(&self, prefix: &str) -> String;
    /// A fresh secret to put in a URL: at least 240 bits.
    fn token(&self) -> String;
}

/// A turn to start for a run of a schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    pub schedule: String,
    pub run: String,
    pub title: String,
    pub prompt: String,
    pub workspace: String,
    pub driver: String,
    pub model: Option<String>,
    /// The thread to go on; none asks for a new one.
    pub thread: Option<String>,
    pub runtime_mode: Option<String>,
    pub interaction_mode: Option<String>,
}

/// The turn that began.
#[derive(Debug, Clone, PartialEq)]
pub struct Started {
    pub thread: String,
    pub turn: String,
}

/// A turn that could not begin: the agent group's own code and message.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

/// The agent group, as a schedule needs it: this crate does not depend on it, the service's adapter calls its methods.
#[async_trait]
pub trait TurnTrigger: Send + Sync {
    async fn start(&self, run: &RunRequest) -> Result<Started, Refusal>;
    /// Waits for the turn to end: `Ok` when it completed, the reason when it failed or was interrupted.
    async fn finished(&self, started: &Started) -> Result<(), String>;
}
