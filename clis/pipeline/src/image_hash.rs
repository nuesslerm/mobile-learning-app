//! The content hash of a service image: its Dockerfile, every file the Dockerfile COPYs from the build context, and for a Rust image
//! the locked dependencies of the crates it builds (not the whole lockfile, and not the workspace crates it only needs on disk). The
//! hash tags the image `nx-<hash>`: an image with that tag in the registry is never rebuilt, and `wf ship --changed` compares the hash
//! of the commit a service runs with the hash of HEAD. The base image tag is part of the Dockerfile.
use crate::copy_closure::{copy_sources, path_closure_by};
use crate::docker_ignore::DockerIgnore;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use wf_vmops::services::{app_image, VM_OPS_API};

pub struct Image {
    pub name: &'static str,
    pub dockerfile: &'static str,
    /// The build context, relative to the repo root.
    pub context: &'static str,
    /// The crates the image builds; empty for an image that builds none.
    pub roots: &'static [&'static str],
}

const fn image(name: &'static str, dockerfile: &'static str, context: &'static str, roots: &'static [&'static str]) -> Image {
    Image { name, dockerfile, context, roots }
}

/// The images `wf ship` builds on the VM, in the order it lists them.
pub const IMAGES: &[Image] = &[
    image("remote-chat-agent", "backend/remote-chat-agent/Dockerfile", ".", &["backend/remote-chat-agent/rs"]),
    image("remote-chat-agent-stt", "backend/remote-chat-agent/stt/Dockerfile", "backend/remote-chat-agent/stt", &[]),
    image("opencode-server", "backend/remote-chat-agent/opencode-server/Dockerfile", ".", &[]),
    image("leetcode-backend", "backend/leetcode-backend/Dockerfile", ".", &["backend/leetcode-backend/rs"]),
    image("leetcode-auth-cookie-capture", "backend/leetcode-auth-cookie-capture/Dockerfile", ".", &["backend/leetcode-auth-cookie-capture/rs"]),
    image("reporting-service", "backend/reporting-service/Dockerfile", ".", &["backend/reporting-service/rs"]),
    image("libs-proxy", "backend/libs-proxy/Dockerfile", ".", &["backend/libs-proxy/rs"]),
    image("t3-serve", "clis/t3-serve/Dockerfile", ".", &["clis/t3-serve"]),
    image("orca-serve", "clis/orca-serve/Dockerfile", ".", &["clis/orca-serve"]),
    image("vm-ops-api-rs", "backend/vm-ops-api/Dockerfile", ".", &["backend/vm-ops-api/rs"]),
    image("watchdog", "infra/oracle-cloud/watchdog/Dockerfile", ".", &["backend/watchdog/rs"]),
    image("agent-shell", "infra/oracle-cloud/agent-shell/Dockerfile", ".", &[]),
    image("logger-cli", "infra/oracle-cloud/logger-cli/Dockerfile", ".", &[]),
    image("vnc-auth-capture", "clis/vnc-auth-capture/Dockerfile", ".", &[]),
];

/// Where an app's compose project lives when it is not beside its Dockerfile.
const COMPOSE_DIRS: &[(&str, &str)] = &[("libs-proxy", "infra/oracle-cloud/libs-proxy")];

/// Images a release needs besides the app's own: the sidecars its compose file pulls on the same tag.
pub fn release_companions(app: &str) -> &'static [&'static str] {
    match app {
        "remote-chat-agent" => &["remote-chat-agent-stt", "opencode-server"],
        "vm-ops-api" => &["vm-ops-api-rs", "watchdog"],
        _ => &[],
    }
}

/// The images a deploy of `app` builds: its own, when it has one, and its companions.
pub fn images_of(app: &str) -> Vec<&'static str> {
    let own = (app != VM_OPS_API).then(|| IMAGES.iter().find(|i| i.name == app_image(app)).map(|i| i.name)).flatten();
    own.into_iter().chain(release_companions(app).iter().copied()).collect()
}

/// A repository at one commit, read through git.
struct Tree<'a> {
    dir: Option<&'a Path>,
    rev: &'a str,
}

impl Tree<'_> {
    fn git(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("git");
        if let Some(dir) = self.dir {
            cmd.current_dir(dir);
        }
        let mut child = cmd.args(args).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("cannot run git: {e}"))?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes()).map_err(|e| format!("git {args:?}: {e}"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("git {args:?}: {e}"))?;
        if !out.status.success() {
            return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim().chars().take(200).collect::<String>()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// A file at the commit. One command hashes five apps' images at two commits, which share most of their files: each is read once per
    /// process (a commit does not move while a command runs; 364 `git show` spawns made one plan take over a second of CPU).
    fn show(&self, path: &str) -> Result<String, String> {
        /// (repository directory, commit, path) and what git answered.
        type Shown = HashMap<(String, String, String), Result<String, String>>;
        thread_local! {
            static SHOWN: RefCell<Shown> = RefCell::new(HashMap::new());
        }
        let key = (self.dir.map(|d| d.display().to_string()).unwrap_or_default(), self.rev.to_string(), path.to_string());
        if let Some(known) = SHOWN.with(|c| c.borrow().get(&key).cloned()) {
            return known;
        }
        let read = self.git(&["show", &format!("{}:{path}", self.rev)], None).map_err(|e| format!("{path} at {}: {e}", self.rev));
        SHOWN.with(|c| c.borrow_mut().insert(key, read.clone()));
        read
    }

    /// `mode blob path` of every file under `paths` that `ignored` does not keep out of the build context; a path with no file is an
    /// error, never an empty input.
    fn files(&self, paths: &BTreeSet<String>, ignored: &dyn Fn(&str) -> bool) -> Result<String, String> {
        let all = self.listing()?;
        let under = |f: &str, path: &String| f == path || f.strip_prefix(path.as_str()).is_some_and(|rest| rest.starts_with('/'));
        let mut out = String::new();
        for line in all.lines() {
            if line.split_once('\t').is_some_and(|(_, f)| paths.iter().any(|p| under(f, p)) && !ignored(f)) {
                out.push_str(line);
                out.push('\n');
            }
        }
        for path in paths {
            if !all.lines().filter_map(|l| l.split_once('\t')).any(|(_, f)| under(f, path)) {
                return Err(format!("{path} has no file at {}", self.rev));
            }
        }
        Ok(out)
    }

    /// Every file of the commit, read once per process: one `ls-tree` per image and commit was 15k spawns in the deploy flow tests.
    fn listing(&self) -> Result<String, String> {
        thread_local! {
            static LISTED: RefCell<HashMap<(String, String), String>> = RefCell::new(HashMap::new());
        }
        let key = (self.dir.map(|d| d.display().to_string()).unwrap_or_default(), self.rev.to_string());
        if let Some(known) = LISTED.with(|c| c.borrow().get(&key).cloned()) {
            return Ok(known);
        }
        let listing = self.git(&["ls-tree", "-r", "--full-tree", self.rev], None)?;
        LISTED.with(|c| c.borrow_mut().insert(key, listing.clone()));
        Ok(listing)
    }
}

/// The lockfile of the Cargo workspace the first of `roots` belongs to: the nearest parent directory whose `Cargo.toml` has a
/// `[workspace]` table (`backend/<service>` for a service with its own workspace, `backend` for one still in the shared one).
fn lock_file(tree: &Tree, roots: &[&str]) -> Result<String, String> {
    let first = roots.first().ok_or("an image with no crate has no lockfile")?;
    let mut dir = Path::new(first).parent();
    while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty()) {
        let manifest = d.join("Cargo.toml").display().to_string();
        if tree.show(&manifest).ok().and_then(|text| text.parse::<toml::Table>().ok()).is_some_and(|t| t.contains_key("workspace")) {
            return Ok(d.join("Cargo.lock").display().to_string());
        }
        dir = d.parent();
    }
    Err(format!("{first} is in no Cargo workspace"))
}

/// The crates `lock_file` resolves for the crates of `roots`, as one string.
fn locked(tree: &Tree, roots: &[&str], lock_file: &str) -> Result<String, String> {
    let names = roots
        .iter()
        .map(|dir| {
            let manifest: toml::Table = tree.show(&format!("{dir}/Cargo.toml"))?.parse().map_err(|e| format!("{dir}/Cargo.toml: {e}"))?;
            manifest.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str).map(str::to_string).ok_or_else(|| format!("{dir}/Cargo.toml has no package name"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let lock: toml::Table = tree.show(lock_file)?.parse().map_err(|e| format!("{lock_file}: {e}"))?;
    Ok(crate::lock_closure::cargo_closure(&lock, &names).into_iter().collect::<Vec<_>>().join("\n"))
}

/// The names a crate takes from its workspace (`dep.workspace = true` or `dep = { workspace = true, ... }`), in any dependency table.
fn inherited_names(manifest: &toml::Table, out: &mut BTreeSet<String>) {
    let mut tables: Vec<&toml::Table> = ["dependencies", "dev-dependencies", "build-dependencies"].iter().filter_map(|k| manifest.get(*k).and_then(toml::Value::as_table)).collect();
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            tables.extend(["dependencies", "dev-dependencies", "build-dependencies"].iter().filter_map(|k| target.get(*k).and_then(toml::Value::as_table)));
        }
    }
    for table in tables {
        for (name, spec) in table {
            if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                out.insert(name.clone());
            }
        }
    }
}

/// What an image takes from its workspace manifest: the file without `members` and without the `[workspace.dependencies]` entries no
/// crate of the image's closure inherits, so a service moving to a workspace of its own, a new service, or a dependency only another
/// service uses does not change this image (the profiles, the resolver and the entries it uses still do). Re-serialised, so a comment or
/// the order of entries does not either. A manifest with no `[workspace]` is returned as it is.
fn workspace_part(tree: &Tree, manifest: &str, closure: &BTreeSet<String>) -> Result<String, String> {
    let text = tree.show(manifest)?;
    let mut table: toml::Table = text.parse().map_err(|e| format!("{manifest}: {e}"))?;
    let Some(workspace) = table.get_mut("workspace").and_then(toml::Value::as_table_mut) else { return Ok(text) };
    workspace.remove("members");
    workspace.remove("default-members");
    let mut used = BTreeSet::new();
    for dir in closure {
        if let Some(Ok(crate_manifest)) = tree.show(&format!("{dir}/Cargo.toml")).ok().map(|t| t.parse::<toml::Table>()) {
            inherited_names(&crate_manifest, &mut used);
        }
    }
    if let Some(deps) = workspace.get_mut("dependencies").and_then(toml::Value::as_table_mut) {
        deps.retain(|name, _| used.contains(name));
    }
    serde_json::to_string(&table).map_err(|e| format!("{manifest}: {e}"))
}

/// What the build context leaves out: the `.dockerignore` beside the Dockerfile (BuildKit reads `<Dockerfile>.dockerignore` first), else the one at
/// the context root. A file the context never holds is in no image, so it is no input of the hash.
fn build_ignore(tree: &Tree, spec: &Image) -> DockerIgnore {
    let root = if spec.context == "." { ".dockerignore".to_string() } else { format!("{}/.dockerignore", spec.context) };
    [format!("{}.dockerignore", spec.dockerfile), root].iter().find_map(|path| tree.show(path).ok()).map_or_else(DockerIgnore::default, |text| DockerIgnore::parse(&text))
}

/// A repository path as the build context names it.
fn in_context(context: &str, file: &str) -> String {
    if context == "." {
        return file.to_string();
    }
    file.strip_prefix(context).and_then(|rest| rest.strip_prefix('/')).unwrap_or(file).to_string()
}

/// The hash of `image` as of commit `rev` in the repository at `dir` (the working directory without one).
pub fn hash(dir: Option<&Path>, rev: &str, image: &str) -> Result<String, String> {
    let spec = IMAGES.iter().find(|i| i.name == image).ok_or_else(|| format!("unknown image {image}"))?;
    let tree = Tree { dir, rev };
    let text = tree.show(spec.dockerfile)?;
    let under = |p: &str| if spec.context == "." { p.to_string() } else { format!("{}/{p}", spec.context) };
    let mut sources: BTreeSet<String> = copy_sources(&text).into_iter().map(|p| under(&p)).collect();
    if sources.contains(&under(".")) {
        return Err(format!("{} copies its whole context, which no hash can bound", spec.dockerfile));
    }
    sources.insert(spec.dockerfile.to_string());
    let mut lock = String::new();
    if !spec.roots.is_empty() {
        let closure = path_closure_by(&|p| tree.show(p), spec.roots)?;
        let lock_path = lock_file(&tree, spec.roots)?;
        let manifest_path = lock_path.replace("Cargo.lock", "Cargo.toml");
        // a workspace crate outside the closure is copied only so cargo finds every member; the lockfile counts through its closure and
        // the workspace manifest through the part of it the closure uses
        sources.retain(|s| *s != lock_path && *s != manifest_path && (closure.contains(s) || tree.show(&format!("{s}/Cargo.toml")).is_err()));
        lock = format!("{}\n{}", locked(&tree, spec.roots, &lock_path)?, workspace_part(&tree, &manifest_path, &closure)?);
    }
    let ignore = build_ignore(&tree, spec);
    let digest = format!("{}\n{lock}", tree.files(&sources, &|file| ignore.excludes(&in_context(spec.context, file)))?);
    let hash = tree.git(&["hash-object", "--stdin"], Some(&digest))?;
    Ok(hash.trim().chars().take(24).collect())
}

/// The hash of everything a deploy of `app` is built from at `rev`: its images' hashes and its compose project, which no image holds
/// (the `docker-compose*` files beside its Dockerfile; for vm-ops-api, the compose projects it swaps itself and the watchdog in from).
pub fn leg_hash(dir: Option<&Path>, rev: &str, app: &str) -> Result<String, String> {
    let tree = Tree { dir, rev };
    let mut parts = images_of(app).into_iter().map(|i| hash(dir, rev, i).map(|h| format!("{i} {h}"))).collect::<Result<Vec<_>, _>>()?;
    let compose: BTreeSet<String> = if app == VM_OPS_API {
        ["infra/oracle-cloud/vm-ops-api", "infra/oracle-cloud/watchdog"].map(String::from).into()
    } else if let Some(own) = IMAGES.iter().find(|i| i.name == app_image(app)) {
        let beside = COMPOSE_DIRS.iter().find(|(a, _)| *a == app).map(|(_, d)| *d).unwrap_or_else(|| own.dockerfile.rsplit_once('/').map_or("", |(d, _)| d));
        // the files directly in `beside`, from the commit's one listing
        let all = tree.listing()?;
        all.lines()
            .filter_map(|l| l.split_once('\t').map(|(_, f)| f))
            .filter(|f| f.strip_prefix(beside).and_then(|r| r.strip_prefix('/')).is_some_and(|name| !name.contains('/') && name.starts_with("docker-compose")))
            .map(String::from)
            .collect()
    } else {
        BTreeSet::new()
    };
    if !compose.is_empty() {
        parts.push(tree.files(&compose, &|_| false)?);
    }
    tree.git(&["hash-object", "--stdin"], Some(&parts.join("\n"))).map(|h| h.trim().chars().take(24).collect())
}
