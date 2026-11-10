use semver::Version;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bump {
    Major,
    Minor,
    Patch,
}

impl Bump {
    pub fn parse(word: &str) -> Option<Bump> {
        match word {
            "major" => Some(Bump::Major),
            "minor" => Some(Bump::Minor),
            "patch" => Some(Bump::Patch),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Bump::Major => "major",
            Bump::Minor => "minor",
            Bump::Patch => "patch",
        }
    }
}

/// The next release version. A pre-release or build suffix has no defined successor here, so it is refused.
pub fn bump(current: &str, how: Bump) -> Result<String, String> {
    let v = Version::parse(current).map_err(|error| format!("version {current:?} is not semver: {error}"))?;
    if !v.pre.is_empty() || !v.build.is_empty() {
        return Err(format!("version {current} carries a pre-release or build suffix; set a plain x.y.z by hand first"));
    }
    let next = match how {
        Bump::Major => Version::new(v.major + 1, 0, 0),
        Bump::Minor => Version::new(v.major, v.minor + 1, 0),
        Bump::Patch => Version::new(v.major, v.minor, v.patch + 1),
    };
    Ok(next.to_string())
}

/// The highest of `versions`; entries that are not semver are ignored.
pub fn latest<'a>(versions: impl IntoIterator<Item = &'a str>) -> Option<String> {
    versions.into_iter().filter_map(|text| Version::parse(text).ok()).max().map(|v| v.to_string())
}

/// Whether `text` is a version newer than `than`; false when either is not semver.
pub fn newer(text: &str, than: &str) -> bool {
    matches!((Version::parse(text), Version::parse(than)), (Ok(a), Ok(b)) if a > b)
}
