//! What a secret read may do inside a CI run (docs/plans/ci-scoped-vault-access/plan.md, wave 2): which token variable serves a
//! scope, the values that are repository variables and never vault keys, the write URLs only a `push` to `dev` may resolve, and the
//! `::add-mask::` announcement of every value fetched in GitHub Actions. All of it reads the process environment by name.
use super::registry::{Entry, Scope};
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const DEPLOY_TOKEN_VAR: &str = "BWS_CI_DEPLOY_ACCESS_TOKEN";

/// The cache and artifact links that can overwrite shared objects (docs/solutions/ci/job-secret-needs.md).
const WRITE_URL_KEYS: [&str; 2] = ["BUILD_CACHE_WRITE_PAR_URL", "VERIFICATION_ARTIFACTS_PAR_URL"];

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn is_true(name: &str) -> bool {
    var(name).is_some_and(|v| v == "true")
}

pub fn in_actions() -> bool {
    is_true("GITHUB_ACTIONS")
}

pub fn in_ci() -> bool {
    in_actions() || is_true("CI")
}

pub fn token_var(scope: Scope) -> Option<&'static str> {
    match scope {
        Scope::CiOpen => Some(DEPLOY_TOKEN_VAR),
        Scope::CiDeploy => Some(DEPLOY_TOKEN_VAR),
        Scope::Vm | Scope::Public => None,
    }
}

pub fn scope_token(scope: Scope) -> Option<String> {
    token_var(scope).and_then(var)
}

/// A CI run that was given at least one scope token reads by scope; with none it reads as it always did (the VM runner's
/// container-level token) until that token is removed.
pub fn scoped_mode() -> bool {
    in_ci() && (scope_token(Scope::CiOpen).is_some() || scope_token(Scope::CiDeploy).is_some())
}

/// `Some(reason)` when this event and ref may not resolve the write URL `key`; only checked in GitHub Actions. A push to `dev` and a dispatch on `dev` may (the
/// dispatcher can push to `dev` too: the forced cold run is one); a pull request or another branch may not.
pub fn write_refusal(key: &str) -> Option<String> {
    if !in_actions() || !WRITE_URL_KEYS.contains(&key) {
        return None;
    }
    let (event, git_ref) = (var("GITHUB_EVENT_NAME").unwrap_or_default(), var("GITHUB_REF").unwrap_or_default());
    (!matches!(event.as_str(), "push" | "workflow_dispatch") || git_ref != "refs/heads/dev").then(|| format!("write URL refused for {event}/{git_ref}"))
}

/// The repository variable of a `public` key: the environment variable named like the registry key.
pub fn public_value(entry: &Entry) -> Option<String> {
    var(&entry.key)
}

fn public_missing(entry: &Entry) -> String {
    format!("{} is a public value and is not set: define the repository variable {} (Settings, Secrets and variables, Actions, Variables) and pass it to the job as env {}", entry.key, entry.key, entry.key)
}

/// Why this CI run may not read `entry` at all, whatever the vault holds; `None` outside CI and for a readable key.
pub fn refusal(entry: &Entry) -> Option<String> {
    if let Some(why) = write_refusal(&entry.key) {
        return Some(format!("{}: {why}", entry.key));
    }
    if !scoped_mode() {
        return None;
    }
    match entry.scope {
        Scope::Vm => Some(format!("{}: scope vm, not readable in CI", entry.key)),
        Scope::Public if public_value(entry).is_none() => Some(public_missing(entry)),
        Scope::CiOpen | Scope::CiDeploy => {
            let var = token_var(entry.scope).expect("a CI scope names a token variable");
            scope_token(entry.scope).is_none().then(|| format!("{}: scope {} needs {var}, which is not set", entry.key, entry.scope.name()))
        }
        Scope::Public => None,
    }
}

static MASK_TO_STDOUT: AtomicBool = AtomicBool::new(false);
static MASKED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// `sync-env` writes a file and prints no value, so it announces masks on stdout; every other verb (`get` and `env` print values,
/// `verify` prints JSON) and every library caller whose stdout another program parses announces on stderr, which the runner reads for
/// workflow commands as well.
pub fn mask_on_stdout(on: bool) {
    MASK_TO_STDOUT.store(on, Ordering::Relaxed);
}

/// `::add-mask::` for each line of `value`, once per distinct line, flushed before the caller can use the value.
pub fn announce(value: &str) {
    if !in_actions() {
        return;
    }
    let mut seen = MASKED.lock().unwrap_or_else(|e| e.into_inner());
    let seen = seen.get_or_insert_with(HashSet::new);
    let mut text = String::new();
    for line in value.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()) {
        if seen.insert(line.to_string()) {
            text.push_str(&format!("::add-mask::{line}\n"));
        }
    }
    if text.is_empty() {
        return;
    }
    if MASK_TO_STDOUT.load(Ordering::Relaxed) {
        let mut o = std::io::stdout().lock();
        let _ = o.write_all(text.as_bytes());
        let _ = o.flush();
    } else {
        let mut e = std::io::stderr().lock();
        let _ = e.write_all(text.as_bytes());
        let _ = e.flush();
    }
}
