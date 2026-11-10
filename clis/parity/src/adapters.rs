//! The core's ports over the disk, git and the bucket a published run lives in.
use parity_core::ports::{Evidence, History, Traces, Tree, Upstream};
use parity_core::track::Sources;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub struct FsTree {
    root: PathBuf,
}

impl FsTree {
    pub fn new(root: &Path) -> FsTree {
        FsTree { root: root.to_path_buf() }
    }
}

impl Tree for FsTree {
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(path)).ok()
    }

    fn files(&self, dir: &str) -> Vec<String> {
        let mut out = vec![];
        let mut stack = vec![self.root.join(dir)];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name == "node_modules" || name == ".git" {
                    continue;
                }
                match entry.file_type() {
                    Ok(t) if t.is_dir() => stack.push(entry.path()),
                    Ok(t) if t.is_file() => {
                        if let Ok(rel) = entry.path().strip_prefix(&self.root) {
                            out.push(rel.to_string_lossy().replace('\\', "/"));
                        }
                    }
                    _ => {}
                }
            }
        }
        out.sort();
        out
    }

    fn is_file(&self, path: &str) -> bool {
        self.root.join(path).is_file()
    }
}

pub struct GitHistory {
    root: PathBuf,
}

impl GitHistory {
    pub fn new(root: &Path) -> GitHistory {
        GitHistory { root: root.to_path_buf() }
    }

    fn git(&self, dir: &Path, args: &[&str]) -> Option<String> {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl History for GitHistory {
    fn checkout_head(&self, dir: &str) -> Option<String> {
        let checkout = self.root.join(dir);
        // Without its own .git the directory is no checkout, and git would answer for the repository around it.
        if !checkout.join(".git").exists() {
            return None;
        }
        self.git(&checkout, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    fn show(&self, rev: &str, path: &str) -> Option<String> {
        self.git(&self.root, &["show", &format!("{rev}:{path}")])
    }

    fn is_dirty(&self, path: &str) -> bool {
        self.git(&self.root, &["status", "--porcelain", "--", path]).is_some_and(|s| !s.trim().is_empty())
    }

    fn messages_since(&self, base: &str) -> Vec<String> {
        self.git(&self.root, &["log", "--format=%B%x00", &format!("{base}..HEAD")]).map(|s| s.split('\0').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect()).unwrap_or_default()
    }
}

/// The method-hit traces of a chain run: where `--traces` points on the disk, or at the bundle `wf test publish` made of it.
pub struct RunTraces;

const TRACE_FILE: &str = "trace.json";
/// How deep under a test output directory a suite's `trace.json` may be (`<out>/<suite>/trace.json`, or a published bundle's folders).
const DEPTH: usize = 3;

fn is_url(source: &str) -> bool {
    source.starts_with("https://") || source.starts_with("http://")
}

fn fetch(url: &str) -> Result<String, String> {
    let reply = cli_http::Request::get(url).timeout(Duration::from_secs(30)).send().map_err(|e| format!("{url}: {e}"))?;
    if reply.ok() {
        Ok(reply.text())
    } else {
        Err(format!("{url}: answered {}", reply.status))
    }
}

fn traces_under(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < DEPTH {
            traces_under(&path, depth + 1, out);
        } else if path.file_name().is_some_and(|n| n == TRACE_FILE) {
            out.push(path);
        }
    }
}

/// The files a bundle's `traces` list names, each fetched when it is a URL and read beside the bundle when it is a path.
fn bundle_traces(label: &str, text: &str, beside: Option<&Path>) -> Result<Vec<(String, String)>, String> {
    let bundle: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{label}: not JSON: {e}"))?;
    let listed = bundle["traces"].as_array().ok_or_else(|| format!("{label}: neither a method-trace/1 file nor a bundle with a `traces` list"))?;
    listed
        .iter()
        .filter_map(|t| t.as_str())
        .map(|t| if is_url(t) { fetch(t).map(|text| (t.to_string(), text)) } else { let path = beside.map_or(PathBuf::from(t), |b| b.join(t)); std::fs::read_to_string(&path).map(|text| (path.display().to_string(), text)).map_err(|e| format!("{}: {e}", path.display())) })
        .collect()
}

impl Traces for RunTraces {
    fn load(&self, source: &str) -> Result<Vec<(String, String)>, String> {
        if is_url(source) {
            return bundle_traces(source, &fetch(source)?, None);
        }
        let path = Path::new(source);
        if path.is_dir() {
            let mut found = vec![];
            traces_under(path, 0, &mut found);
            found.sort();
            return found.iter().map(|p| std::fs::read_to_string(p).map(|t| (p.display().to_string(), t)).map_err(|e| format!("{}: {e}", p.display()))).collect();
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{source}: {e}"))?;
        if serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|v| v["schema"] == parity_core::evidence::TRACE_SCHEMA) {
            return Ok(vec![(source.to_string(), text)]);
        }
        bundle_traces(source, &text, path.parent())
    }
}

/// What `wf mimic replay --suite` writes into a trace: each call's `diff` label and its test's `cassette` provenance. A replay proves a row
/// only when it is clean (or allowlisted with a reason) against a cassette recorded from the reference itself; authored and imported ones
/// are our own claims and never serve a row alone.
pub struct MimicEvidence;

impl Evidence for MimicEvidence {
    fn admits(&self, cassette: Option<&str>, diff: Option<&str>) -> bool {
        cassette == Some("recorded") && matches!(diff, Some("clean" | "allowlisted"))
    }
}

/// The newest-release services over HTTPS (the proxy of the environment honoured). The token of `GITHUB_TOKEN` goes to the GitHub API only.
pub struct HttpUpstream {
    github_api: String,
    token: Option<String>,
}

impl HttpUpstream {
    pub fn new(sources: &Sources, token: Option<String>) -> HttpUpstream {
        HttpUpstream { github_api: sources.github_api.clone(), token: token.filter(|t| !t.is_empty()) }
    }
}

impl Upstream for HttpUpstream {
    fn get(&self, url: &str) -> Result<String, String> {
        // GitHub's release downloads answer 5xx or time out in bursts; a tracker run is hours long, so a few tries cost nothing and a single one fails the run.
        let base = std::env::var("WF_TRACK_RETRY_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(2000);
        let mut last = String::new();
        for attempt in 0..5u32 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(base << (attempt - 1)));
            }
            let mut request = cli_http::Request::get(url).follow_redirects().timeout(Duration::from_secs(60)).header("Accept", "application/json").header("User-Agent", "wf-parity-track");
            if let (Some(token), true) = (&self.token, url.starts_with(&self.github_api)) {
                request = request.header("Authorization", &format!("Bearer {token}"));
            }
            match request.send() {
                Ok(reply) if reply.ok() => return Ok(reply.text()),
                Ok(reply) if reply.status == 429 || reply.status >= 500 => last = format!("{url}: answered {}", reply.status),
                Ok(reply) => return Err(format!("{url}: answered {}", reply.status)),
                Err(e) => last = format!("{url}: {e}"),
            }
        }
        Err(last)
    }
}

/// The services the tracker asks: the public ones, or the ones `WF_TRACK_NPM`, `WF_TRACK_GITHUB_API` and `WF_TRACK_GITHUB_WEB` name.
pub fn sources_from_env() -> Sources {
    let defaults = Sources::default();
    let pick = |name: &str, fallback: String| std::env::var(name).ok().filter(|v| !v.is_empty()).map(|v| v.trim_end_matches('/').to_string()).unwrap_or(fallback);
    Sources { npm: pick("WF_TRACK_NPM", defaults.npm), github_api: pick("WF_TRACK_GITHUB_API", defaults.github_api), github_web: pick("WF_TRACK_GITHUB_WEB", defaults.github_web) }
}
