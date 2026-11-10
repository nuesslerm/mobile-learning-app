//! The leetcode auth cookie capture listener: `api` serves `POST /trigger` over the capture use case of
//! `leetcode-auth-cookie-capture-core`, `infrastructure` implements its ports (the backend and Telegram over HTTP, the `wf browser`
//! daemon, the virtual display), and `main.rs` wires the two.

pub mod api;
pub mod infrastructure;
