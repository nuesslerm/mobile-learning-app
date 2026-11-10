//! `wf secret login list|capture|done|launch-url|<tool>|listener`: the VNC auth-session listener's control channel
//! (and the listener itself, `crate::vnc`) and the credential files it relays.
use wf_kit::args::{parse, parse_none};
use secrets::load_values;
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::vnc::descriptor::{self, builtin, Descriptor, KNOWN_TOOLS as TOOLS};
const LINK_TTL_HOURS: u32 = 12;
const DEFAULT_URL: &str = "http://vnc-auth:8795";

fn example_spec() -> Value {
    json!({
        "id": "my-tool",
        "loginUrl": "https://example.com/accounts/login/",
        "authErrorPatterns": ["401", "403", "session expired"],
        "capture": {
            "kind": "cookies",
            "urls": ["https://example.com"],
            "require": ["SESSION", "csrftoken"],
            "format": "cookie-header"
        },
        "deliver": {
            "kind": "http",
            "url": "${MY_BACKEND_BASE_URL}/api/session",
            "method": "POST",
            "headers": { "x-admin-secret": "${MY_BACKEND_ADMIN_SECRET}" },
            "jsonBody": { "cookie": "${credential}" }
        },
        "verify": { "url": "${MY_BACKEND_BASE_URL}/api/user-status", "trueField": "isSignedIn" }
    })
}

struct Listener {
    base: String,
    secret: String,
    owner: String,
}

/// The listener's address and credentials, resolved from the vault like every other `wf` command.
fn connect() -> Result<Listener, i32> {
    let v = load_values(&["VNC_AUTH_TRIGGER_SECRET", "VNC_AUTH_OWNER_CHAT_ID"]);
    let base = std::env::var("VNC_AUTH_LISTENER_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
    match (&v["VNC_AUTH_TRIGGER_SECRET"], &v["VNC_AUTH_OWNER_CHAT_ID"]) {
        (Some(secret), Some(owner)) => Ok(Listener {
            base: base.trim_end_matches('/').to_string(),
            secret: secret.clone(),
            owner: owner.clone(),
        }),
        _ => {
            eprintln!("VNC_AUTH_TRIGGER_SECRET and VNC_AUTH_OWNER_CHAT_ID must be set in this container's env (wf secret sync-env --for agent-shell) -- cannot reach the VNC listener without them.");
            Err(1)
        }
    }
}

impl Listener {
    /// POSTs JSON; `Err` carries the failure text as the TypeScript client words it.
    fn post(&self, path: &str, body: &Value, sep: &str) -> Result<String, String> {
        let name = path.trim_start_matches('/');
        let response = cli_http::Request::post(&format!("{}{path}", self.base))
            .via(wf_vmops::vmops::tailnet_via())
            .header("x-auth-trigger-secret", &self.secret)
            .header("x-auth-owner-id", &self.owner)
            .json(body)
            .send()?;
        let status = i64::from(response.status);
        let body = response.text();
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            let detail = if body.is_empty() { String::new() } else { format!("{sep}{body}") };
            Err(format!("{name} failed: {status}{detail}"))
        }
    }
}

fn expand_tilde(p: &str) -> std::path::PathBuf {
    crate::vnc::expand_tilde(p)
}

/// The validity command as a process: run as `user` through `s6-setuidgid` when given. The listener must run it as the user the
/// login command runs as, or it creates root-owned files (a gcloud config dir) the login then cannot write to.
fn check_command(cmd: &str, user: Option<&str>) -> Command {
    match user {
        Some(user) => {
            let mut c = Command::new("s6-setuidgid");
            c.args([user, "sh", "-c", cmd]);
            c
        }
        None => {
            let mut c = Command::new("sh");
            c.args(["-c", cmd]);
            c
        }
    }
}

/// What the tool's validity command prints (first 200 chars), so a login that is not ready can say why; empty without one.
pub(crate) fn check_output(d: &Descriptor, user: Option<&str>) -> String {
    let Some(cmd) = &d.valid_cmd else { return String::new() };
    let out = check_command(&format!("{cmd} 2>&1"), user).stdin(Stdio::null()).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).chars().take(200).collect()).unwrap_or_default()
}

/// Whether the tool's credential is there: its verify probe wins, then its validity check, then every credential file.
pub(crate) fn is_authenticated(d: &Descriptor, user: Option<&str>) -> bool {
    if let Some(probe) = &d.verify {
        return descriptor::verify(probe, &|name| descriptor::env_or_vault(name)).unwrap_or(false);
    }
    if let Some(cmd) = &d.valid_cmd {
        return check_command(cmd, user).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    }
    if let Some(path) = &d.valid_path {
        return expand_tilde(path).exists();
    }
    !d.credential_paths.is_empty() && d.credential_paths.iter().all(|p| expand_tilde(p).exists())
}

/// `Err` is an unknown tool, already reported.
fn find_tool(id: &str) -> Result<Descriptor, i32> {
    builtin(id).ok_or_else(|| {
        eprint!("Unknown tool: {id}\nKnown tools: {}\n", TOOLS.join(", "));
        1
    })
}

/// `Number(env) || undefined`: a non-zero finite number, else the default.
fn env_ms(name: &str, default: f64) -> f64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|n| n.is_finite() && *n != 0.0).unwrap_or(default)
}

/// `wf secret login <tool> [--copy] [--clear]`: the login runs in the VM's browser (the only place a person can finish it), so this
/// starts it, prints the link and waits until the listener reports the tool logged in. With `copy`, the files the tool's
/// config declares copyable are also written here. With `clear`, the tool is logged out first, here and in the VM's browser.
fn login(tool: &Descriptor, copy: bool, clear: bool) -> Result<(), i32> {
    let timeout = env_ms("VNC_AUTH_CAPTURE_TIMEOUT_MS", LINK_TTL_HOURS as f64 * 3_600_000.0);
    let poll = env_ms("VNC_AUTH_POLL_INTERVAL_MS", 2_000.0);
    let listener = connect()?;
    if clear {
        clear_login(&listener, tool)?;
    }
    let session = listener.post("/auth/start", &json!({ "tool": tool.id }), " ").map_err(|e| {
        eprintln!("Failed to start a login session: {e}");
        1
    })?;
    let s: Value = serde_json::from_str(&session).unwrap_or(Value::Null);
    let text = |k: &str| s[k].as_str().map_or("undefined".to_string(), String::from);
    print!(
        "Open this link on any device to complete {} login in the shared desktop:\n{}\nThis link is reusable across devices and expires in {LINK_TTL_HOURS} hours.\n\nWaiting for the login to finish...\n",
        tool.id,
        text("linkUrl")
    );
    let _ = std::io::stdout().flush();
    let session_id = text("sessionId");
    let mut why_not = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout.max(0.0) / 1000.0);
    // The listener judges whether the login finished (its own validity check, in the browser's container), so a stale
    // credential on this machine never counts. A listener that cannot be reached for a moment is retried.
    while std::time::Instant::now() < deadline {
        match listener.post("/auth/credential", &json!({ "sessionId": session_id, "copy": copy || tool.vault.is_some() }), " ") {
            Ok(reply) => {
                let reply: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
                why_not = reply["check"].as_str().unwrap_or_default().trim().to_string();
                if reply["ready"] == json!(true) {
                    if copy {
                        if let Err(e) = store_files(&reply["files"]) {
                            eprintln!("{}: the credential arrived but could not be stored: {e}", tool.id);
                            return Err(1);
                        }
                    }
                    let _ = listener.post("/auth/done", &json!({ "sessionId": session_id }), " ");
                    println!("{}: authenticated ✓ ({})", tool.id, if copy { "credential copied to this machine" } else { "the login stays on the VM; agent-shell has its credential" });
                    publish_to_vault(tool, &reply["files"]);
                    return if copy { finish_gcloud(tool) } else { Ok(()) };
                }
            }
            Err(e) if e.contains("declares nothing to copy") => {
                eprintln!("{}: this login declares no credential to copy; run it without --copy", tool.id);
                return Err(1);
            }
            Err(_) => {}
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(poll.max(0.0) / 1000.0));
    }
    eprintln!("{}: login did not complete within the timeout -- run the same command again for a fresh link", tool.id);
    if !why_not.is_empty() {
        eprintln!("the VM's check of the login said: {why_not}");
    }
    Err(1)
}

/// What a cleared vault secret holds (base64 of "logged-out"): a value, because the vault does not take an empty one, and not a
/// credential, so whatever restores it from the vault finds no login.
const LOGGED_OUT: &str = "bG9nZ2VkLW91dA==";

/// Mirrors the tool's declared credential file into its vault secret (base64, as the listener sent it). It needs the vault's
/// write token, which not every machine has, so a failure is reported and never fails the login that already finished.
fn publish_to_vault(tool: &Descriptor, files: &Value) {
    let Some(vault) = &tool.vault else { return };
    let content = files.as_array().into_iter().flatten().find(|f| f["path"].as_str() == Some(vault.file.as_str())).and_then(|f| f["content"].as_str());
    let Some(content) = content else {
        eprintln!("{}: the listener sent no {} to store in the vault", tool.id, vault.file);
        return;
    };
    match secrets::update_secret_value(&vault.key, content) {
        Ok(()) => println!("{}: vault secret {} updated", tool.id, vault.key),
        Err(e) => eprintln!("{}: could not update the vault secret {}: {e}", tool.id, vault.key),
    }
}

/// `--clear`: forgets the tool's login on this machine (the files its config declares copyable, gcloud's account) and in the
/// VM's login container (its credential files, a revoke, the browser's cookies), so the login that follows starts from nothing.
/// agent-shell's relayed copy stays until that login overwrites it.
fn clear_login(listener: &Listener, tool: &Descriptor) -> Result<(), i32> {
    let here: Vec<&String> = tool.copy.iter().filter(|p| std::fs::remove_file(expand_tilde(p)).is_ok()).collect();
    if let Some(vault) = &tool.vault {
        match secrets::update_secret_value(&vault.key, LOGGED_OUT) {
            Ok(()) => println!("{}: vault secret {} cleared", tool.id, vault.key),
            Err(e) => eprintln!("{}: could not clear the vault secret {}: {e}", tool.id, vault.key),
        }
    }
    if tool.id == "gcloud" {
        let _ = Command::new("gcloud").args(["config", "unset", "account"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let reply = listener.post("/auth/logout", &json!({ "tool": tool.id }), " ").map_err(|e| {
        eprintln!("Failed to clear the login on the VM: {e}");
        1
    })?;
    let reply: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
    println!(
        "{}: cleared {} file(s) here and {} on the VM{} (agent-shell keeps its copy until the new login replaces it)",
        tool.id,
        here.len(),
        reply["removed"].as_array().map_or(0, Vec::len),
        if reply["browser"] == json!(true) { ", browser cookies cleared" } else { "" }
    );
    Ok(())
}

/// Writes each `{path, content}` the listener returned under this machine's home, owner-only, replacing what was there.
fn store_files(files: &Value) -> Result<(), String> {
    use base64::Engine as _;
    use std::os::unix::fs::PermissionsExt;
    for file in files.as_array().ok_or("no files")? {
        let path = crate::vnc::expand_tilde(file["path"].as_str().ok_or("a file without a path")?);
        let bytes = base64::engine::general_purpose::STANDARD.decode(file["content"].as_str().ok_or("a file without content")?).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// gcloud keeps the active account in its config, not in `credentials.db`: select the one the copied database holds.
fn finish_gcloud(tool: &Descriptor) -> Result<(), i32> {
    if tool.id != "gcloud" {
        return Ok(());
    }
    let gcloud = |args: &[&str]| Command::new("gcloud").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let account = gcloud(&["auth", "list", "--format=value(account)"]).ok().and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(str::to_string)).filter(|a| !a.is_empty());
    match account {
        Some(account) if gcloud(&["config", "set", "account", &account]).is_ok_and(|o| o.status.success()) => {
            println!("gcloud: active account {account}");
            Ok(())
        }
        _ => {
            eprintln!("gcloud: the credential was copied but no account could be selected");
            Err(1)
        }
    }
}

fn exit_with(r: Result<(), i32>) -> Option<i32> {
    Some(r.err().unwrap_or(0))
}

fn capture(spec: Option<&String>, spec_json: Option<&String>) -> Result<(), i32> {
    if spec.is_some() == spec_json.is_some() {
        eprint!(
            "Provide exactly one of --spec <path> or --spec-json <json>.\n\nA spec looks like this — ${{VAR}} is resolved from the LISTENER's environment,\nso it names a secret without containing one:\n\n{}\n",
            serde_json::to_string_pretty(&example_spec()).expect("json")
        );
        return Err(1);
    }
    let text = match spec {
        Some(path) => std::fs::read_to_string(path).map_err(|e| {
            eprintln!("Could not read the spec as JSON: {e}");
            1
        })?,
        None => spec_json.cloned().unwrap_or_default(),
    };
    let spec: Value = serde_json::from_str(&text).map_err(|e| {
        eprintln!("Could not read the spec as JSON: {e}");
        1
    })?;
    let listener = connect()?;
    let session = listener
        .post("/auth/start", &json!({ "spec": spec }), " ")
        .map_err(|e| {
            eprintln!("Failed to start the capture session: {}", e);
            1
        })?;
    let s: Value = serde_json::from_str(&session).unwrap_or(Value::Null);
    let text = |k: &str| s[k].as_str().map_or("undefined".to_string(), String::from);
    print!(
        "Open this link on any device to complete the {} login:\n{}\n\nThis link is reusable across devices and expires in {LINK_TTL_HOURS} hours.\n\nWhen the login is finished, complete the capture with:\n  wf secret login done {}\n",
        text("tool"),
        text("linkUrl"),
        text("sessionId")
    );
    Ok(())
}

fn done(session_id: &str) -> Result<(), i32> {
    let listener = connect()?;
    listener
        .post("/auth/done", &json!({ "sessionId": session_id }), " ")
        .map_err(|e| {
            eprintln!("Capture failed: {e}");
            1
        })?;
    println!("Credential captured and delivered.");
    Ok(())
}

/// `origin + pathname` of a URL, the only parts safe to echo; `<unparseable>` when it is not one.
fn safe_display(url: &str) -> String {
    let unparseable = || "<unparseable>".to_string();
    let Some((scheme, rest)) = url.split_once("://") else {
        return unparseable();
    };
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return unparseable();
    }
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = authority.rsplit('@').next().unwrap_or_default().to_lowercase();
    let scheme = scheme.to_lowercase();
    let host = match (scheme.as_str(), host.rsplit_once(':')) {
        ("http", Some((h, "80"))) | ("https", Some((h, "443"))) => h.to_string(),
        _ => host,
    };
    if host.is_empty() {
        return unparseable();
    }
    format!("{scheme}://{host}{path}")
}

fn launch_url(url: &str) -> Result<(), i32> {
    let listener = connect()?;
    listener
        .post("/auth/launch-url", &json!({ "url": url }), ": ")
        .map_err(|e| {
            eprintln!("Failed to launch url: {e}");
            1
        })?;
    println!("Navigated the shared desktop's browser to {}", safe_display(url));
    Ok(())
}

pub fn run(args: &[String]) -> Option<i32> {
    let (sub, rest) = args.split_first()?;
    match sub.as_str() {
        "list" => {
            parse_none(rest, &[], &[])?;
            for t in TOOLS {
                println!("{t}");
            }
            Some(0)
        }
        "capture" => {
            let p = parse_none(rest, &["spec", "spec-json"], &[])?;
            exit_with(capture(p.opts.get("spec"), p.opts.get("spec-json")))
        }
        "done" => {
            let p = parse(rest, &[], &[]).filter(|p| p.positional.len() == 1)?;
            exit_with(done(&p.positional[0]))
        }
        "launch-url" => {
            let p = parse(rest, &[], &[]).filter(|p| p.positional.len() == 1)?;
            exit_with(launch_url(&p.positional[0]))
        }
        "listener" => crate::vnc::run_listener(rest),
        "login" => {
            let p = parse(rest, &[], &["copy", "clear"]).filter(|p| p.positional.len() == 1)?;
            let tool = match find_tool(&p.positional[0]) {
                Ok(t) => t,
                Err(code) => return Some(code),
            };
            exit_with(login(&tool, p.flag("copy"), p.flag("clear")))
        }
        _ => None,
    }
}
