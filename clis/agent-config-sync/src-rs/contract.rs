//! Loads and validates the `.agents/` contract. Mirrors the zod schemas of the TypeScript
//! version, including zod's key order (object parsing re-emits keys in schema order).
use crate::fsx::{list_dir, list_files, lstat, read_text, read_text_or_null};
use crate::paths::{basename, join};
use crate::{Res, SHARED_SKILLS_DIR, SOURCES_PATH};
use regex::Regex;
use serde_json::{Map, Number, Value};
use std::path::Path;

const PLACEHOLDER: &str = "__PROJECT_ROOT__";

pub struct Doc {
    pub name: String,
    pub source: String,
    pub body: String,
}

pub struct Instruction {
    pub source: String,
    pub dir: String,
    pub body: String,
}

pub struct Skill {
    pub name: String,
    pub source: String,
    pub files: Vec<(String, String)>,
}

pub struct Claude {
    pub include_shared: bool,
    pub default_mode: Option<String>,
    pub deny: Vec<String>,
    pub allow: Vec<String>,
}

pub struct Opencode {
    pub include_shared: bool,
    pub permission: Map<String, Value>,
}

pub struct Codex {
    pub include_shared: bool,
    pub approval_policy: String,
    pub default_permissions: String,
    pub permissions: Map<String, Value>,
    pub rules: Vec<String>,
}

pub struct Permissions {
    pub shared_deny: Vec<String>,
    pub claude: Claude,
    pub opencode: Opencode,
    pub codex: Codex,
}

pub struct Contract {
    /// server name -> `{default, opencode?, claude?, codex?}` with placeholders resolved.
    pub mcps: Map<String, Value>,
    pub hooks_claude: Map<String, Value>,
    pub hooks_opencode: Map<String, Value>,
    pub hooks_codex: Map<String, Value>,
    pub permissions: Permissions,
    pub plugins_opencode: Vec<Value>,
    pub claude_settings: Map<String, Value>,
    pub setup: String,
    pub instructions: Vec<Instruction>,
    pub commands: Vec<Doc>,
    pub rules: Vec<Doc>,
    pub skills: Vec<Skill>,
}

pub fn fail<T>(path: &str, message: &str) -> Res<T> {
    Err(format!("agent contract: {path}: {message}"))
}

/// JS prints whole-valued floats as integers; keep the output byte-identical.
fn normalize_numbers(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if !n.is_i64() && !n.is_u64() && f.fract() == 0.0 && f.abs() < 1e15 => {
                Value::Number(Number::from(f as i64))
            }
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(normalize_numbers).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (k, normalize_numbers(v)))
                .collect(),
        ),
        other => other,
    }
}

pub fn parse_json(raw: &str, path: &str) -> Res<Value> {
    serde_json::from_str::<Value>(raw)
        .map(normalize_numbers)
        .map_err(|e| format!("{path}: {e}"))
}

fn object<'a>(v: &'a Value, path: &str) -> Res<&'a Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| format!("{path}: expected an object"))
}

fn strict(m: &Map<String, Value>, allowed: &[&str], path: &str) -> Res<()> {
    match m.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => fail(path, &format!("unrecognized key \"{k}\"")),
        None => Ok(()),
    }
}

fn string_array(v: &Value, path: &str) -> Res<Vec<String>> {
    let arr = v
        .as_array()
        .ok_or_else(|| format!("{path}: expected an array"))?;
    arr.iter()
        .map(|i| {
            i.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{path}: expected strings"))
        })
        .collect()
}

fn opt_str_array(m: &Map<String, Value>, key: &str, path: &str) -> Res<Vec<String>> {
    m.get(key).map_or(Ok(Vec::new()), |v| {
        string_array(v, &format!("{path}.{key}"))
    })
}

fn opt_bool(m: &Map<String, Value>, key: &str, path: &str) -> Res<Option<bool>> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => fail(path, &format!("{key} must be a boolean")),
    }
}

fn opt_str(m: &Map<String, Value>, key: &str, path: &str) -> Res<Option<String>> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => fail(path, &format!("{key} must be a string")),
    }
}

fn resolve(s: &str, root: &str) -> String {
    s.replace(PLACEHOLDER, root)
}

fn read_json(root: &str, rel: &str) -> Res<Value> {
    parse_json(&read_text(Path::new(&join(&[root, rel])))?, rel)
}

fn read_json_or_null(root: &str, rel: &str) -> Res<Option<Value>> {
    read_text_or_null(Path::new(&join(&[root, rel])))?
        .map(|raw| parse_json(&raw, rel))
        .transpose()
}

/* ---------- mcps ---------- */

fn resolve_server_config(cfg: &Map<String, Value>, root: &str) -> Map<String, Value> {
    let mut next = Map::new();
    for (key, value) in cfg {
        let resolved = match value {
            Value::String(s) => Value::String(resolve(s, root)),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|i| match i {
                        Value::String(s) => Value::String(resolve(s, root)),
                        other => other.clone(),
                    })
                    .collect(),
            ),
            Value::Object(nested) => Value::Object(
                nested
                    .iter()
                    .map(|(k, v)| {
                        let v = match v {
                            Value::String(s) => Value::String(resolve(s, root)),
                            other => other.clone(),
                        };
                        (k.clone(), v)
                    })
                    .collect(),
            ),
            other => other.clone(),
        };
        next.insert(key.clone(), resolved);
    }
    next
}

pub fn read_mcps(root: &str) -> Res<Map<String, Value>> {
    let path = ".agents/config/mcps.json";
    let raw = read_json(root, path)?;
    let top = object(&raw, path)?;
    strict(top, &["servers"], path)?;
    let servers = object(
        top.get("servers")
            .ok_or_else(|| format!("{path}: servers is required"))?,
        path,
    )?;
    let mut out = Map::new();
    for (name, per) in servers {
        let ctx = format!("{path}: servers.{name}");
        let per = object(per, &ctx)?;
        strict(per, &["default", "opencode", "claude", "codex"], &ctx)?;
        if !per.contains_key("default") {
            return fail(&ctx, "default is required");
        }
        let mut resolved = Map::new();
        for tool in ["default", "opencode", "claude", "codex"] {
            if let Some(cfg) = per.get(tool) {
                let cfg = object(cfg, &format!("{ctx}.{tool}"))?;
                resolved.insert(
                    tool.to_string(),
                    Value::Object(resolve_server_config(cfg, root)),
                );
            }
        }
        out.insert(name.clone(), Value::Object(resolved));
    }
    Ok(out)
}

pub fn pick_slice<'a>(
    mcps: &'a Map<String, Value>,
    server: &str,
    tool: &str,
) -> Option<&'a Map<String, Value>> {
    let per = mcps.get(server)?.as_object()?;
    per.get(tool).or_else(|| per.get("default"))?.as_object()
}

/* ---------- hooks ---------- */

const CLAUDE_HOOKS: &[&str] = &[
    "SessionStart",
    "Setup",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "Notification",
    "MessageDisplay",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "TeammateIdle",
    "InstructionsLoaded",
    "ConfigChange",
    "CwdChanged",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "PreCompact",
    "PostCompact",
    "Elicitation",
    "ElicitationResult",
    "SessionEnd",
];
const OPENCODE_HOOKS: &[&str] = &[
    "session.created",
    "session.updated",
    "session.deleted",
    "session.status",
    "session.error",
    "session.idle",
    "session.compacted",
    "session.diff",
    "tool.execute.before",
    "tool.execute.after",
];
const CODEX_HOOKS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "SessionEnd",
];

/// Fields per handler type in zod's schema order; unknown keys are stripped as zod does.
fn handler_fields(kind: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    // (string fields in order, resolved-for-placeholders fields)
    match kind {
        "command" => Some((&["command"], &["command"])),
        "http" => Some((&["url"], &["url"])),
        "mcp_tool" => Some((&["server", "tool"], &[])),
        "prompt" => Some((&["prompt"], &["prompt"])),
        "agent" => Some((&["agent", "prompt"], &["prompt"])),
        _ => None,
    }
}

fn read_handler(h: &Value, root: &str, path: &str) -> Res<Value> {
    let h = object(h, path)?;
    let kind = h.get("type").and_then(Value::as_str).unwrap_or("");
    let Some((fields, resolved)) = handler_fields(kind) else {
        return fail(path, "invalid hook handler type");
    };
    let mut out = Map::new();
    out.insert("type".into(), Value::String(kind.to_string()));
    for f in fields {
        let Some(Value::String(s)) = h.get(*f) else {
            return fail(path, &format!("{kind} hook needs string {f}"));
        };
        let s = if resolved.contains(f) {
            resolve(s, root)
        } else {
            s.clone()
        };
        out.insert((*f).to_string(), Value::String(s));
    }
    match h.get("timeout") {
        None => {}
        Some(t @ Value::Number(_)) => {
            out.insert("timeout".into(), t.clone());
        }
        Some(_) => return fail(path, "timeout must be a number"),
    }
    Ok(Value::Object(out))
}

fn read_tool_hooks(
    v: Option<&Value>,
    allowed: &[&str],
    root: &str,
    path: &str,
) -> Res<Map<String, Value>> {
    let Some(v) = v else { return Ok(Map::new()) };
    let block = object(v, path)?;
    let hooks = object(
        block
            .get("hooks")
            .ok_or_else(|| format!("{path}: hooks is required"))?,
        path,
    )?;
    let mut out = Map::new();
    for (event, groups) in hooks {
        if !allowed.contains(&event.as_str()) {
            return fail(path, &format!("unknown hook event \"{event}\""));
        }
        let groups = groups
            .as_array()
            .ok_or_else(|| format!("{path}.{event}: expected an array"))?;
        let mut resolved_groups = Vec::new();
        for (i, group) in groups.iter().enumerate() {
            let gpath = format!("{path}.{event}[{i}]");
            let g = object(group, &gpath)?;
            let mut og = Map::new();
            for key in ["matcher", "if"] {
                if let Some(s) = opt_str(g, key, &gpath)? {
                    og.insert(key.into(), Value::String(s));
                }
            }
            let handlers = g
                .get("hooks")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{gpath}: hooks must be an array"))?;
            let handlers = handlers
                .iter()
                .map(|h| read_handler(h, root, &gpath))
                .collect::<Res<Vec<_>>>()?;
            og.insert("hooks".into(), Value::Array(handlers));
            resolved_groups.push(Value::Object(og));
        }
        out.insert(event.clone(), Value::Array(resolved_groups));
    }
    Ok(out)
}

type Hooks = (Map<String, Value>, Map<String, Value>, Map<String, Value>);

fn read_hooks(root: &str) -> Res<Hooks> {
    let path = ".agents/config/hooks.json";
    let raw = read_json(root, path)?;
    let top = object(&raw, path)?;
    strict(top, &["claude", "opencode", "codex"], path)?;
    Ok((
        read_tool_hooks(top.get("claude"), CLAUDE_HOOKS, root, path)?,
        read_tool_hooks(top.get("opencode"), OPENCODE_HOOKS, root, path)?,
        read_tool_hooks(top.get("codex"), CODEX_HOOKS, root, path)?,
    ))
}

/* ---------- permissions ---------- */

const DEFAULT_MODES: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
];
const PROFILE_KEYS: &[&str] = &[
    "extends",
    "description",
    "filesystem",
    "network",
    "workspace_roots",
    "glob_scan_max_depth",
];

fn empty_permissions() -> Permissions {
    Permissions {
        shared_deny: Vec::new(),
        claude: Claude {
            include_shared: true,
            default_mode: None,
            deny: Vec::new(),
            allow: Vec::new(),
        },
        opencode: Opencode {
            include_shared: true,
            permission: Map::new(),
        },
        codex: Codex {
            include_shared: true,
            approval_policy: "on-request".into(),
            default_permissions: "project".into(),
            permissions: Map::new(),
            rules: Vec::new(),
        },
    }
}

fn read_profile(profile: &Value, path: &str) -> Res<Value> {
    let p = object(profile, path)?;
    strict(p, PROFILE_KEYS, path)?;
    let mut out = Map::new();
    for key in PROFILE_KEYS {
        let Some(v) = p.get(*key) else { continue };
        if *key == "network" {
            let n = object(v, path)?;
            strict(n, &["enabled", "domains"], path)?;
            let mut net = Map::new();
            for nk in ["enabled", "domains"] {
                if let Some(nv) = n.get(nk) {
                    net.insert(nk.into(), nv.clone());
                }
            }
            out.insert((*key).into(), Value::Object(net));
        } else {
            out.insert((*key).into(), v.clone());
        }
    }
    Ok(Value::Object(out))
}

fn read_permissions(root: &str) -> Res<Permissions> {
    let path = ".agents/config/permissions.json";
    let Some(raw) = read_json_or_null(root, ".agents/config/permissions.json")? else {
        return Ok(empty_permissions());
    };
    let top = object(&raw, path)?;
    strict(top, &["shared", "claude", "opencode", "codex"], path)?;
    let resolve_all = |v: Vec<String>| v.iter().map(|s| resolve(s, root)).collect::<Vec<_>>();

    let shared_deny = match top.get("shared") {
        None => Vec::new(),
        Some(s) => {
            let s = object(s, path)?;
            strict(s, &["deny"], path)?;
            resolve_all(opt_str_array(s, "deny", path)?)
        }
    };

    let mut p = empty_permissions();
    p.shared_deny = shared_deny;

    if let Some(c) = top.get("claude") {
        let c = object(c, path)?;
        strict(c, &["includeShared", "permissions"], path)?;
        p.claude.include_shared = opt_bool(c, "includeShared", path)?.unwrap_or(true);
        if let Some(perms) = c.get("permissions") {
            let perms = object(perms, path)?;
            strict(perms, &["defaultMode", "deny", "allow"], path)?;
            if let Some(mode) = opt_str(perms, "defaultMode", path)? {
                if !DEFAULT_MODES.contains(&mode.as_str()) {
                    return fail(path, &format!("invalid defaultMode \"{mode}\""));
                }
                p.claude.default_mode = Some(mode).filter(|m| !m.is_empty());
            }
            p.claude.deny = resolve_all(opt_str_array(perms, "deny", path)?);
            p.claude.allow = resolve_all(opt_str_array(perms, "allow", path)?);
        }
    }
    if let Some(o) = top.get("opencode") {
        let o = object(o, path)?;
        strict(o, &["includeShared", "permission"], path)?;
        p.opencode.include_shared = opt_bool(o, "includeShared", path)?.unwrap_or(true);
        if let Some(perm) = o.get("permission") {
            let perm = object(perm, path)?;
            let decision = |v: &Value| matches!(v.as_str(), Some("allow") | Some("deny"));
            for (k, v) in perm {
                // a tool takes one decision, or a pattern map (opencode's `bash: { "git stash*": "deny" }`)
                if let Some(patterns) = v.as_object() {
                    if let Some((pattern, _)) = patterns.iter().find(|(_, d)| !decision(d)) {
                        return fail(
                            path,
                            &format!("opencode permission \"{k}\" pattern \"{pattern}\" must be allow or deny"),
                        );
                    }
                } else if !decision(v) {
                    return fail(
                        path,
                        &format!("opencode permission \"{k}\" must be allow or deny"),
                    );
                }
            }
            p.opencode.permission = perm.clone();
        }
    }
    if let Some(x) = top.get("codex") {
        let x = object(x, path)?;
        strict(
            x,
            &[
                "includeShared",
                "approvalPolicy",
                "defaultPermissions",
                "permissions",
                "rules",
            ],
            path,
        )?;
        p.codex.include_shared = opt_bool(x, "includeShared", path)?.unwrap_or(true);
        if let Some(a) = opt_str(x, "approvalPolicy", path)? {
            p.codex.approval_policy = a;
        }
        if let Some(d) = opt_str(x, "defaultPermissions", path)? {
            p.codex.default_permissions = d;
        }
        if let Some(perms) = x.get("permissions") {
            for (name, profile) in object(perms, path)? {
                p.codex
                    .permissions
                    .insert(name.clone(), read_profile(profile, path)?);
            }
        }
        p.codex.rules = resolve_all(opt_str_array(x, "rules", path)?);
    }
    Ok(p)
}

/* ---------- plugins and claude settings ---------- */

fn read_plugins(root: &str) -> Res<Vec<Value>> {
    let path = ".agents/config/plugins.json";
    let Some(raw) = read_json_or_null(root, path)? else {
        return Ok(Vec::new());
    };
    let top = object(&raw, path)?;
    strict(top, &["opencode", "claude", "codex"], path)?;
    let Some(entries) = top.get("opencode") else {
        return Ok(Vec::new());
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| format!("{path}: opencode must be an array"))?;
    entries
        .iter()
        .map(|e| match e {
            Value::String(s) => Ok(Value::String(resolve(s, root))),
            Value::Array(t) => match t.as_slice() {
                [Value::String(p), opts @ Value::Object(_)] => Ok(Value::Array(vec![
                    Value::String(resolve(p, root)),
                    opts.clone(),
                ])),
                _ => fail(path, "plugin tuple must be [string, object]"),
            },
            _ => fail(path, "plugin must be a string or [string, object]"),
        })
        .collect()
}

fn read_claude_settings(root: &str) -> Res<Map<String, Value>> {
    let path = ".agents/config/claude-settings.json";
    let Some(raw) = read_text_or_null(Path::new(&join(&[root, path])))? else {
        return Ok(Map::new());
    };
    if raw.trim().is_empty() {
        return Ok(Map::new());
    }
    let v = parse_json(&raw, path)?;
    let m = object(&v, path)?;
    strict(m, &["outputStyle", "effortLevel", "env"], path)?;
    let mut out = Map::new();
    if let Some(style) = opt_str(m, "outputStyle", path)? {
        if style.is_empty() {
            return fail(path, "outputStyle must not be empty");
        }
        out.insert("outputStyle".into(), Value::String(style));
    }
    if let Some(level) = opt_str(m, "effortLevel", path)? {
        if !["low", "medium", "high", "xhigh", "max"].contains(&level.as_str()) {
            return fail(path, "invalid effortLevel");
        }
        out.insert("effortLevel".into(), Value::String(level));
    }
    if let Some(env) = m.get("env") {
        let vars = object(env, path)?;
        if let Some((name, _)) = vars.iter().find(|(name, value)| name.is_empty() || !value.is_string()) {
            return fail(path, &format!("env.{name} must be a string (an empty string blanks the variable)"));
        }
        out.insert("env".into(), env.clone());
    }
    Ok(out)
}

/* ---------- sources, instructions, commands, rules, skills ---------- */

fn name_ok(name: &str) -> bool {
    Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap().is_match(name)
}

fn frontmatter(body: &str) -> Option<String> {
    Regex::new(r"^---\n([\s\S]*?)\n---\n")
        .unwrap()
        .captures(body)
        .map(|c| c[1].to_string())
}

fn has_description(fm: &str) -> bool {
    Regex::new(r"(?m)^description:\s*\S").unwrap().is_match(fm)
}

fn check_command_body(body: &str) -> Result<(), &'static str> {
    let fm = frontmatter(body).ok_or("must start with a --- frontmatter block")?;
    if has_description(&fm) {
        Ok(())
    } else {
        Err("frontmatter needs a description")
    }
}

fn check_skill_body(name: &str, body: &str) -> Result<(), String> {
    let fm = frontmatter(body).ok_or("must start with a --- frontmatter block")?;
    let re = Regex::new(&format!(r"(?m)^name:\s*{}\s*$", regex::escape(name))).unwrap();
    if !re.is_match(&fm) {
        return Err(format!("frontmatter name must be {name}"));
    }
    if !has_description(&fm) {
        return Err("frontmatter needs a description".into());
    }
    Ok(())
}

struct Sources {
    instructions: String,
    commands: Vec<String>,
    skills: Vec<String>,
    rules: Vec<String>,
    setup: String,
}

fn read_real_file(root: &str, rel: &str) -> Res<String> {
    let abs = join(&[root, rel]);
    match lstat(Path::new(&abs))? {
        None => fail(rel, "missing"),
        Some(m) if m.symlink => fail(rel, "sources must be real files, not symlinks"),
        Some(_) => read_text(Path::new(&abs)),
    }
}

fn read_sources(root: &str) -> Res<Sources> {
    let raw = parse_json(&read_real_file(root, SOURCES_PATH)?, SOURCES_PATH)?;
    let m = object(&raw, SOURCES_PATH)?;
    strict(
        m,
        &["instructions", "commands", "skills", "rules", "setup"],
        SOURCES_PATH,
    )?;
    let non_empty = |key: &str| -> Res<String> {
        match m.get(key) {
            Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
            _ => fail(SOURCES_PATH, &format!("{key} must be a non-empty string")),
        }
    };
    let non_empty_list = |key: &str| -> Res<Vec<String>> {
        let list = string_array(
            m.get(key)
                .ok_or_else(|| format!("{SOURCES_PATH}: {key} is required"))?,
            key,
        )?;
        if list.is_empty() || list.iter().any(String::is_empty) {
            return fail(
                SOURCES_PATH,
                &format!("{key} needs at least one non-empty entry"),
            );
        }
        Ok(list)
    };
    let rules = opt_str_array(m, "rules", SOURCES_PATH)?;
    if rules.iter().any(|r| !r.ends_with(".md")) {
        return fail(SOURCES_PATH, "rules must end with .md");
    }
    Ok(Sources {
        instructions: non_empty("instructions")?,
        commands: non_empty_list("commands")?,
        skills: non_empty_list("skills")?,
        rules,
        setup: non_empty("setup")?,
    })
}

fn load_instructions(root: &str, dir: &str) -> Res<Vec<Instruction>> {
    let mut out: Vec<Instruction> = Vec::new();
    for rel in list_files(Path::new(&join(&[root, dir])))? {
        let source = join(&[dir, &rel]);
        let Some(stem) = rel.strip_suffix(".md") else {
            return fail(&source, "instruction sources are .md files");
        };
        let target = if rel == "root.md" {
            ".".to_string()
        } else {
            stem.to_string()
        };
        if !lstat(Path::new(&join(&[root, &target])))?.is_some_and(|m| m.dir) {
            return fail(
                &source,
                &format!("target directory {target} does not exist"),
            );
        }
        let body = read_real_file(root, &source)?;
        if body.trim().is_empty() {
            return fail(&source, "empty");
        }
        out.push(Instruction {
            source,
            dir: target,
            body,
        });
    }
    if !out.iter().any(|i| i.dir == ".") {
        return fail(dir, "needs root.md");
    }
    Ok(out)
}

fn load_commands(root: &str, entries: &[String]) -> Res<Vec<Doc>> {
    let mut out: Vec<Doc> = Vec::new();
    for entry in entries {
        let files: Vec<String> = if entry.ends_with(".md") {
            vec![entry.clone()]
        } else {
            list_dir(Path::new(&join(&[root, entry])))?
                .into_iter()
                .filter(|f| f.ends_with(".md"))
                .map(|f| join(&[entry, &f]))
                .collect()
        };
        for source in files {
            let name = basename(&source, ".md");
            if !name_ok(&name) {
                return fail(&source, "command name must match /^[a-z0-9][a-z0-9-]*$/");
            }
            if out.iter().any(|c| c.name == name) {
                return fail(&source, &format!("duplicate command {name}"));
            }
            let body = read_real_file(root, &source)?;
            if let Err(msg) = check_command_body(&body) {
                return fail(&source, msg);
            }
            out.push(Doc { name, source, body });
        }
    }
    Ok(out)
}

fn load_skill(root: &str, source: &str) -> Res<Skill> {
    let name = basename(source, "");
    if !name_ok(&name) {
        return fail(source, "skill name must match /^[a-z0-9][a-z0-9-]*$/");
    }
    let mut files = Vec::new();
    for rel in list_files(Path::new(&join(&[root, source])))? {
        let content = read_real_file(root, &join(&[source, &rel]))?;
        files.push((rel, content));
    }
    let Some((_, skill_md)) = files.iter().find(|(rel, _)| rel == "SKILL.md") else {
        return fail(source, "missing SKILL.md");
    };
    if let Err(msg) = check_skill_body(&name, skill_md) {
        return fail(&join(&[source, "SKILL.md"]), &msg);
    }
    Ok(Skill {
        name,
        source: source.to_string(),
        files,
    })
}

fn load_skills(root: &str, entries: &[String]) -> Res<Vec<Skill>> {
    let shared_prefix = format!("{SHARED_SKILLS_DIR}/");
    let mut dirs: Vec<String> = Vec::new();
    for entry in entries {
        if lstat(Path::new(&join(&[root, entry, "SKILL.md"])))?.is_some() {
            dirs.push(entry.clone());
        } else {
            for d in list_dir(Path::new(&join(&[root, entry])))? {
                dirs.push(join(&[entry, &d]));
            }
        }
    }
    let external: Vec<&String> = dirs
        .iter()
        .filter(|d| !d.starts_with(&shared_prefix))
        .collect();
    let external_names: Vec<String> = external.iter().map(|d| basename(d, "")).collect();
    let own: Vec<&String> = dirs
        .iter()
        .filter(|d| d.starts_with(&shared_prefix) && !external_names.contains(&basename(d, "")))
        .collect();
    let mut out: Vec<Skill> = Vec::new();
    for dir in own.into_iter().chain(external) {
        let skill = load_skill(root, dir)?;
        if out.iter().any(|s| s.name == skill.name) {
            return fail(dir, &format!("duplicate skill {}", skill.name));
        }
        out.push(skill);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub const FIRST_PARTY_TRUSTED_SERVERS: &[&str] = &["Claude_Code_Remote", "github", "ccd_session", "Claude_Docs"];

/// Servers with full trust: declared for Claude in `mcps.json`, plus the first-party list.
pub fn trusted_mcp_servers(mcps: &Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, per) in mcps {
        let declared = per
            .as_object()
            .is_some_and(|p| p.get("claude").is_some_and(|c| !c.is_null()));
        if declared && !out.contains(name) {
            out.push(name.clone());
        }
    }
    for name in FIRST_PARTY_TRUSTED_SERVERS {
        if !out.iter().any(|n| n == name) {
            out.push((*name).to_string());
        }
    }
    out
}

pub fn load_contract(root: &str) -> Res<Contract> {
    let sources = read_sources(root)?;
    let mcps = read_mcps(root)?;
    if lstat(Path::new(&join(&[root, &sources.setup])))?.is_none() {
        return fail(&sources.setup, "setup script missing");
    }
    let (hooks_claude, hooks_opencode, hooks_codex) = read_hooks(root)?;
    let mut permissions = read_permissions(root)?;
    let mut seen: Vec<String> = Vec::new();
    permissions.claude.allow.retain(|a| {
        let fresh = !seen.contains(a);
        if fresh {
            seen.push(a.clone());
        }
        fresh
    });
    for server in trusted_mcp_servers(&mcps) {
        let entry = format!("mcp__{server}");
        if !permissions.claude.allow.contains(&entry) {
            permissions.claude.allow.push(entry);
        }
    }
    let mut rules = Vec::new();
    for source in &sources.rules {
        rules.push(Doc {
            name: basename(source, ".md"),
            source: source.clone(),
            body: read_real_file(root, source)?,
        });
    }
    Ok(Contract {
        mcps,
        hooks_claude,
        hooks_opencode,
        hooks_codex,
        permissions,
        plugins_opencode: read_plugins(root)?,
        claude_settings: read_claude_settings(root)?,
        setup: sources.setup.clone(),
        instructions: load_instructions(root, &sources.instructions)?,
        commands: load_commands(root, &sources.commands)?,
        rules,
        skills: load_skills(root, &sources.skills)?,
    })
}
