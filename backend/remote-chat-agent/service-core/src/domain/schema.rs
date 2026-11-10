//! What clients send, checked by serde. An input that does not fit its type is refused with serde's own message, led by the path to
//! the field that does not (`modelSelection.cli: unknown variant ...`); only the first problem is reported. The small types here are
//! the rules a field can have that a plain Rust type does not: not empty, a UTC time, one of a few words.

use serde::de::{self, Deserialize, DeserializeOwned, Deserializer, Unexpected};
use serde::Deserialize as DeserializeDerive;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Reads `value` as a `T`. The error is the library's message, with the path to the field in front when the field is not the value
/// itself.
pub fn parse<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_path_to_error::deserialize(value).map_err(|e| e.to_string())
}

/// A string of at least one character.
#[derive(Debug, Clone, PartialEq)]
pub struct NonEmpty(pub String);

impl<'de> Deserialize<'de> for NonEmpty {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.is_empty() {
            return Err(de::Error::invalid_length(0, &"a string of at least 1 character"));
        }
        Ok(Self(text))
    }
}

impl From<NonEmpty> for String {
    fn from(text: NonEmpty) -> Self {
        text.0
    }
}

/// An ISO 8601 date-time in UTC, written with a `Z`.
#[derive(Debug, Clone, PartialEq)]
pub struct Utc(pub String);

impl<'de> Deserialize<'de> for Utc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.ends_with('Z') && OffsetDateTime::parse(&text, &Rfc3339).is_ok() {
            return Ok(Self(text));
        }
        Err(de::Error::invalid_value(Unexpected::Str(&text), &"a UTC date-time such as 2026-01-01T00:00:00.000Z"))
    }
}

impl From<Utc> for String {
    fn from(time: Utc) -> Self {
        time.0
    }
}

/// The coding agents a thread may run on.
#[derive(Debug, Clone, Copy, PartialEq, DeserializeDerive)]
#[serde(rename_all = "lowercase")]
pub enum Cli {
    Opencode,
    Claude,
    Codex,
}

impl Cli {
    pub const ALL: [&'static str; 3] = ["opencode", "claude", "codex"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, DeserializeDerive)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    ApprovalRequired,
    AutoAcceptEdits,
    Auto,
    FullAccess,
}

impl RuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval-required",
            Self::AutoAcceptEdits => "auto-accept-edits",
            Self::Auto => "auto",
            Self::FullAccess => "full-access",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, DeserializeDerive)]
#[serde(rename_all = "lowercase")]
pub enum InteractionMode {
    Default,
    Plan,
}

impl InteractionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Plan => "plan",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, DeserializeDerive)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::AcceptForSession => "acceptForSession",
            Self::Decline => "decline",
            Self::Cancel => "cancel",
        }
    }
}

/// The one role a client message may have.
#[derive(Debug, Clone, Copy, PartialEq, DeserializeDerive)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    User,
}
