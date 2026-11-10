use std::fmt;

/// The tool versions `mise.toml` pins.
#[derive(Clone, Debug, Default)]
pub struct Pins {
    pub node: Option<String>,
    pub pnpm: Option<String>,
}

/// What the caller found out about this checkout and toolchain; the decision below does no looking itself.
#[derive(Clone, Debug)]
pub struct WriteFacts {
    /// Every path git reports as changed, staged or untracked.
    pub changed: Vec<String>,
    /// Whether `HEAD` is an ancestor of `origin/dev`; `None` when git could not say.
    pub head_on_origin_dev: Option<bool>,
    pub node: Option<String>,
    pub pnpm: Option<String>,
    pub pins: Pins,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Dirty { files: Vec<String> },
    OffDev,
    ToolsUnpinned { tool: &'static str, found: Option<String>, pinned: Option<String> },
}

impl Refusal {
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::Dirty { .. } => "dirty",
            Refusal::OffDev => "not-on-dev",
            Refusal::ToolsUnpinned { .. } => "tools-unpinned",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.reason())?;
        match self {
            Refusal::Dirty { files } => write!(f, "the work tree has changes: {}", files.iter().take(5).cloned().collect::<Vec<_>>().join("; ")),
            Refusal::OffDev => f.write_str("HEAD is not known to be an ancestor of origin/dev"),
            Refusal::ToolsUnpinned { tool, found: Some(found), pinned: Some(pinned) } => write!(f, "{tool} {found} is not the pinned {pinned}"),
            Refusal::ToolsUnpinned { tool, found: None, .. } => write!(f, "{tool} is not installed"),
            Refusal::ToolsUnpinned { tool, pinned: None, .. } => write!(f, "mise.toml pins no {tool}"),
        }
    }
}

fn same_version(found: &Option<String>, pinned: &Option<String>, tool: &'static str) -> Result<(), Refusal> {
    let bare = |v: &String| v.trim().trim_start_matches('v').to_string();
    match (found, pinned) {
        (Some(f), Some(p)) if bare(f) == bare(p) => Ok(()),
        _ => Err(Refusal::ToolsUnpinned { tool, found: found.clone(), pinned: pinned.clone() }),
    }
}

/// Whether this run may write to the shared cache: a first write is kept for days, so only a clean checkout of a commit on the
/// trunk, running the pinned node and pnpm (both are in every Nx hash), may seed it. The first refusal is the one reported.
pub fn write_allowed(facts: &WriteFacts) -> Result<(), Refusal> {
    if !facts.changed.is_empty() {
        return Err(Refusal::Dirty { files: facts.changed.clone() });
    }
    if facts.head_on_origin_dev != Some(true) {
        return Err(Refusal::OffDev);
    }
    same_version(&facts.node, &facts.pins.node, "node")?;
    same_version(&facts.pnpm, &facts.pins.pnpm, "pnpm")
}
