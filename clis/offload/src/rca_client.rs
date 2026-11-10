//! An ask served by the rca service: `POST /runs` on the remote-chat-agent over the tailnet. The service owns
//! opencode, the provider keys and the ladder with its shared cooldowns, so a caller needs none of them and has no local rung to
//! fall back on: an unreachable service is the answer, with the next step.
use cli_http::Request;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const DEFAULT_HOST: &str = "remote-chat-agent-prod";
const DEFAULT_PORT: &str = "8787";
const SECRET_KEY: &str = "CHAT_AGENT_API_SECRET";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45 * 60);

/// What the service answered to one run: the HTTP status and its JSON body.
pub struct Answered {
    pub status: u16,
    pub body: Value,
}

/// One request to the service; a refusal is an `Answered` with its status, an unreachable service an `Err` with the next step.
pub fn send(method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Answered, String> {
    let secret = secrets::load_values(&[SECRET_KEY]).get(SECRET_KEY).cloned().flatten().ok_or(format!("{SECRET_KEY} could not be resolved (`wf secret get {SECRET_KEY}`)"))?;
    let host = std::env::var("RCA_HOST").ok().filter(|h| !h.is_empty()).unwrap_or_else(|| DEFAULT_HOST.into());
    let port = std::env::var("RCA_PORT").ok().filter(|p| !p.is_empty()).unwrap_or_else(|| DEFAULT_PORT.into());
    let mut request = Request::new(method, &format!("http://{host}:{port}{path}")).via(wf_vmops::vmops::tailnet_via()).header("x-bot-secret", &secret).timeout(timeout);
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().map_err(|e| format!("the rca service at {host}:{port} is unreachable: {e} (the tailnet: `wf vm net up`; the service: `wf vm ops stats`, container remote-chat-agent)"))?;
    let body = serde_json::from_str(&response.text()).unwrap_or(Value::Null);
    Ok(Answered { status: response.status, body })
}

/// One `POST /runs`.
pub fn call(request: &Value, timeout: Duration) -> Result<Answered, String> {
    send("POST", "/runs", Some(request), timeout)
}

fn refused(answered: &Answered) -> String {
    format!("the rca service answered {}: {}", answered.status, answered.body["error"].as_str().unwrap_or("no reason given"))
}

/// The service's run log as text, for `--stats` and `--savings`.
pub fn run_log() -> Result<String, String> {
    let answered = send("GET", "/runs/log", None, Duration::from_secs(60))?;
    if answered.status != 200 {
        return Err(refused(&answered));
    }
    Ok(answered.body["log"].as_str().unwrap_or_default().to_string())
}

/// Warms `pool` again in the shared ledger; whether it was cold.
pub fn clear_cooldown(pool: &str) -> Result<bool, String> {
    let answered = send("POST", "/runs/cooldowns/clear", Some(&json!({ "pool": pool })), Duration::from_secs(60))?;
    if answered.status != 200 {
        return Err(refused(&answered));
    }
    Ok(answered.body["wasCold"].as_bool().unwrap_or(false))
}

/// Writes what a run changed into `root` (created or rewritten files, removals), refusing a path that leaves it; the lines say what
/// was done, one per file.
pub fn apply_changes(root: &Path, changes: &[Value]) -> Result<Vec<String>, String> {
    use base64::Engine;
    let mut lines = Vec::new();
    for change in changes {
        let path = change["path"].as_str().unwrap_or_default();
        if path.is_empty() || Path::new(path).components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(format!("the service returned a path that leaves the repository: {path:?}"));
        }
        let target = root.join(path);
        if change["deleted"].as_bool() == Some(true) {
            let _ = std::fs::remove_file(&target);
            lines.push(format!("[delete] {path}"));
            continue;
        }
        let bytes = match (change["content"].as_str(), change["contentBase64"].as_str()) {
            (Some(text), _) => text.as_bytes().to_vec(),
            (None, Some(encoded)) => base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| format!("{path}: {e}"))?,
            _ => continue,
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{path}: {e}"))?;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("{path}: {e}"))?;
        lines.push(format!("[write] {path}"));
    }
    Ok(lines)
}

/// One `--files` entry: `path` (inside `root`, staged at the same relative path) or `source=staged` (any readable file, staged at the
/// relative path after the `=`, which a prompt can then name). Returns where to read it and where it is staged.
fn staged_path(root: &Path, entry: &str) -> Result<(PathBuf, String), String> {
    let (source, staged) = match entry.split_once('=') {
        Some((source, staged)) => (PathBuf::from(source), staged.to_string()),
        None => {
            let given = PathBuf::from(entry);
            let relative = if given.is_absolute() { given.strip_prefix(root).map(Path::to_path_buf).map_err(|_| format!("--files {entry} is outside {}: stage it as {entry}=<relative path>", root.display()))? } else { given };
            (relative.clone(), relative.to_string_lossy().into_owned())
        }
    };
    if staged.is_empty() || Path::new(&staged).components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("--files {entry}: {staged:?} must be a relative path that stays inside the directory"));
    }
    Ok((if source.is_absolute() { source } else { root.join(source) }, staged))
}

/// The request body of one read-only run: the prompt, the shape, the pinned models and the files the prompt reads. Text goes as
/// `content`, anything else (a screenshot) as `contentBase64`.
/// What a run asks of opencode beside its prompt: the session to continue, the agent, the model variant, the new session's title and
/// whether it may edit and run commands.
pub struct Extras<'a> {
    pub session: Option<&'a str>,
    pub agent: Option<&'a str>,
    pub variant: Option<&'a str>,
    pub title: Option<&'a str>,
    pub auto: bool,
    /// The `-f` files: the model gets them with the message, not by a tool call.
    pub attach: &'a [String],
    /// A project command to run with the prompt as its arguments.
    pub command: Option<&'a str>,
}

pub fn request(prompt: &str, shape: &str, models: &[String], files: &[String], caller: &str, extras: &Extras, root: &Path) -> Result<Value, String> {
    use base64::Engine;
    let mut attached = Vec::new();
    for entry in files {
        let (source, staged) = staged_path(root, entry)?;
        let bytes = std::fs::read(&source).map_err(|e| format!("--files {entry}: {e}"))?;
        attached.push(match String::from_utf8(bytes) {
            Ok(text) => json!({ "path": staged, "content": text }),
            Err(binary) => json!({ "path": staged, "contentBase64": base64::engine::general_purpose::STANDARD.encode(binary.into_bytes()) }),
        });
    }
    // The project's own opencode config and instructions go along, as `opencode run` reads them from its directory; a file the caller
    // named itself wins.
    let named: std::collections::HashSet<String> = attached.iter().filter_map(|f| f["path"].as_str().map(str::to_string)).collect();
    for (path, bytes) in project_files(root) {
        if named.contains(&path) {
            continue;
        }
        attached.push(match String::from_utf8(bytes) {
            Ok(text) => json!({ "path": path, "content": text }),
            Err(binary) => json!({ "path": path, "contentBase64": base64::engine::general_purpose::STANDARD.encode(binary.into_bytes()) }),
        });
    }
    let mut body = json!({ "prompt": prompt, "shape": shape, "files": attached, "caller": caller });
    if !models.is_empty() {
        body["models"] = json!(models);
    }
    for (key, value) in [("session", extras.session), ("agent", extras.agent), ("variant", extras.variant)] {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            body[key] = json!(value);
        }
    }
    if let Some(title) = extras.title {
        // a bare `--title` asks for the prompt's first line
        body["title"] = json!(if title.is_empty() { prompt.lines().next().unwrap_or_default().chars().take(80).collect::<String>() } else { title.to_string() });
    }
    body["auto"] = json!(extras.auto);
    if let Some(command) = extras.command.filter(|c| !c.is_empty()) {
        body["command"] = json!(command);
    }
    if !extras.attach.is_empty() {
        body["attach"] = json!(extras.attach.iter().map(|entry| staged_path(root, entry).map(|(_, staged)| staged)).collect::<Result<Vec<_>, _>>()?);
    }
    Ok(body)
}

/// What opencode reads from a project directory that the service, with no checkout, has to be sent: the config (`opencode.json[c]`),
/// the instruction files (`AGENTS.md`, `CLAUDE.md`, and what the config's `instructions` names) and the `.opencode` directories of
/// agents, commands, skills, plugins, tools and modes, the skills of `.claude` and `.agents`, and the files those name (`{file:path}` in
/// a config or an agent, `@path` in a command). A local MCP server stays in the config only when it runs a script of the project with
/// a runtime the service has (`node`, `bun`, `python3`, `sh`): one that installs a package (`npx`) is dropped, as is a file over
/// 512 KB, a `node_modules` and `.opencode/bin`.
pub fn project_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    const DIRS: [&str; 12] = ["agent", "agents", "command", "commands", "skill", "skills", "plugin", "plugins", "tool", "tools", "mode", "modes"];
    const MAX_BYTES: u64 = 512 * 1024;
    const MAX_FILES: usize = 200;
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if entry.file_name() == "node_modules" || entry.file_name() == ".git" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.metadata().is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES) {
                if let Ok(relative) = path.strip_prefix(root) {
                    out.push(relative.to_string_lossy().into_owned());
                }
            }
        }
    }
    // `*` matches within one path segment and `**` any number of them.
    fn matches(pattern: &[&str], path: &[&str]) -> bool {
        match (pattern.first(), path.first()) {
            (None, None) => true,
            (Some(&"**"), _) => matches(&pattern[1..], path) || (!path.is_empty() && matches(pattern, &path[1..])),
            (Some(p), Some(name)) => segment(p, name) && matches(&pattern[1..], &path[1..]),
            _ => false,
        }
    }
    fn segment(pattern: &str, name: &str) -> bool {
        match pattern.split_once('*') {
            None => pattern == name,
            Some((head, tail)) => name.starts_with(head) && (0..=name.len().saturating_sub(head.len())).any(|i| segment(tail, &name[head.len() + i..])),
        }
    }
    let mut wanted: Vec<String> = Vec::new();
    // AGENTS.md wins over CLAUDE.md, as in opencode
    let rules = if root.join("AGENTS.md").is_file() { "AGENTS.md" } else { "CLAUDE.md" };
    for name in ["opencode.json", "opencode.jsonc", rules, ".opencode/opencode.json", ".opencode/opencode.jsonc"] {
        if root.join(name).is_file() {
            wanted.push(name.to_string());
        }
    }
    for dir in DIRS {
        walk(root, &root.join(".opencode").join(dir), &mut wanted);
    }
    for dir in [".claude/skills", ".agents/skills"] {
        walk(root, &root.join(dir), &mut wanted);
    }
    // what the config's `instructions` names: a path or a glob, relative to the project
    let config: Option<Value> = ["opencode.json", ".opencode/opencode.json"].iter().find_map(|f| std::fs::read_to_string(root.join(f)).ok()).and_then(|t| serde_json::from_str(&t).ok());
    for entry in config.as_ref().and_then(|c| c["instructions"].as_array()).into_iter().flatten().filter_map(Value::as_str) {
        if entry.starts_with('/') || entry.contains("..") || entry.contains("://") {
            continue;
        }
        if entry.contains('*') {
            let mut all = Vec::new();
            walk(root, root, &mut all);
            let pattern: Vec<&str> = entry.split('/').collect();
            wanted.extend(all.into_iter().filter(|f| !f.starts_with(".git/") && matches(&pattern, &f.split('/').collect::<Vec<_>>())));
        } else if root.join(entry).is_file() {
            wanted.push(entry.to_string());
        }
    }
    // the files that configs, agents and commands name: `{file:path}` (relative to the project or to the file) and `@path`
    let mut named: Vec<String> = Vec::new();
    for relative in &wanted {
        let is_config = relative.ends_with("opencode.json") || relative.ends_with("opencode.jsonc");
        let is_agent_or_command = relative.starts_with(".opencode/") && relative.ends_with(".md") && (relative.contains("/agent") || relative.contains("/command") || relative.contains("/mode"));
        if !is_config && !is_agent_or_command {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(relative)) else { continue };
        let own_dir = Path::new(relative).parent().unwrap_or(Path::new(""));
        // the script a kept local MCP server runs
        if is_config {
            if let Some(servers) = serde_json::from_str::<Value>(&text).ok().and_then(|c| c["mcp"].as_object().cloned()) {
                for server in servers.values().filter(|s| s["type"] == "local") {
                    for arg in server["command"].as_array().into_iter().flatten().skip(1).filter_map(Value::as_str) {
                        let arg = arg.trim_start_matches("./");
                        if !arg.starts_with('/') && !arg.contains("..") && root.join(arg).is_file() {
                            named.push(arg.to_string());
                        }
                    }
                }
            }
        }
        let mut rest = text.as_str();
        while let Some(at) = rest.find("{file:") {
            rest = &rest[at + 6..];
            let Some(end) = rest.find('}') else { break };
            let target = rest[..end].trim().trim_start_matches("./");
            for base in [Path::new(""), own_dir] {
                let candidate = base.join(target);
                if !target.is_empty() && !target.starts_with('/') && !target.contains("..") && root.join(&candidate).is_file() {
                    named.push(candidate.to_string_lossy().into_owned());
                    break;
                }
            }
        }
        if is_agent_or_command && relative.contains("/command") {
            for token in text.split(|c: char| c.is_whitespace() || c == '`' || c == '"' || c == '\'' || c == '(' || c == ')').filter(|t| t.starts_with('@') && t.len() > 1) {
                let target = token[1..].trim_end_matches(['.', ',', ';', ':']);
                if !target.starts_with('/') && !target.contains("..") && root.join(target).is_file() {
                    named.push(target.to_string());
                }
            }
        }
    }
    wanted.extend(named);
    wanted.sort();
    wanted.dedup();
    wanted.truncate(MAX_FILES);
    wanted
        .into_iter()
        .filter_map(|relative| {
            let bytes = std::fs::read(root.join(&relative)).ok()?;
            let bytes = if relative.ends_with("opencode.json") || relative.ends_with("opencode.jsonc") { without_local_mcp(bytes) } else { bytes };
            Some((relative, bytes))
        })
        .collect()
}

/// The config without the local MCP servers the service cannot run (those that install a package first); anything that is not plain
/// JSON goes as it is.
fn without_local_mcp(bytes: Vec<u8>) -> Vec<u8> {
    let Ok(mut config) = serde_json::from_slice::<Value>(&bytes) else { return bytes };
    if let Some(servers) = config.get_mut("mcp").and_then(Value::as_object_mut) {
        servers.retain(|_, server| {
            server["type"] != "local" || server["command"].get(0).and_then(Value::as_str).is_some_and(|program| matches!(program, "node" | "bun" | "python3" | "python" | "sh" | "bash"))
        });
    }
    serde_json::to_vec_pretty(&config).unwrap_or(bytes)
}

pub fn timeout_of(ms: Option<u64>) -> Duration {
    ms.map_or(DEFAULT_TIMEOUT, Duration::from_millis)
}
