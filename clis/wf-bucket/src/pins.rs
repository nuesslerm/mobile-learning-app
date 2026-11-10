//! The tools `mise.toml` pins that Nx puts in a task's hash, against what runs here: a run on other versions never shares a cache
//! entry with the pinned ones, so the build cache refuses its writes.
use std::path::Path;
use std::process::{Command, Stdio};

/// One tool `mise.toml` pins, against the version installed here.
pub struct Pin {
    pub tool: &'static str,
    pub pinned: String,
    pub actual: String,
}

impl Pin {
    pub fn ok(&self) -> bool {
        !self.actual.is_empty() && (self.pinned == "latest" || self.actual == self.pinned || self.actual.starts_with(&format!("{}.", self.pinned)))
    }
}

/// The `[tools]` entries of `mise.toml` with a plain string version.
pub fn mise_pins(text: &str) -> Vec<(String, String)> {
    let mut in_tools = false;
    let mut pins = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_tools = line == "[tools]";
        } else if let (true, Some((key, value))) = (in_tools, line.split_once('=')) {
            if let Some(version) = value.trim().strip_prefix('"').and_then(|v| v.split_once('"')).map(|(v, _)| v) {
                pins.push((key.trim().to_string(), version.to_string()));
            }
        }
    }
    pins
}

/// The version `<bin> --version` prints: `v24.13.1` (node), `11.5.2` (pnpm), `rustc 1.97.0 (...)` (rustc).
fn installed(root: &Path, bin: &str) -> String {
    let out = Command::new(bin).arg("--version").current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|o| o.status.success());
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let mut words = text.split_whitespace();
    let word = if bin == "rustc" { words.nth(1) } else { words.next() };
    word.unwrap_or_default().trim_start_matches('v').to_string()
}

/// The tools `mise.toml` pins that Nx puts in a task's hash (node, pnpm, and Rust when it is pinned), with what runs here.
pub fn pins(root: &Path) -> Vec<Pin> {
    let pinned = mise_pins(&std::fs::read_to_string(root.join("mise.toml")).unwrap_or_default());
    [("node", "node"), ("pnpm", "pnpm"), ("rust", "rustc")]
        .into_iter()
        .filter_map(|(tool, bin)| pinned.iter().find(|(k, _)| k == tool).map(|(_, version)| Pin { tool, pinned: version.clone(), actual: installed(root, bin) }))
        .collect()
}
