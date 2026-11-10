//! `wf secret`: list, get, env, sync-env, doctor and the writers. Anything this does not recognise returns `None`, which
//! the dispatcher reports as unsupported.
mod audit;
mod backend;
mod cache;
mod ci;
mod dotenv;
mod groups;
mod job_scopes;
mod parity;
mod registry;
mod scope;
mod sync_env;
mod util;
mod vault;
mod verify;
mod writers;

pub use writers::update_secret_value;

use wf_kit::args::{parse, parse_none, Parsed};
use backend::Backend;
use registry::Entry;
use std::collections::HashMap;
use std::io::Write;
use sync_env::Env;
use util::{json_string, mask, pad_end};

fn out(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

fn err(s: &str) {
    let mut e = std::io::stderr().lock();
    let _ = e.write_all(s.as_bytes());
    let _ = e.flush();
}

fn truthy(v: &Option<String>) -> Option<&String> {
    v.as_ref().filter(|s| !s.is_empty())
}

struct Ctx {
    registry: Vec<Entry>,
    backend: Box<dyn Backend>,
    env: Env,
}

impl Ctx {
    fn new(backend_flag: Option<&str>) -> Result<Ctx, String> {
        let backend_name = backend::backend_name(backend_flag);
        let backend = backend::build(&backend_name)?;
        // With a vault configured the vault is the only source: the shell and the env files `wf` layers cannot shadow it.
        let env = if backend.authoritative() { Env::vault_only() } else { Env::new() };
        Ok(Ctx {
            registry: registry::load()?,
            backend,
            env,
        })
    }

    /// Fills the in-memory env from the backend. Without a configured vault an unusable backend warns once and the existing
    /// environment stays the source (a deployed `.env`); with one, a failure is an error, never a quiet fallback.
    fn load(&mut self) -> Result<(), String> {
        self.walk(|_| true, false)
    }

    fn load_where(&mut self, wanted: impl Fn(&Entry) -> bool) -> Result<(), String> {
        self.walk(wanted, true)
    }

    /// A key CI may not read is an error when it was asked for by name, and skipped in a walk over the whole registry.
    fn walk(&mut self, wanted: impl Fn(&Entry) -> bool, named: bool) -> Result<(), String> {
        let mut warned = false;
        for entry in self.registry.iter().filter(|e| wanted(e)) {
            if let Some(why) = ci::refusal(entry) {
                if named {
                    return Err(why);
                }
                continue;
            }
            if truthy(&self.env.get(&entry.env_var)).is_some() {
                continue;
            }
            match self.backend.resolve(entry) {
                Err(e) if self.backend.authoritative() => {
                    return Err(format!("backend \"{}\" failed for {}: {e}", self.backend.name(), entry.key));
                }
                Err(e) => {
                    if !warned {
                        warned = true;
                        err(&format!(
                            "[secrets] backend \"{}\" unavailable, continuing with the existing environment — {e}\n",
                            self.backend.name()
                        ));
                    }
                }
                Ok(Some(v)) if !v.is_empty() => self.env.set(&entry.env_var, v),
                Ok(_) => {}
            }
        }
        Ok(())
    }

    fn get(&mut self, entry: &Entry) -> Result<Option<String>, String> {
        if let Some(why) = ci::refusal(entry) {
            return Err(why);
        }
        if let Some(v) = truthy(&self.env.get(&entry.env_var)) {
            return Ok(Some(v.clone()));
        }
        self.backend.resolve(entry)
    }

    fn valid_key(&self, key: &str) -> Option<usize> {
        self.registry.iter().position(|e| e.key == key)
    }

    fn unknown_key(&self, key: &str) -> i32 {
        let valid: Vec<&str> = self.registry.iter().map(|e| e.key.as_str()).collect();
        err(&format!(
            "Unknown key: {key}\nValid keys: {}\n",
            valid.join(", ")
        ));
        1
    }

}

struct Status {
    key: String,
    env_var: String,
    source: &'static str,
    masked: String,
}

fn list_statuses(ctx: &mut Ctx) -> Result<Vec<Status>, String> {
    let entries = ctx.registry.clone();
    let mut out = Vec::new();
    for e in &entries {
        let (source, masked) = if let Some(why) = ci::refusal(e) {
            ("refused", why)
        } else if let Some(v) = truthy(&ctx.env.get(&e.env_var)) {
            ("env", mask(v))
        } else if let Some(v) = ctx.backend.resolve(e)?.filter(|v| !v.is_empty()) {
            ("backend", mask(&v))
        } else {
            ("missing", "(missing)".to_string())
        };
        out.push(Status {
            key: e.key.clone(),
            env_var: e.env_var.clone(),
            source,
            masked,
        });
    }
    Ok(out)
}

fn cmd_list(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    ctx.load()?;
    let statuses = list_statuses(&mut ctx)?;
    let col = statuses
        .iter()
        .map(|s| s.key.chars().count())
        .max()
        .unwrap_or(0)
        .max(3);
    out(&format!("{}  SOURCE    VALUE\n", pad_end("KEY", col)));
    out(&format!("{}  --------  -----\n", "-".repeat(col)));
    for s in &statuses {
        out(&format!(
            "{}  {}  {}\n",
            pad_end(&s.key, col),
            pad_end(s.source, 8),
            s.masked
        ));
    }
    Ok(0)
}

fn cmd_get(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    let key = &p.positional[0];
    let Some(idx) = ctx.valid_key(key) else {
        return Ok(ctx.unknown_key(key));
    };
    // Only the key asked for: loading the whole registry listed the write-capable vault on every get.
    ctx.load_where(|e| e.key == *key)?;
    let entry = ctx.registry[idx].clone();
    let mut value = ctx.get(&entry)?;
    if value.is_none() && cache::mode() == cache::Mode::Use {
        // A fresh sealed copy predates a secret created since: look once at the live vault before calling the key missing.
        cache::refresh_this_run();
        let mut live = Ctx::new(p.opts.get("backend").map(String::as_str))?;
        live.load_where(|e| e.key == *key)?;
        let entry = live.registry[idx].clone();
        value = live.get(&entry)?;
    }
    match value {
        None => {
            err(&format!("{key}: not found\n"));
            Ok(1)
        }
        Some(v) => {
            out(&format!("{v}\n"));
            Ok(0)
        }
    }
}

fn cmd_env(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    // `--keys a,b` narrows the walk to those registry keys (or env var names), so a caller that needs a handful of
    // secrets does not resolve the whole registry.
    if let Some(keys) = p.opts.get("keys") {
        let wanted: Vec<&str> = keys.split(',').map(str::trim).filter(|k| !k.is_empty()).collect();
        ctx.registry.retain(|e| wanted.contains(&e.key.as_str()) || wanted.contains(&e.env_var.as_str()));
    }
    ctx.load()?;
    let statuses = list_statuses(&mut ctx)?;
    for s in statuses.iter().filter(|s| !matches!(s.source, "missing" | "refused")) {
        let entry = ctx
            .registry
            .iter()
            .find(|e| e.key == s.key)
            .cloned()
            .expect("listed from registry");
        let value = ctx.get(&entry)?.unwrap_or_default();
        out(&format!("{}={}\n", s.env_var, json_string(&value)));
    }
    Ok(0)
}

fn read_base(path: &str) -> Result<Vec<(String, String)>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    Ok(dotenv::parse(&String::from_utf8_lossy(&bytes)))
}

fn cmd_sync_env(p: Parsed) -> Result<i32, String> {
    // The command that writes a deploy's env from the vault is the refresh point: a rotated secret lands here, not an hour later.
    cache::refresh_this_run();
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    let (required, optional): (Vec<String>, Vec<String>) = if let Some(app) = p.opts.get("for") {
        let Some(group) = groups::GROUPS.iter().find(|g| &g.app == app) else {
            let known: Vec<&str> = groups::GROUPS.iter().map(|g| g.app.as_str()).collect();
            err(&format!(
                "Unknown --for group: {app}\nKnown groups: {}\n",
                known.join(", ")
            ));
            return Ok(1);
        };
        (
            group.required.clone(),
            group.optional.clone(),
        )
    } else if !p.positional.is_empty() {
        for k in &p.positional {
            if ctx.valid_key(k).is_none() {
                return Ok(ctx.unknown_key(k));
            }
        }
        (p.positional.clone(), Vec::new())
    } else {
        err("Pass one or more KEY args or --for <app>\n");
        return Ok(1);
    };

    let all: Vec<String> = required.iter().chain(optional.iter()).cloned().collect();
    // A broken backend throws for ANY key; only a genuine "not in the vault" resolves to missing.
    let result = sync_env::sync_env(&all, &ctx.registry, ctx.backend.as_mut(), &ctx.env)?;
    let missing_required: Vec<&String> = result
        .missing
        .iter()
        .filter(|k| required.contains(k))
        .collect();
    if !result.placeholders.is_empty() {
        err(&format!(
            "NOTE: {} secret(s) exist in the vault but still hold a PLACEHOLDER, so they were treated as MISSING and not written: {}. Fill them with `wf secret set <KEY>`; a placeholder is exactly as unconfigured as an absent value, and shipping one breaks the container instead of the deploy.\n",
            result.placeholders.len(),
            result.placeholders.join(", ")
        ));
    }
    if !result.from_ambient_env.is_empty() {
        err(&format!(
            "WARNING: {} secret(s) took their value from the AMBIENT ENVIRONMENT, not the vault: {}. That is intended for a deliberate override (e.g. a value exported from .env.local), and wrong for anything else — a rotated secret in the vault will NOT reach this .env while an ambient value shadows it. On a deploy host nothing should be setting these: if you did not export them on purpose, unset them and re-run.\n",
            result.from_ambient_env.len(),
            result.from_ambient_env.join(", ")
        ));
    }
    if !result.skipped.is_empty() {
        err(&format!(
            "NOTE: omitted {} non-deploy-scoped secret(s) from the .env: {}. These are registered to another vault project (see `vault` in registry.json) and must not ship to a host.\n",
            result.skipped.len(),
            result.skipped.join(", ")
        ));
    }
    if !missing_required.is_empty() {
        let names: Vec<&str> = missing_required.iter().map(|s| s.as_str()).collect();
        err(&format!(
            "ERROR: could not resolve required secret(s): {}\n",
            names.join(", ")
        ));
        err("Refusing to write a partial .env. Fix the backend or its configuration and retry.\n");
        return Ok(1);
    }

    let base_path: Option<String> = p.opts.get("base").cloned().or_else(|| {
        std::path::Path::new(".env.defaults")
            .exists()
            .then(|| ".env.defaults".to_string())
    });
    let env_content = match &base_path {
        Some(path) => {
            if !std::path::Path::new(path).exists() {
                err(&format!("--base file not found: {path}\n"));
                return Ok(1);
            }
            let (content, collisions) =
                sync_env::compose_dotenv(read_base(path)?, &result.resolved);
            if !collisions.is_empty() {
                err(&format!(
                    "WARNING: key(s) in {path} overridden by resolved secrets (a secret name in a committed base is a smell): {}\n",
                    collisions.join(", ")
                ));
            }
            content
        }
        None => sync_env::format_dotenv(&result.resolved),
    };
    if let Some(file) = p.opts.get("out") {
        std::fs::write(file, &env_content).map_err(|e| format!("cannot write {file}: {e}"))?;
        let base_note = base_path
            .as_ref()
            .map_or(String::new(), |b| format!(" + base {b}"));
        out(&format!(
            "Wrote {} secret(s){base_note} to {file}\n",
            result.resolved.len()
        ));
    } else if p.flags.iter().any(|f| f == "reveal") {
        out(&env_content);
    } else {
        // No --out: print a redacted preview, so real values never land in a terminal, log or transcript by default.
        let redacted: Vec<(String, String)> = result
            .resolved
            .iter()
            .map(|(k, v)| (k.clone(), mask(v)))
            .collect();
        let content = match &base_path {
            Some(path) => sync_env::compose_dotenv(read_base(path)?, &redacted).0,
            None => sync_env::format_dotenv(&redacted),
        };
        err("No --out given -- printing a REDACTED preview (values hidden), since nothing in this repo's deploy path relies on sync-env's raw stdout and printing real secrets by default is unsafe for scripts and agents. Use --out <file> to write the real .env, or --reveal to print real values (only at a real terminal you trust).\n");
        out(&content);
    }
    Ok(0)
}

/// What `verify` found for one app: a finding per key (the app's group and the `extra` keys), the failed ones listed first.
struct Verified {
    app: String,
    findings: Vec<verify::Finding>,
}

impl Verified {
    fn ok(&self) -> bool {
        !self.findings.iter().any(verify::Finding::failed)
    }

    fn to_json(&self) -> serde_json::Value {
        let mut findings: Vec<&verify::Finding> = self.findings.iter().collect();
        findings.sort_by_key(|f| !f.failed());
        serde_json::json!({ "app": self.app, "ok": self.ok(), "findings": findings.iter().map(|f| f.to_json()).collect::<Vec<_>>() })
    }
}

fn verify_app(ctx: &mut Ctx, app: &str, extra: &[&str]) -> Result<Verified, String> {
    let Some(group) = groups::GROUPS.iter().find(|g| g.app == app) else {
        let known: Vec<&str> = groups::GROUPS.iter().map(|g| g.app.as_str()).collect();
        return Err(format!("Unknown --for group: {app}\nKnown groups: {}", known.join(", ")));
    };
    // A write URL this event may not resolve is not a defect of the deploy: the run reads only.
    let mut keys: Vec<String> = group.required.iter().chain(group.optional.iter()).filter(|k| ci::write_refusal(k).is_none() && !group.vm_only.contains(k)).cloned().collect();
    for k in extra {
        if !keys.iter().any(|existing| existing == k) {
            keys.push(k.to_string());
        }
    }
    let result = sync_env::sync_env(&keys, &ctx.registry, ctx.backend.as_mut(), &ctx.env)?;
    let required: Vec<&String> = group.required.iter().filter(|k| ci::write_refusal(k).is_none() && !group.vm_only.contains(k)).collect();
    let mut findings = verify::check(&ctx.registry, &result.resolved);
    for key in &result.missing {
        let wanted = required.contains(&key) || extra.contains(&key.as_str());
        if wanted {
            let why = if result.placeholders.contains(key) { "still holds a placeholder" } else { "is not in the vault" };
            findings.push(verify::Finding { key: key.clone(), checks: vec!["present"], status: verify::Status::Failed(why.into()) });
        }
    }
    Ok(Verified { app: app.to_string(), findings })
}

/// `wf secret verify --for <app>`: every secret the app's deploy carries, checked for being usable, not only present.
fn cmd_verify(p: Parsed) -> Result<i32, String> {
    if let Some(scope) = p.opts.get("scope") {
        return cmd_verify_scope(&p, scope);
    }
    let Some(app) = p.opts.get("for") else {
        err("Pass --for <app> or --scope <ci-open|ci-deploy>\n");
        return Ok(1);
    };
    cache::refresh_this_run();
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    let verified = verify_app(&mut ctx, app, &[])?;
    out(&format!("{}\n", serde_json::to_string_pretty(&verified.to_json()).map_err(|e| e.to_string())?));
    Ok(if verified.ok() { 0 } else { 1 })
}

/// `wf secret verify --scope <scope>`: every registry key of a CI scope is in the vault project the token reads, and that project
/// holds no key of another scope. Names only, never a value.
fn cmd_verify_scope(p: &Parsed, scope: &str) -> Result<i32, String> {
    let Some(scope) = scope::ci_scope(scope) else {
        err(&format!("Unknown --scope: {scope}\nKnown scopes: ci-open, ci-deploy\n"));
        return Ok(1);
    };
    cache::refresh_this_run();
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    let visible = ctx.backend.names()?;
    let registry = ctx.registry.clone();
    let report = scope::report(&registry, scope, visible.as_deref(), &mut |entry| Ok(truthy(&ctx.backend.resolve(entry)?).is_some()))?;
    out(&format!("{}\n", serde_json::to_string_pretty(&report.to_json()).map_err(|e| e.to_string())?));
    Ok(if report.ok() { 0 } else { 1 })
}

/// `wf secret parity --json`: per CI scope, the registry keys the token's project holds and a nonce-keyed hash of each value. Run it
/// on two runner kinds with the same `--nonce` and diff the output; no value is printed.
fn cmd_parity(p: Parsed) -> Result<i32, String> {
    let nonce = match p.opts.get("nonce") {
        Some(n) if n.len() >= 16 => n.clone(),
        Some(_) => {
            err("--nonce must be at least 16 characters\n");
            return Ok(1);
        }
        None => parity::fresh_nonce()?,
    };
    cache::refresh_this_run();
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    let registry = ctx.registry.clone();
    let (report, ok) = parity::report(&registry, ctx.backend.as_mut(), &nonce)?;
    out(&format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?));
    Ok(if ok { 0 } else { 1 })
}

/// The CI-scope violations of the workflows in `workflows` (`wf secret scope-check`), for `wf check`.
pub fn scope_violations(workflows: &std::path::Path) -> Result<Vec<String>, String> {
    job_scopes::violations(workflows)
}

/// The preflight of a deploy: `Err` names every secret of `app` (and of `extra`, a registry key the deploy also uses) that is missing or
/// unusable, so the deploy stops before it builds anything.
pub fn verify_for(app: &str, extra: &[&str]) -> Result<(), String> {
    let mut ctx = Ctx::new(None)?;
    let verified = verify_app(&mut ctx, app, extra)?;
    let failed: Vec<String> = verified.findings.iter().filter_map(|f| if let verify::Status::Failed(reason) = &f.status { Some(format!("  {}: {reason}", f.key)) } else { None }).collect();
    if failed.is_empty() {
        return Ok(());
    }
    Err(format!("preflight: {} secret(s) of {app} are not usable, nothing was built:\n{}", failed.len(), failed.join("\n")))
}

fn cmd_doctor(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(p.opts.get("backend").map(String::as_str))?;
    let if_available = p.flags.iter().any(|f| f == "if-available");
    if if_available && !audit::has_any_scope_credential()? {
        // Loud rather than silent: a skipped check that reads as a pass stops preventing anything.
        out("secrets doctor SKIPPED: no Bitwarden scope credential in this environment, so registry/vault drift was not checked.\n");
        return Ok(0);
    }
    let registry = ctx.registry.clone();
    let report = match audit::audit_drift(&registry, ctx.backend.as_mut()) {
        Ok(r) => r,
        Err(message) => {
            // A credential that exists but cannot reach the vault is the same situation as having none.
            if if_available && audit::is_list_failure(&message) {
                out(&format!(
                    "secrets doctor SKIPPED: the Bitwarden scope credential in this environment cannot reach the vault, so registry/vault drift was not checked. The backend said: {}\n",
                    message.lines().next().unwrap_or("")
                ));
                return Ok(0);
            }
            err(&format!("{message}\n"));
            return Ok(1);
        }
    };
    out(&format!("{}\n", audit::format_report(&report)));
    // Only actionable drift fails: a wiring bug, or a secret nothing can reach.
    Ok(
        if report.mismatch_suspects.is_empty() && report.unregistered.is_empty() {
            0
        } else {
            1
        },
    )
}

/// The registry is compiled into wf, so after editing `registry.json` this rebuilds the binary, then runs the drift check the
/// pre-commit hook runs, with the rebuilt binary.
fn registry_sync() -> i32 {
    let root = wf_kit::root::repo_root();
    match std::process::Command::new("cargo").args(["build", "--release", "-p", "wf", "--manifest-path"]).arg(root.join("clis/Cargo.toml")).status() {
        Ok(status) if status.success() => {}
        Ok(status) => return status.code().unwrap_or(1),
        Err(e) => {
            err(&format!("cannot run cargo: {e}\n"));
            return 1;
        }
    }
    std::process::Command::new(root.join("clis/target/release/wf"))
        .args(["secret", "doctor", "--backend", "bitwarden", "--if-available"])
        .status()
        .map_or(1, |s| s.code().unwrap_or(1))
}

/// `args` are the arguments after `secrets`. `None` for a shape it does not serve.
pub fn run(args: &[String]) -> Option<i32> {
    if args == ["registry", "sync"] {
        return Some(registry_sync());
    }
    let (sub, rest) = args.split_first()?;
    let parsed = match sub.as_str() {
        "list" => parse_none(rest, &["backend"], &[]),
        "env" => parse_none(rest, &["backend", "keys"], &[]),
        "get" => parse(rest, &["backend"], &[]).filter(|p| p.positional.len() == 1),
        "sync-env" => parse(rest, &["for", "backend", "out", "base"], &["reveal"]),
        "doctor" => {
            parse_none(rest, &["backend"], &["if-available"])
        }
        "verify" => parse_none(rest, &["for", "scope", "backend"], &[]),
        "scope-check" => parse_none(rest, &["workflows", "jobs"], &["json"]),
        "parity" => parse_none(rest, &["nonce", "backend"], &["json"]),
        "cache" => parse(rest, &[], &[]).filter(|p| p.positional == ["clear"]),
        "set" => parse(rest, &[], &[]).filter(|p| (1..=2).contains(&p.positional.len())),
        "delete" => parse(rest, &[], &[]).filter(|p| p.positional.len() == 1),
        "push-notes" => parse_none(rest, &[], &["apply", "verbose"]),
        "create" => parse(rest, &["value", "value-from"], &["value-stdin", "placeholder", "apply"]).filter(|p| p.positional.len() == 2),
        "update" => parse(rest, &["value"], &["value-stdin", "apply"]).filter(|p| p.positional.len() == 1),
        "rename" => parse(rest, &[], &["additive", "apply"]).filter(|p| p.positional.len() == 2),
        "move" => parse(rest, &[], &["apply"]).filter(|p| p.positional.len() == 2),
        _ => None,
    }?;
    ci::mask_on_stdout(sub == "sync-env" && !parsed.flags.iter().any(|f| f == "reveal"));
    let result = match sub.as_str() {
        "list" => cmd_list(parsed),
        "get" => cmd_get(parsed),
        "env" => cmd_env(parsed),
        "sync-env" => cmd_sync_env(parsed),
        "verify" => cmd_verify(parsed),
        "scope-check" => job_scopes::cmd(&parsed),
        "parity" => cmd_parity(parsed),
        "cache" => {
            out(&format!("Removed {} cached vault copy(ies)\n", cache::clear()));
            Ok(0)
        }
        "set" => writers::cmd_set(parsed),
        "delete" => writers::cmd_delete(parsed),
        "push-notes" => writers::cmd_push_notes(parsed),
        "create" => writers::cmd_create(parsed),
        "update" => writers::cmd_update(parsed),
        "rename" => writers::cmd_rename(parsed),
        "move" => writers::cmd_move(parsed),
        _ => cmd_doctor(parsed),
    };
    // A vault write makes any copy of the vault stale.
    if matches!(sub.as_str(), "create" | "update" | "rename" | "move" | "delete") && matches!(result, Ok(0)) {
        cache::clear();
    }
    Some(result.unwrap_or_else(|message| {
        err(&format!("Error: {message}\n"));
        1
    }))
}

/// `load([...keys])` of the TypeScript library, for another command that needs a few secrets: fills what
/// the environment lacks from the default backend and returns the values by registry key. With a vault configured the
/// vault alone answers (the shell is never consulted) and a failure is printed, leaving the values missing; the caller reports
/// a missing value itself.
pub fn load_values(keys: &[&str]) -> HashMap<String, Option<String>> {
    let mut out = HashMap::new();
    let Ok(mut ctx) = Ctx::new(None) else {
        for k in keys {
            out.insert(
                (*k).to_string(),
                std::env::var(k).ok().filter(|v| !v.is_empty()),
            );
        }
        return out;
    };
    if let Err(e) = ctx.load_where(|e| keys.contains(&e.key.as_str())) {
        err(&format!("wf: secrets: {e}\n"));
    }
    let ambient = !ctx.backend.authoritative();
    for k in keys {
        let value = ctx.registry.iter().find(|e| e.key == *k).and_then(|e| ctx.env.get(&e.env_var));
        let value = if ambient { value.or_else(|| std::env::var(k).ok()) } else { value };
        out.insert((*k).to_string(), value.filter(|v| !v.is_empty()));
    }
    out
}

/// One registry secret through the default backend, for commands outside `wf secret` that need a value.
pub fn secret(key: &str) -> Result<Option<String>, String> {
    let mut ctx = Ctx::new(None)?;
    let Some(idx) = ctx.valid_key(key) else {
        return Err(format!("Unknown key: {key}"));
    };
    let entry = ctx.registry[idx].clone();
    ctx.get(&entry)
}

/// One registry secret through a named backend.
pub fn secret_from(key: &str, backend: &str) -> Result<Option<String>, String> {
    let mut ctx = Ctx::new(Some(backend))?;
    let Some(idx) = ctx.valid_key(key) else {
        return Err(format!("Unknown key: {key}"));
    };
    let entry = ctx.registry[idx].clone();
    ctx.get(&entry)
}

/// Whether a vault is configured, so that a registered secret is read from it alone and never from the shell or an env file.
pub fn vault_is_authoritative() -> bool {
    Ctx::new(None).is_ok_and(|ctx| ctx.backend.authoritative())
}

/// Why this CI event may not resolve the cache and artifact write URLs (`write URL refused for <event>/<ref>`); `None` when it may.
pub fn write_urls_refused() -> Option<String> {
    ci::write_refusal("BUILD_CACHE_WRITE_PAR_URL")
}

/// The process-environment value of a registered secret, for a caller that honours a deliberate override (a test harness, a
/// deployed `.env`): none while a vault is configured, because then the vault is the only source.
pub fn ambient(name: &str) -> Option<String> {
    if vault_is_authoritative() || ci::write_refusal(name).is_some() {
        None
    } else {
        std::env::var(name).ok()
    }
}

/// Puts every registry secret into the process environment, as the TypeScript's `load()` does before a delegated run, so the
/// child inherits the provider keys: with a vault configured the vault's value replaces whatever the shell held, without one
/// only a missing value is filled. An unusable backend warns once (no vault) or is reported and ends the load (vault).
pub fn load_all_into_process_env() {
    let Ok(mut ctx) = Ctx::new(None) else { return };
    if let Err(e) = ctx.load() {
        err(&format!("wf: secrets: {e}\n"));
        return;
    }
    let overwrite = ctx.backend.authoritative();
    for e in ctx.registry.clone() {
        let missing = std::env::var(&e.env_var).map_or(true, |v| v.is_empty());
        if let Some(v) = truthy(&ctx.env.get(&e.env_var)).filter(|_| missing || overwrite) {
            std::env::set_var(&e.env_var, v);
        }
    }
}
