//! `skills.discover`: the pinned host's catalog of skill roots (`buildSkillDiscoverySources`: the home roots of each agent, and the repo roots of
//! every repo and of the asked folder) read through `skills.list`, one call per workspace. A home root is named below `~`, because the
//! executor never says where its home is; a repo with no path is named by its workspace.
use crate::agent::ask;
use crate::session::{now_ms, text};
use agent_api_catalog::SKILLS_LIST;
use orca_core::wire::Failure;
use orca_core::AgentApi;
use orca_map_workspace::Target;
use serde_json::{json, Value};
use std::cmp::Ordering;

struct Root {
    id: &'static str,
    label: &'static str,
    dir: &'static str,
    kind: &'static str,
    providers: &'static [&'static str],
    owner: Option<&'static str>,
}

const SHARED: &[&str] = &["agent-skills"];

const fn home(id: &'static str, label: &'static str, dir: &'static str, owner: &'static str) -> Root {
    Root { id, label, dir, kind: "home", providers: SHARED, owner: Some(owner) }
}

/// The host's home roots, in its order (sources are answered sorted by label).
const HOME: &[Root] = &[
    Root { id: "home-codex", label: "Codex home", dir: ".codex/skills", kind: "home", providers: &["codex"], owner: Some("codex") },
    Root { id: "home-agents", label: "Agent skills home", dir: ".agents/skills", kind: "home", providers: SHARED, owner: None },
    Root { id: "codex-plugin-cache", label: "Codex plugin cache", dir: ".codex/plugins/cache", kind: "plugin", providers: &["codex", "agent-skills"], owner: Some("codex") },
    Root { id: "home-claude", label: "Claude Code home", dir: ".claude/skills", kind: "home", providers: &["claude"], owner: Some("claude") },
    home("home-cursor", "Cursor home", ".cursor/skills", "cursor"),
    home("home-gemini", "Gemini CLI home", ".gemini/skills", "gemini"),
    home("home-droid", "Droid home", ".factory/skills", "droid"),
    home("home-continue", "Continue home", ".continue/skills", "continue"),
    home("home-trae", "Trae home", ".trae-cn/skills", "trae"),
    home("home-grok", "Grok home", ".grok/skills", "grok"),
    home("home-aug", "Augment home", ".augment/skills", "aug"),
    home("home-zcode", "ZCode home", ".zcode/skills", "zcode"),
    home("home-codebuddy", "CodeBuddy home", ".codebuddy/skills", "codebuddy"),
    home("home-kiro", "Kiro home", ".kiro/skills", "kiro"),
    home("home-antigravity", "Antigravity home", ".gemini/config/skills", "antigravity"),
    home("home-openclaude", "OpenClaude home", ".openclaude/skills", "openclaude"),
    home("home-opencode", "OpenCode home", ".config/opencode/skills", "opencode"),
    home("home-pi", "Pi home", ".pi/agent/skills", "pi"),
    home("home-omp", "OMP home", ".omp/agent/skills", "omp"),
    home("home-qoder", "Qoder home", ".qoder/skills", "qoder"),
    home("home-hermes", "Hermes home", ".hermes/skills", "hermes"),
    home("home-prime-agent", "Prime Agent home", ".prime/agent/skills", "prime-agent"),
    home("home-muse", "Muse home", ".config/muse/skills", "muse"),
];

/// The host's repo roots of one repo: (id prefix, directory, providers, owner); the label is `Repo <name> <first segment>`.
const REPO: &[(&str, &str, &[&str], Option<&str>)] = &[
    ("agents", ".agents/skills", SHARED, None),
    ("claude", ".claude/skills", &["claude"], Some("claude")),
    ("droid", ".factory/skills", SHARED, Some("droid")),
    ("continue", ".continue/skills", SHARED, Some("continue")),
    ("trae", ".trae/skills", SHARED, Some("trae")),
    ("grok", ".grok/skills", SHARED, Some("grok")),
    ("aug", ".augment/skills", SHARED, Some("aug")),
    ("codebuddy", ".codebuddy/skills", SHARED, Some("codebuddy")),
    ("kiro", ".kiro/skills", SHARED, Some("kiro")),
    ("openclaude", ".openclaude/skills", SHARED, Some("openclaude")),
    ("qoder", ".qoder/skills", SHARED, Some("qoder")),
    ("opencode", ".opencode/skills", SHARED, Some("opencode")),
];

/// The host's `stablePathId` is a sha1 prefix; any stable digest serves, since the id only has to stay the same for the same path.
fn stable_id(path: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(path.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn fold(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// One repo the discovery covers: its workspace (none for an asked folder no workspace holds) and the path its roots are named under.
struct Repo {
    workspace: Option<String>,
    base: String,
}

impl Repo {
    fn of(t: &Target) -> Repo {
        Repo { workspace: Some(t.workspace.clone()), base: if t.root_path.is_empty() { t.workspace.clone() } else { t.root_path.trim_end_matches('/').to_string() } }
    }

    fn name(&self) -> &str {
        self.base.rsplit('/').find(|p| !p.is_empty()).unwrap_or(&self.base)
    }
}

/// A source row, with whether the executor saw its directory (`None`: nobody could look).
fn source(id: String, label: String, path: String, kind: &str, providers: &[&str], owner: Option<&str>, seen: Option<bool>) -> Value {
    let mut row = json!({"id": id, "label": label, "path": path, "sourceKind": kind, "providers": providers, "owner": owner, "exists": seen == Some(true)});
    match seen {
        Some(true) => {}
        Some(false) => row["skippedReason"] = json!("missing"),
        None => row["skippedReason"] = json!("unavailable"),
    }
    row
}

pub(crate) fn discover(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    if params.get("runtime").is_some_and(|r| !r.is_null() && r != "host" && r != "wsl") {
        return Err(Failure::invalid("Invalid option: expected one of \"host\"|\"wsl\""));
    }
    let all = orca_map_workspace::targets(agent)?;
    let focus = match (text(params, "worktreeId"), text(params, "cwd")) {
        (Some(id), _) => Some(Ok(orca_map_workspace::resolve(agent, &format!("id:{id}"))?)),
        (None, Some(cwd)) => Some(orca_map_workspace::resolve(agent, &format!("path:{cwd}")).map_err(|_| cwd.to_string())),
        (None, None) => None,
    };
    let mut repos: Vec<Repo> = Vec::new();
    let mut add = |r: Repo| {
        if !repos.iter().any(|x| x.base == r.base) {
            repos.push(r);
        }
    };
    match &focus {
        Some(Ok(t)) => add(Repo::of(t)),
        Some(Err(cwd)) => add(Repo { workspace: None, base: cwd.trim_end_matches('/').to_string() }),
        None => {}
    }
    all.iter().for_each(|t| add(Repo::of(t)));
    let home_from = repos.iter().find_map(|r| r.workspace.clone());

    let repo_dirs: Vec<&str> = REPO.iter().map(|r| r.1).collect();
    let home_dirs: Vec<&str> = HOME.iter().map(|r| r.dir).collect();
    let (mut sources, mut skills) = (Vec::new(), Vec::new());
    let mut home_present: Option<Vec<Value>> = None;
    for repo in &repos {
        let scanned = repo.workspace.as_ref().and_then(|w| {
            let homes: &[&str] = if home_from.as_ref() == Some(w) { &home_dirs } else { &[] };
            ask(agent, SKILLS_LIST.name(), json!({"workspace": w, "dirs": repo_dirs, "home_dirs": homes})).ok()
        });
        let present = |scope: &str, dir: &str| scanned.as_ref().map(|s| s["present"].as_array().into_iter().flatten().any(|p| p["scope"] == scope && p["dir"] == dir));
        let stable = stable_id(&repo.base);
        for (key, dir, providers, owner) in REPO {
            let segment = dir.split('/').next().unwrap_or(dir);
            sources.push(source(format!("repo-{key}-{stable}"), format!("Repo {} {segment}", repo.name()), format!("{}/{dir}", repo.base), "repo", providers, *owner, present("workspace", dir)));
        }
        if repo.workspace.is_some() && repo.workspace == home_from {
            home_present = scanned.as_ref().map(|s| s["present"].as_array().cloned().unwrap_or_default());
        }
        for s in scanned.iter().flat_map(|s| s["skills"].as_array().cloned().unwrap_or_default()) {
            let (Some(dir), Some(name), Some(path)) = (s["dir"].as_str(), s["name"].as_str(), s["path"].as_str()) else { continue };
            let in_home = s["scope"] == "home";
            let (label, root, providers, kind) = if in_home {
                let Some(r) = HOME.iter().find(|r| r.dir == dir) else { continue };
                (r.label.to_string(), format!("~/{dir}"), r.providers, r.kind)
            } else {
                let Some((_, _, providers, _)) = REPO.iter().find(|r| r.1 == dir) else { continue };
                (format!("Repo {} {}", repo.name(), dir.split('/').next().unwrap_or(dir)), format!("{}/{dir}", repo.base), *providers, "repo")
            };
            let file = if in_home { path.to_string() } else { format!("{}/{path}", repo.base) };
            let directory = file.rsplit_once('/').map_or(file.clone(), |(d, _)| d.to_string());
            if skills.iter().any(|k: &Value| k["skillFilePath"] == file.as_str()) {
                continue;
            }
            skills.push(json!({"id": stable_id(&file), "name": name, "description": s["description"], "providers": providers, "sourceKind": kind, "sourceLabel": label, "rootPath": root, "directoryPath": directory, "skillFilePath": file, "installed": true, "updatedAt": null}));
        }
    }
    for r in HOME {
        let seen = home_present.as_ref().map(|p| p.iter().any(|x| x["scope"] == "home" && x["dir"] == r.dir));
        sources.push(source(r.id.to_string(), r.label.to_string(), format!("~/{}", r.dir), r.kind, r.providers, r.owner, seen));
    }

    let kinds: Option<Vec<&str>> = params["sourceKinds"].as_array().filter(|k| !k.is_empty()).map(|k| k.iter().filter_map(Value::as_str).collect());
    let names: Option<Vec<String>> = params["names"].as_array().map(|n| n.iter().filter_map(Value::as_str).map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()).filter(|n: &Vec<String>| !n.is_empty());
    let kind_ok = |k: &str| kinds.as_ref().is_none_or(|ks| ks.contains(&k) || (k == "home" && ks.contains(&"bundled")));
    sources.retain(|s| kind_ok(s["sourceKind"].as_str().unwrap_or_default()));
    skills.retain(|s| {
        let kind = kinds.as_ref().is_none_or(|ks| ks.contains(&s["sourceKind"].as_str().unwrap_or_default()));
        let folder = s["directoryPath"].as_str().and_then(|d| d.rsplit('/').next()).unwrap_or_default().to_lowercase();
        let name = s["name"].as_str().unwrap_or_default().trim().to_lowercase();
        kind && names.as_ref().is_none_or(|n| n.contains(&name) || n.contains(&folder))
    });
    let field = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    skills.sort_by(|a, b| fold(&field(a, "name"), &field(b, "name")).then_with(|| fold(&field(a, "sourceLabel"), &field(b, "sourceLabel"))).then_with(|| field(a, "skillFilePath").cmp(&field(b, "skillFilePath"))));
    sources.sort_by(|a, b| fold(&field(a, "label"), &field(b, "label")));
    Ok(json!({"skills": skills, "sources": sources, "scannedAt": now_ms()}))
}
