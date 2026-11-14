//! `wf knowledge index --gemini`: one markdown bundle for the Gemini Gem. `repomix --compress` yields a skeleton of the
//! repository, `wf offload ask` drafts `ARCHITECTURE.md` from it, and the bundle is that file followed by the full repomix
//! pack, uploaded over one Drive file in place (same file id, so the Gem's attachment never moves).
use crate::gem_drive::{self, Part};
use crate::gem_split::split_pack;
use rca_core::stats::js_json_pretty;
use rca_core::time::iso;
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::time::now_ms;

static TOTAL_FILES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Total Files:\s*([\d,]+)").expect("regex"));

const DEFAULT_MANIFEST: &str = "docs/gemini-context/manifest.json";
const DEFAULT_PROMPT: &str = "docs/gemini-context/architecture-prompt.md";
const INCLUDE: &str = "backend/**,clis/**,frontend/mla-tamagui/**,infra/**";
/// What the architecture is drafted from: package manifests, entry points, barrels and the area guides, as signatures (about 90 k
/// tokens), not every source file.
const SKELETON_INCLUDE: &str = "backend/**/Cargo.toml,clis/**/Cargo.toml,**/package.json,**/lib.rs,**/main.rs,**/index.ts,**/AGENTS.md,docs/solutions/architecture/**";
/// Generated data and recorded fixtures that would fill a Doc without teaching the Gem anything: stroke tables, golden snapshots,
/// the secret-key registry, generated modules, recorded fixtures and benchmark baselines, recorded contract data.
const PACK_IGNORE: &str = "**/scripts/tamil-strokes/**,**/registry.json,**/*.snap,**/*.generated.ts,**/*.generated.rs,**/*.generated.json,**/fixtures/**,**/baselines/**,**/contracts/data/**,**/cassettes/**,**/help/*.txt,clis/mimic/**,clis/parity/**";
/// A Doc holds about 1,024,000 characters; a part stays under this. A Gem takes 10 files: the architecture Doc and at most nine parts.
const PART_CHARS: usize = 1_000_000;
const MAX_DOCS: usize = 10;
const ASK_ATTEMPTS: usize = 3;
const ASK_TIMEOUT_SECS: u64 = 900;
const HINT_LOGIN: &str = "run: wf secret login gcloud (the Drive scope comes from `gcloud auth login --enable-gdrive-access`)";

struct Options {
    cache_dir: PathBuf,
    manifest: String,
    prompt: String,
    remote: Option<String>,
    dry_run: bool,
    summarize: bool,
}

struct Packed {
    stdout: String,
}

fn repomix(root: &Path, extra: &[&str], output: &Path) -> Result<Packed, String> {
    let run = Command::new("pnpm")
        .args(["exec", "repomix", "--style", "markdown", "--output"])
        .arg(output)
        .args(extra)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("repomix failed to start: {e}"))?;
    if !run.status.success() {
        return Err(format!("repomix failed (exit {}): {}", run.status.code().unwrap_or(1), String::from_utf8_lossy(&run.stderr)));
    }
    Ok(Packed { stdout: String::from_utf8_lossy(&run.stdout).into_owned() })
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

/// One `wf offload ask` run as the read-only agent on the prompt file, served by the rca service; its stdout is the document. It runs
/// in a directory of its own: the prompt carries every input, and the repository's agent instructions are not the draft's.
fn ask(root: &Path, prompt_file: &Path) -> Result<String, String> {
    let cwd = std::env::temp_dir().join("gem-draft-ask");
    std::fs::create_dir_all(cwd.join(".opencode/agents")).map_err(|e| format!("cannot make {}: {e}", cwd.display()))?;
    // the agent the run answers as travels with it, from the directory it runs in
    std::fs::write(cwd.join(".opencode/agents/read-only.md"), include_str!("../../../.opencode/agents/read-only.md")).map_err(|e| format!("cannot write the read-only agent: {e}"))?;
    let wf = std::env::var("WF_BIN").unwrap_or_else(|_| "wf".into());
    let run = Command::new("timeout")
        .arg(ASK_TIMEOUT_SECS.to_string())
        .arg(&wf)
        .args(["offload", "ask", "--agent", "read-only", "--shape", "document", "--prompt-file"])
        .arg(if prompt_file.is_absolute() { prompt_file.to_path_buf() } else { root.join(prompt_file) })
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{wf} offload ask failed to start: {e}"))?;
    if !run.status.success() {
        return Err(format!("wf offload ask exited {}: {}", run.status.code().unwrap_or(1), String::from_utf8_lossy(&run.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&run.stdout).trim().to_string())
}

fn usable(draft: &str) -> bool {
    !draft.is_empty() && !draft.contains("WROTE_NOTHING") && draft.lines().any(|l| l.starts_with('#'))
}

fn draft_architecture(root: &Path, o: &Options, skeleton_path: &Path) -> Result<String, String> {
    // The inputs go inline, in the prompt the rca service takes over HTTP: a model told to read a few hundred KB of skeleton with its
    // file tool takes a quarter of an hour, one given it in the prompt answers in a few minutes.
    let skeleton = read(root, &skeleton_path.strip_prefix(root).unwrap_or(skeleton_path).to_string_lossy())?;
    let prompt = format!(
        "{}\n\nThe inputs follow, inline. Use no tools; reply with the ARCHITECTURE.md document only.\n\n===== CLAUDE.md =====\n{}\n\n===== CONCEPTS.md =====\n{}\n\n===== skeleton (a repomix --compress skeleton: manifests, entry points and signatures) =====\n{skeleton}",
        read(root, &o.prompt)?,
        read(root, "CLAUDE.md")?,
        read(root, "CONCEPTS.md")?,
    );
    let prompt_file = o.cache_dir.join("architecture-request.md");
    std::fs::write(&prompt_file, prompt).map_err(|e| e.to_string())?;
    let mut last = String::new();
    for attempt in 1..=ASK_ATTEMPTS {
        match ask(root, &prompt_file) {
            Ok(draft) if usable(&draft) => return Ok(draft),
            Ok(_) => last = "the model returned no document".into(),
            Err(e) => last = e,
        }
        println!("architecture draft attempt {attempt}/{ASK_ATTEMPTS} failed: {last}");
    }
    Err(format!("ARCHITECTURE.md could not be drafted after {ASK_ATTEMPTS} attempts: {last}"))
}

/// `(folder or local dir, name family)` of the Docs the bundle becomes: `<family>-00-architecture`, `<family>-01`, `<family>-02`, ...
fn parse_remote(remote: &str) -> Result<(String, String), String> {
    let (target, name) = remote.rsplit_once('/').filter(|(t, n)| !t.is_empty() && !n.is_empty()).ok_or_else(|| format!("remote \"{remote}\" must be <drive-folder-id>/<doc-name-prefix> or local:<dir>/<doc-name-prefix>"))?;
    Ok((target.to_string(), name.to_string()))
}

fn gcloud(args: &[&str]) -> Option<String> {
    let out = Command::new("gcloud").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The access token of the first logged-in account: the login relayed into agent-shell is a credential file without an
/// active-account setting, so the account is named, not assumed.
fn mint_token() -> Result<String, String> {
    let account = gcloud(&["auth", "list", "--format=value(account)"]).and_then(|a| a.lines().next().map(str::to_string)).filter(|a| !a.is_empty());
    let token = account.and_then(|a| gcloud(&["auth", "print-access-token", &a])).filter(|t| !t.is_empty());
    token.ok_or_else(|| format!("gcloud has no usable credential -- {HINT_LOGIN}"))
}

/// The proof line goes to the job summary of a GitHub run, one bullet per sync, so the file id of each night can be read side by side.
fn note_in_summary(line: &str) {
    let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") else { return };
    use std::io::Write;
    if let Err(e) = std::fs::OpenOptions::new().append(true).create(true).open(&path).and_then(|mut f| writeln!(f, "- {line}")) {
        wf_kit::wf_err!("wf: the step summary {path} could not be written: {e}");
    }
}

fn sha(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn generate(o: &Options) -> Result<Value, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let remote = match (&o.remote, o.dry_run) {
        (Some(r), _) => Some(r.clone()),
        (None, true) => None,
        (None, false) => return Err("no destination: set GEMINI_CONTEXT_REMOTE or pass --remote <drive-folder-id>/<file-name>".into()),
    };
    // a Drive destination needs a login: say so now, not after the draft (minutes of model time) has been made
    if let (Some(remote), false) = (&remote, o.dry_run) {
        if !parse_remote(remote)?.0.starts_with("local:") {
            mint_token()?;
        }
    }
    std::fs::create_dir_all(&o.cache_dir).map_err(|e| e.to_string())?;
    let architecture_path = o.cache_dir.join("ARCHITECTURE.md");
    let fell_back = !o.summarize && !architecture_path.exists();
    if fell_back {
        println!("no cached {}: --no-summary fell back to a full summary run", architecture_path.display());
    }
    if o.summarize || fell_back {
        let skeleton_path = o.cache_dir.join("skeleton.md");
        println!("packing the compressed skeleton");
        repomix(&root, &["--compress", "--include", SKELETON_INCLUDE], &skeleton_path)?;
        println!("drafting ARCHITECTURE.md");
        let draft = draft_architecture(&root, o, &skeleton_path)?;
        std::fs::write(&architecture_path, format!("{draft}\n")).map_err(|e| e.to_string())?;
    }
    println!("packing the full source");
    let pack_path = o.cache_dir.join("pack.md");
    let packed = repomix(&root, &["--remove-comments", "--include", INCLUDE, "--ignore", PACK_IGNORE, "--include-full-directory-structure"], &pack_path)?;
    let files = TOTAL_FILES.captures(&packed.stdout).and_then(|c| c[1].replace(',', "").parse::<u64>().ok()).unwrap_or(0);
    let architecture = std::fs::read_to_string(&architecture_path).map_err(|e| e.to_string())?;
    let pack = std::fs::read_to_string(&pack_path).map_err(|e| e.to_string())?;
    let parts = split_pack(&pack, PART_CHARS);
    let (_, family) = parse_remote(remote.as_deref().unwrap_or("local:dry/gem"))?;
    let mut docs = vec![Part { name: format!("{family}-00-architecture"), text: format!("{}\n", architecture.trim_end()) }];
    docs.extend(parts.into_iter().enumerate().map(|(i, text)| Part { name: format!("{family}-{:02}", i + 1), text }));
    if docs.len() > MAX_DOCS {
        return Err(format!("the pack needs {} Docs but a Gem takes {MAX_DOCS}: narrow INCLUDE or PACK_IGNORE", docs.len()));
    }
    if let Some(big) = docs.iter().find(|d| gem_drive::chars(&d.text) > PART_CHARS + 1) {
        return Err(format!("{} is {} characters, over the {PART_CHARS} a Doc part may hold", big.name, gem_drive::chars(&big.text)));
    }
    let docs_dir = o.cache_dir.join("docs");
    let _ = std::fs::remove_dir_all(&docs_dir);
    std::fs::create_dir_all(&docs_dir).map_err(|e| e.to_string())?;
    for d in &docs {
        std::fs::write(docs_dir.join(format!("{}.md", d.name)), &d.text).map_err(|e| e.to_string())?;
    }
    let total: usize = docs.iter().map(|d| gem_drive::chars(&d.text)).sum();
    let listing: Vec<Value> = docs.iter().map(|d| json!({ "name": d.name, "chars": gem_drive::chars(&d.text) })).collect();
    let manifest = json!({ "version": 3, "generatedAt": iso(now_ms()), "docs": listing, "chars": total, "files": files, "architectureSha": sha(&architecture), "remote": remote, "uploaded": !o.dry_run });
    if let (Some(remote), false) = (&remote, o.dry_run) {
        let (target, _) = parse_remote(remote)?;
        let lines = match target.strip_prefix("local:") {
            Some(dir) => gem_drive::publish_local(Path::new(dir), &docs)?,
            None => gem_drive::publish_drive(&mint_token()?, &target, &family, &docs)?,
        };
        for line in &lines {
            println!("{line}");
            note_in_summary(line);
        }
        let path = root.join(&o.manifest);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, format!("{}\n", js_json_pretty(&manifest))).map_err(|e| e.to_string())?;
    }
    Ok(manifest)
}

/// What `knowledge index --gemini` takes.
pub const SPEC: Spec = Spec {
    values: &["cache-dir", "manifest", "remote", "prompt"],
    flags: &["gemini", "write", "dry-run", "no-summary"],
    positional: Positional::None,
    ..Spec::NONE
};

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let o = Options {
        cache_dir: PathBuf::from(p.opts.get("cache-dir").cloned().unwrap_or_else(|| ".gemini-context-cache".into())),
        manifest: p.opts.get("manifest").cloned().unwrap_or_else(|| DEFAULT_MANIFEST.into()),
        prompt: p.opts.get("prompt").cloned().unwrap_or_else(|| DEFAULT_PROMPT.into()),
        remote: p.opts.get("remote").cloned().or_else(|| std::env::var("GEMINI_CONTEXT_REMOTE").ok()).filter(|r| !r.is_empty()),
        dry_run: p.flags.iter().any(|f| f == "dry-run"),
        summarize: !p.flags.iter().any(|f| f == "no-summary"),
    };
    let fail = |m: String| {
        wf_kit::wf_err!("wf: {m}");
        Some(1)
    };
    match generate(&o) {
        Ok(manifest) => {
            println!("{} Docs, {} chars, {} files, architecture {}", manifest["docs"].as_array().map_or(0, Vec::len), manifest["chars"], manifest["files"], manifest["architectureSha"].as_str().unwrap_or(""));
            if manifest["uploaded"] == json!(true) {
                println!("Manifest written: {}", o.manifest);
            }
            Some(0)
        }
        Err(m) if m.contains("401") || m.contains("403") => fail(format!("Google rejected the credential -- {HINT_LOGIN}: {m}")),
        Err(m) => fail(m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repomix ignore glob as a regex over a repo-relative path: `**/` is any directories, `**` anything, `*` anything but `/`.
    fn glob(g: &str) -> Regex {
        let mut re = String::from("^");
        let mut chars = g.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '*' if chars.peek() == Some(&'*') => {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        re.push_str("(.*/)?");
                    } else {
                        re.push_str(".*");
                    }
                }
                '*' => re.push_str("[^/]*"),
                c => re.push_str(&regex::escape(&c.to_string())),
            }
        }
        Regex::new(&format!("{re}$")).expect("glob regex")
    }

    fn packed(path: &str) -> bool {
        let top = |p: &str| INCLUDE.split(',').any(|g| path.starts_with(g.trim_end_matches("**")) && !p.is_empty());
        top(path) && !PACK_IGNORE.split(',').any(|g| glob(g).is_match(path))
    }

    // Given the tracked files that are generated or recorded data (a `.generated.` module, a fixtures or baselines directory, recorded
    // contract data), When the pack is built, Then none of them is packed: they would fill Docs without teaching the Gem anything.
    #[test]
    fn generated_and_recorded_data_stays_out_of_the_pack() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = Command::new("git").args(["ls-files", "backend", "clis", "frontend/mla-tamagui", "infra"]).current_dir(&root).output().expect("git ls-files");
        let files = String::from_utf8_lossy(&out.stdout).into_owned();
        let data: Vec<&str> = files
            .lines()
            .filter(|p| {
                let segs: Vec<&str> = p.split('/').collect();
                segs.last().is_some_and(|f| f.contains(".generated.")) || segs.iter().any(|s| matches!(*s, "fixtures" | "baselines" | "cassettes")) || p.contains("/contracts/data/")
            })
            .collect();
        assert!(!data.is_empty(), "the repository has no generated or recorded data to check against");
        let leaked: Vec<&&str> = data.iter().filter(|p| packed(p)).collect();
        assert!(leaked.is_empty(), "packed although generated or recorded: {leaked:?}");
        // and source is still packed
        assert!(packed("clis/knowledge/src/knowledge_gemini.rs"));
    }
}
