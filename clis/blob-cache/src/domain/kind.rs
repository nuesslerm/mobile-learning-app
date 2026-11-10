use super::budget::Budget;
use std::fmt;

/// A namespace of the bucket with its own expiry. A new cache is a new variant and a row in `data/budget.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Nx,
    Pnpm,
    Probe,
    /// Rust compile output, served to `sccache` through the loopback WebDAV server.
    Sccache,
    /// The app's Metro transform cache and `tsc` build info, one restore-latest tarball.
    Metro,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Nx, Kind::Pnpm, Kind::Probe, Kind::Sccache, Kind::Metro];

    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Nx => "nx",
            Kind::Pnpm => "pnpm",
            Kind::Probe => "probe",
            Kind::Sccache => "sccache",
            Kind::Metro => "metro",
        }
    }

    pub fn from_prefix(prefix: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.prefix() == prefix)
    }

    /// Days after the last write that the bucket's lifecycle rule deletes an entry (`infra/oracle-cloud/build-cache.tf`).
    pub fn ttl_days(self) -> u32 {
        Budget::committed().ttl_days(self)
    }
}

/// The number every reader and writer agrees on; a higher one is a new, empty namespace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation(pub u64);

impl Generation {
    /// The stored pointer; absent, or anything that is not a number, is generation 0.
    pub fn parse(stored: Option<&[u8]>) -> Generation {
        Generation(stored.and_then(|b| std::str::from_utf8(b).ok()).and_then(|t| t.trim().parse().ok()).unwrap_or(0))
    }

    pub fn next(self) -> Generation {
        Generation(self.0.saturating_add(1))
    }
}

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// An object name in the bucket: `<kind>/g<generation>/<name>`, or the generation pointer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    kind: Option<Kind>,
    text: String,
}

impl Key {
    pub fn new(kind: Kind, generation: Generation, name: &str) -> Result<Key, BadName> {
        let fine = !name.is_empty() && name.len() <= 200 && !name.starts_with('.') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if !fine {
            return Err(BadName(name.to_string()));
        }
        Ok(Key { kind: Some(kind), text: format!("{}/g{generation}/{name}", kind.prefix()) })
    }

    /// A path under a prefix of its own (`libs/`) that has no kind, generation or expiry: slash-separated names of
    /// `[A-Za-z0-9._-]`, none empty or starting with a dot.
    pub fn at(path: &str) -> Result<Key, BadName> {
        let fine = path.len() <= 400
            && path.split('/').all(|part| !part.is_empty() && !part.starts_with('.') && part.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)));
        if !fine {
            return Err(BadName(path.to_string()));
        }
        Ok(Key { kind: None, text: path.to_string() })
    }

    /// `meta/generation`: one number, never expires, written only by a purge.
    pub fn generation_pointer() -> Key {
        Key { kind: None, text: "meta/generation".to_string() }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn kind(&self) -> Option<Kind> {
        self.kind
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct BadName(pub String);

impl fmt::Display for BadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "blob name {:?} must be 1 to 200 characters of [A-Za-z0-9._-] and not start with a dot", self.0)
    }
}

impl std::error::Error for BadName {}
