//! What the sessions stand on: the time of a change and fresh ids.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub trait Ids: Send + Sync {
    /// A fresh id that starts with `prefix` and has never been given before.
    fn id(&self, prefix: &str) -> String;
}
