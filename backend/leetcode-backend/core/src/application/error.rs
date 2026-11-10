/// Why a use case failed, in the terms the caller maps to its protocol.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AppError {
    /// The session file could not be read or written.
    #[error("{0}")]
    Store(String),
    /// leetcode.com (or the local runner) failed or answered badly.
    #[error("{0}")]
    Upstream(String),
    /// The route needs a LeetCode session and there is none.
    #[error("no session")]
    NoSession,
    /// The cookie string handed to `set_session` has no session value.
    #[error("{0}")]
    InvalidCookie(String),
}
