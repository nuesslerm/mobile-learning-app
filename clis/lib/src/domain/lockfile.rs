//! What a `Cargo.lock` pins.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub name: String,
    pub version: String,
    pub source: Source,
}

/// Where the locked package comes from. The registry's index URL is never kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Path,
    CratesIo,
    Mla,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Path => "path",
            Source::CratesIo => "crates-io",
            Source::Mla => "mla",
        }
    }
}

fn source_of(text: Option<&str>) -> Option<Source> {
    match text {
        None => Some(Source::Path),
        Some(s) if s.contains("crates.io-index") || s.contains("index.crates.io") => Some(Source::CratesIo),
        Some(s) if s.starts_with("sparse+") || s.starts_with("registry+") => Some(Source::Mla),
        Some(_) => None,
    }
}

pub fn pins(lock: &str) -> Result<Vec<Pin>, String> {
    let value: toml::Table = lock.parse().map_err(|error| format!("not a Cargo.lock: {error}"))?;
    Ok(value
        .get("package")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let source = source_of(p.get("source").and_then(|s| s.as_str()))?;
            Some(Pin { name: p.get("name")?.as_str()?.to_string(), version: p.get("version")?.as_str()?.to_string(), source })
        })
        .collect())
}
