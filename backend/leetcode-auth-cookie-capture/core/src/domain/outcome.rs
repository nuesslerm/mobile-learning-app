/// How an attempt ended. Every branch is a value: the attempt itself never fails once the backend has accepted it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The backend already has an active capture; nothing was started.
    AlreadyActive { expires_at: u64 },
    Succeeded,
    TimedOut,
    NoCookiesFound,
    Failed { message: String },
}
