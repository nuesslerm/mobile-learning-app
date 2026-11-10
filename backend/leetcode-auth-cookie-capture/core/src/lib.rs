//! The capture attempt of the LeetCode login capture listener without its transport: `domain` (the configuration, the cookie rule,
//! the sign-in check, the outcomes) and `application` (the attempt itself, which reaches the backend, Telegram, the browser and the
//! display stack only through ports). The `testing` feature adds the fakes the flow tests drive it with.

pub mod application;
pub mod domain;

#[cfg(feature = "testing")]
pub mod testing;
