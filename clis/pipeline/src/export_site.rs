//! The web export the browser e2e tests run against: `wf test prepare --web` builds it and stamps what it was built
//! from, `wf test run` serves it on loopback and refuses one that is missing or older than the tree.
use wf_kit::args::{parse_spec, Positional, Spec};
use harness_core::staticd::{Listener, Reply};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

const SPEC: Spec = Spec { flags: &["web", "force"], positional: Positional::None, ..Spec::NONE };

/// The app, its export dir and the base path its `experiments.baseUrl` serves it under.
const APP: &str = "frontend/mla-tamagui";
pub const BASE_PATH: &str = "mla-tamagui";
const STAMP: &str = "wf-export.json";

/// What an export is built from: the app, its libraries, and what pnpm links them with. The app's other workspace links
/// (the lockfile names them, e.g. `frontend/libraries/remote-chat-agent-contracts`) come on top.
const INPUT_ROOTS: [&str; 8] = ["frontend", "libraries", "patches", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "tsconfig.json", "babel.config.cjs"];

/// Source maps go into the export: the browser debugger resolves logpoints and failure stacks through them.
const SOURCE_MAPS: &str = "--source-maps";

/// Why an export cannot be used, and the command that fixes it.
pub struct Problem {
    pub error: String,
    pub next: String,
}

impl Problem {
    fn new(error: impl Into<String>, next: &str) -> Self {
        Self { error: error.into(), next: next.to_string() }
    }

    pub fn json(&self) -> Value {
        json!({ "ok": false, "error": self.error, "next": self.next })
    }
}

pub fn dir(repo: &Path) -> PathBuf {
    repo.join(APP).join("dist")
}

/// A source file the bundle can contain: the tests, stories and docs beside it do not change it. The export's own output
/// does not count, but a library's tracked `dist` does (the app bundles it).
fn is_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let output = path.strip_prefix(APP).is_some_and(|rest| rest.starts_with("/dist/") || rest.starts_with("/dist.rejected/"));
    !(output || path.ends_with(".md") || path.contains("/__tests__/") || path.contains("/tests/") || name.contains(".test.") || name.contains(".stories.") || name.starts_with("vitest"))
}

/// The input roots: the fixed ones, and every workspace directory the app links to in the lockfile (none without one).
fn input_roots(repo: &Path) -> Result<Vec<String>, String> {
    let mut roots: Vec<String> = INPUT_ROOTS.iter().map(|r| r.to_string()).collect();
    if let Ok(lock) = std::fs::read_to_string(repo.join("pnpm-lock.yaml")) {
        roots.extend(crate::lock_closure::linked_dirs(&lock, APP)?);
    }
    Ok(roots)
}

/// A hash of the app inputs as they are on disk (tracked files with their edits, and new files git does not ignore).
pub fn inputs_hash(repo: &Path) -> Result<String, String> {
    let listed = Command::new("git").args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"]).args(input_roots(repo)?).current_dir(repo).output().map_err(|e| format!("cannot run git: {e}"))?;
    if !listed.status.success() {
        return Err(format!("git ls-files failed in {}: {}", repo.display(), String::from_utf8_lossy(&listed.stderr).trim()));
    }
    let mut files: Vec<String> = String::from_utf8_lossy(&listed.stdout).split('\0').filter(|f| !f.is_empty() && is_input(f)).map(str::to_string).collect();
    files.sort();
    files.dedup();
    let mut hash = Sha256::new();
    for file in files {
        // a tracked file deleted from the tree is simply absent from the hash
        if let Ok(bytes) = std::fs::read(repo.join(&file)) {
            hash.update(file.as_bytes());
            hash.update([0]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(&bytes);
        }
    }
    Ok(hex::encode(hash.finalize())[..16].to_string())
}

fn head(repo: &Path) -> String {
    Command::new("git").args(["rev-parse", "HEAD"]).current_dir(repo).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// A baked value shown as it is: a flag, or a URL that carries no credential.
fn plain(name: &str, value: &str) -> bool {
    matches!(name, "EXPO_PUBLIC_DEVICE_LAB" | "EXPO_PUBLIC_TAMIL_CANVAS") || (name.ends_with("_BASE_URL") && !value.contains(['@', '?', '#']))
}

/// The `EXPO_PUBLIC_*` values the build inlines, as the stamp records them: anything not known to be plain only as "set:" and
/// a few hex digits of its hash, enough to tell that it changed.
fn baked_env() -> Value {
    let shown = |k: &str, v: String| if plain(k, &v) { v } else { format!("set:{}", &hex::encode(Sha256::digest(v.as_bytes()))[..8]) };
    let baked: std::collections::BTreeMap<String, String> = std::env::vars().filter(|(k, v)| k.starts_with("EXPO_PUBLIC_") && !v.is_empty()).map(|(k, v)| (k.clone(), shown(&k, v))).collect();
    json!(baked)
}

/// The flags `export:web` passes to `expo export` (through `EXPO_EXPORT_FLAGS`, since the script ends with the render guard):
/// the caller's, and source maps.
fn export_flags() -> String {
    let own = std::env::var("EXPO_EXPORT_FLAGS").unwrap_or_default();
    if own.split_whitespace().any(|f| f == SOURCE_MAPS) { own.trim().to_string() } else { format!("{} {SOURCE_MAPS}", own.trim()).trim().to_string() }
}

/// What a result records of the baked values: their names and one digest, never the values (results get published).
pub fn baked_summary(baked: &Value) -> Value {
    let keys: Vec<&String> = baked.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    json!({ "keys": keys, "digest": &hex::encode(Sha256::digest(baked.to_string().as_bytes()))[..12] })
}

/// The stamp of the export in the repo, when the export is whole and built from the tree as it is now.
pub fn check(repo: &Path) -> Result<Value, Problem> {
    let export = dir(repo);
    let rebuild = "wf test prepare --web";
    if !export.join("index.html").is_file() {
        return Err(Problem::new(format!("no web export at {}: the browser e2e tests run against one", export.display()), rebuild));
    }
    let stamp: Value = std::fs::read_to_string(export.join(STAMP)).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or_else(|| Problem::new(format!("the web export at {} has no {STAMP}, so what it was built from is unknown (it was not built by `wf test prepare --web`)", export.display()), "wf test prepare --web --force"))?;
    let now = inputs_hash(repo).map_err(|e| Problem::new(e, rebuild))?;
    if stamp["inputs"] != now.as_str() {
        let (built, at) = (stamp["sha"].as_str().unwrap_or("?"), head(repo));
        return Err(Problem::new(format!("the web export at {} is stale: built at commit {built} from app inputs {}, the tree is at {at} with inputs {now} (an export of other code crashes the app)", export.display(), stamp["inputs"].as_str().unwrap_or("?")), rebuild));
    }
    Ok(stamp)
}

fn build(repo: &Path, baked: Value, flags: String) -> Result<Value, Problem> {
    let export = dir(repo);
    let inputs = inputs_hash(repo).map_err(|e| Problem::new(e, "wf test prepare --web"))?;
    // a half-built export must not pass for the last good one
    let _ = std::fs::remove_file(export.join(STAMP));
    let started = Instant::now();
    let sampler = wf_vmops::vm_sampler::start_from_env();
    // its progress goes to stderr: stdout carries this command's JSON
    let status = Command::new("pnpm")
        .args(["--config.verify-deps-before-run=false", "--filter", "mla-tamagui", "run", "export:web"])
        .env("EXPO_EXPORT_FLAGS", &flags)
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .map_err(|e| Problem::new(format!("cannot run pnpm: {e}"), "mise install"))?;
    let vm = sampler.map(|s| s.finish());
    let memory = json!({ "peakRssMb": crate::eas::children_peak_rss_mb(), "vmMemAvailableMinMb": vm.as_ref().map(|v| v["memAvailableMinMb"].clone()), "vmLoad1Max": vm.as_ref().map(|v| v["load1Max"].clone()) });
    let next = "pnpm --filter mla-tamagui run export:web";
    if !status.success() {
        return Err(Problem::new(format!("export:web failed ({status}); a static render that failed in a page leaves the export in {APP}/dist.rejected, and the build output above says why"), next));
    }
    if !export.join("index.html").is_file() {
        return Err(Problem::new(format!("export:web finished but {} has no index.html", export.display()), next));
    }
    let stamp = json!({ "schema": 1, "sha": head(repo), "inputs": inputs, "builtAt": wf_kit::time::now_ms(), "buildS": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0, "baked": baked, "flags": flags, "memory": memory });
    std::fs::write(export.join(STAMP), stamp.to_string()).map_err(|e| Problem::new(format!("cannot write {STAMP}: {e}"), next))?;
    Ok(stamp)
}

/// An export of the tree as it is: the one in place when it is current and was built with the same baked values and flags,
/// else a new build. `force` always builds.
pub fn ensure(repo: &Path, force: bool) -> Result<(Value, bool), Problem> {
    let (baked, flags) = (baked_env(), export_flags());
    if !force {
        if let Ok(stamp) = check(repo) {
            if stamp["baked"] == baked && stamp["flags"] == flags.as_str() {
                return Ok((stamp, true));
            }
        }
    }
    build(repo, baked, flags).map(|stamp| (stamp, false))
}

/// `wf test prepare --web [--force]`: builds the web export of the tree unless a current one is in place; prints {ok, reused, dir,
/// sha, inputs, builtAt, buildS, baked, flags, memory}, or {ok: false, error, next}.
pub fn run(args: &[String]) -> i32 {
    let Some(p) = parse_spec(args, &SPEC) else { return 2 };
    if !p.flag("web") {
        println!("{}", json!({ "ok": false, "error": "wf test prepare needs a target: --web is the only one", "next": "wf test prepare --web" }));
        return 2;
    }
    let repo = crate::e2e_tests::repo();
    match ensure(&repo, p.flag("force")) {
        Ok((stamp, reused)) => {
            println!("{}", json!({ "ok": true, "reused": reused, "dir": dir(&repo).display().to_string(), "sha": stamp["sha"], "inputs": stamp["inputs"], "builtAt": stamp["builtAt"], "buildS": stamp["buildS"], "baked": stamp["baked"], "flags": stamp["flags"], "memory": stamp["memory"] }));
            0
        }
        Err(problem) => {
            println!("{}", problem.json());
            1
        }
    }
}

/* ---------- the server ---------- */

pub struct ExportServer {
    listener: Listener,
    pub base_url: String,
}

impl ExportServer {
    pub fn stop(&self) {
        self.listener.stop();
    }
}

/// The current export of the repo, served on a free loopback port; its stamp says what it was built from.
pub fn serve_current(repo: &Path) -> Result<(ExportServer, Value), Problem> {
    let stamp = check(repo)?;
    let server = serve(&dir(repo), BASE_PATH, 0).map_err(|e| Problem::new(e, "wf test run"))?;
    Ok((server, stamp))
}

fn bracket_child(dir: &Path, want_dir: bool) -> Option<PathBuf> {
    let hits: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ft = e.file_type().ok();
            name.starts_with('[') && name[1..].contains(']') && name.len() > 2 && if want_dir { ft.is_some_and(|t| t.is_dir()) } else { ft.is_some_and(|t| t.is_file()) && name.ends_with(".html") }
        })
        .map(|e| e.path())
        .collect();
    (hits.len() == 1).then(|| hits[0].clone())
}

/// Expo writes dynamic routes to literal `[param]` paths; walk segments the way a host rewrite would.
fn resolve_path(root: &Path, rel: &str) -> Option<PathBuf> {
    let segs: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    let mut cur = root.to_path_buf();
    for (i, seg) in segs.iter().enumerate() {
        let literal = cur.join(seg);
        if i == segs.len() - 1 {
            let html = cur.join(format!("{seg}.html"));
            for c in [literal.clone(), html, literal.join("index.html")] {
                if c.is_file() {
                    return Some(c);
                }
            }
            return bracket_child(&cur, false);
        }
        cur = if literal.is_dir() { literal } else { bracket_child(&cur, true)? };
    }
    let index = cur.join("index.html");
    index.is_file().then_some(index)
}

fn answer(target: &str, root: &Path, base: &str, fallback: &Path) -> Reply {
    let path = cli_http::decode_uri_component(target.split('?').next().unwrap_or("/"));
    if !base.is_empty() && path != base && !path.starts_with(&format!("{base}/")) {
        return Reply::text(404, "");
    }
    let rel = path[base.len()..].trim_start_matches(['/', '\\']).to_string();
    if Path::new(&rel).components().any(|c| c == Component::ParentDir) {
        return Reply::text(400, "");
    }
    let hit = resolve_path(root, &rel);
    let has_ext = Path::new(&rel).extension().is_some();
    let Some(file) = hit.or_else(|| (!has_ext).then(|| fallback.to_path_buf())) else {
        return Reply::text(404, "");
    };
    match std::fs::read(&file) {
        Ok(body) => Reply::file(&file, body),
        Err(e) => Reply::text(500, e.to_string()),
    }
}

/// Serves an export's files under `base_path` on a loopback port (0: any free one): a route's own html, the `[param]`
/// page of a dynamic route, and for any other path without an extension the SPA fallback (`404.html`, else `index.html`).
pub fn serve(root: &Path, base_path: &str, port: u16) -> Result<ExportServer, String> {
    let base = format!("/{}", base_path.trim_matches('/'));
    let base = base.trim_end_matches('/').to_string();
    let not_found = root.join("404.html");
    let fallback = if not_found.is_file() { not_found } else { root.join("index.html") };
    let (served, prefix) = (root.to_path_buf(), base.clone());
    let listener = Listener::bind("127.0.0.1", port, move |target| answer(target, &served, &prefix, &fallback))?;
    let base_url = format!("http://127.0.0.1:{}{base}", listener.port());
    Ok(ExportServer { listener, base_url })
}

#[cfg(test)]
mod tests {
    use super::{baked_summary, is_input, plain};
    use serde_json::json;

    #[test]
    fn only_the_files_a_bundle_is_made_of_are_inputs() {
        // Given paths of sources the bundle contains, a library's built dist among them
        let sources = ["frontend/mla-tamagui/app/_layout.tsx", "frontend/mla-tamagui/metro.config.js", "libraries/browser-client/src/index.ts", "pnpm-lock.yaml", "frontend/mla-tamagui/assets/icon.png", "frontend/libraries/voice-edit-dsl/dist/index.js"];
        // and paths of tests, stories, docs and the export's own output
        let others = ["frontend/mla-tamagui/dist/index.html", "frontend/mla-tamagui/dist.rejected/index.html", "frontend/mla-tamagui/tests/flow/a.flow.test.tsx", "frontend/libraries/x/src/__tests__/a.ts", "frontend/libraries/x/src/a.test.ts", "frontend/libraries/x/README.md", "frontend/mla-tamagui/vitest.config.ts"];
        // When each is asked whether it is an input
        // Then the sources are, and the rest is not
        assert!(sources.iter().all(|f| is_input(f)), "{sources:?}");
        assert!(others.iter().all(|f| !is_input(f)), "{others:?}");
    }

    #[test]
    fn a_baked_credential_is_never_shown_and_a_result_carries_no_value() {
        // Given a URL, a URL with user info, a token and a name nobody listed
        let shown = [("EXPO_PUBLIC_AGENT_BASE_URL", "http://agent:8787"), ("EXPO_PUBLIC_X_BASE_URL", "http://u:p@h"), ("EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", "t"), ("EXPO_PUBLIC_NEW_HOST", "h")];
        // When each is asked whether it may be shown as it is
        let plain_ones: Vec<bool> = shown.iter().map(|(k, v)| plain(k, v)).collect();
        // Then only the clean URL is, and a result's summary keeps the names and one digest, not the values
        assert_eq!(plain_ones, [true, false, false, false]);
        let summary = baked_summary(&json!({ "EXPO_PUBLIC_AGENT_BASE_URL": "http://agent:8787" }));
        assert_eq!(summary["keys"], json!(["EXPO_PUBLIC_AGENT_BASE_URL"]));
        assert!(!summary.to_string().contains("agent:8787") && summary["digest"].as_str().is_some_and(|d| d.len() == 12), "{summary}");
    }
}
