//! The host side of the step interpreter: what runs the server a case is played against. The pinned release is fetched and verified against
//! its sha256 (`provision`), started in an environment of its own (`hermetic`), and a `ScriptDriver` keeps one server for many cases and
//! starts a new one on `reset`. A candidate that is already running is a `UrlTarget`.
pub mod child_env;
pub mod driver;
pub mod git;
pub mod hermetic;
pub mod pin;
pub mod provision;
pub mod target;

pub use driver::{Launch, ScriptDriver};
