//! `wf check --deployed <sha>`: tier 1 (typecheck and unit/flow tests of the affected packages in a clean worktree of the sha),
//! the deploy gate (each service's /health reports the sha), tier 3 (backend e2e, EAS update), the report in the artifacts
//! bucket, the PR description block, and a fix unit when tier 3 fails; `--dry-run` prints that plan without any network.
use wf_bucket::artifacts::{content_type_for, public_base, put_par, read_public, OCI_BUCKET_BASE};
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::proc::{capture, git_out, Out};
use wf_kit::root::repo_root;
use pipeline::verify::{affected, apps_for_paths, filters, is_code, workspace_packages, APP_PATHS, FRONTEND_APP, GATE_STEPS, HTTP_HEALTH_APPS};
use wf_vmops::services::{health_url, running_commit, VM_OPS_API};
use wf_vmops::vmops::{request_json, tailnet_via, Opts};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Res<T> = Result<T, String>;

const INSTALL_STEP: &str = "pnpm install --prefer-offline --frozen-lockfile";
const OUTPUT_TAIL: usize = 2048;
const REPORT_EPIC: &str = "verify";
const REPORT_BRANCH: &str = "dev";
const PR_BLOCK_START: &str = "<!-- wf-verify -->";
const PR_BLOCK_END: &str = "<!-- /wf-verify -->";

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn tail_chars(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

fn item(name: &str, status: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("status".into(), json!(status));
    m
}

fn with_detail(mut m: Map<String, Value>, detail: String) -> Value {
    m.insert("detail".into(), json!(detail));
    Value::Object(m)
}

fn tier_of(tier: i64, items: Vec<Value>, ms: i64) -> Value {
    let status = if items.is_empty() {
        "skip"
    } else if items.iter().any(|i| i["status"] == json!("fail")) {
        "fail"
    } else {
        "pass"
    };
    json!({ "tier": tier, "status": status, "ms": ms, "items": items })
}

fn failing(items: &[Value]) -> Vec<&Value> {
    items.iter().filter(|i| i["status"] == json!("fail")).collect()
}

/* ---------- commands ---------- */

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn git(root: &Path, args: &[&str]) -> Res<String> {
    let o = capture("git", &s(args), Some(root), None);
    if o.code != 0 {
        return Err(format!("git {}: {}", args.join(" "), o.stderr.trim()));
    }
    Ok(o.stdout)
}

/* ---------- OCI object store ---------- */

fn bucket() -> Res<(String, String)> {
    let prefix = std::env::var("GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX").ok().filter(|p| !p.is_empty()).ok_or("GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX is unset; it holds the golden-screenshots bucket URL prefix (terraform output golden_screenshots_bucket_url_prefix)")?;
    let re = Regex::new(r"^https://objectstorage\.([^.]+)\.oraclecloud\.com/n/([^/]+)/b/([^/]+)/o/$").unwrap();
    re.captures(&prefix).map(|m| (m[2].to_string(), m[3].to_string())).ok_or_else(|| format!("GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX does not look like a bucket URL prefix (expected .../n/<namespace>/b/<bucket>/o/, got \"{prefix}\")"))
}

fn scratch_dir() -> Res<PathBuf> {
    tempfile::Builder::new().prefix("wf-verify-").tempdir().map(tempfile::TempDir::keep).map_err(|e| e.to_string())
}

fn oci_put(key: &str, body: &str) -> Res<()> {
    let (namespace, bucket) = bucket()?;
    let dir = scratch_dir()?;
    let file = dir.join("body");
    let result = std::fs::write(&file, body).map_err(|e| e.to_string()).and_then(|()| {
        let args = s(&["os", "object", "put", "--namespace-name", &namespace, "--bucket-name", &bucket, "--name", key, "--file", &file.to_string_lossy(), "--force"]);
        let o = capture("oci", &args, None, None);
        if o.code == 0 {
            Ok(())
        } else {
            Err(format!("Command failed: oci os object put\n{}", o.stderr))
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The object's text, or None when it does not exist; any other failure is an error.
fn oci_get(key: &str) -> Res<Option<String>> {
    let (namespace, bucket) = bucket()?;
    let dir = scratch_dir()?;
    let file = dir.join("body");
    let args = s(&["os", "object", "get", "--namespace-name", &namespace, "--bucket-name", &bucket, "--name", key, "--file", &file.to_string_lossy()]);
    let o = capture("oci", &args, None, None);
    let result = if o.code == 0 {
        std::fs::read_to_string(&file).map(Some).map_err(|e| e.to_string())
    } else if Regex::new(r#"ObjectNotFound|"status":\s*404"#).unwrap().is_match(&o.stderr) {
        Ok(None)
    } else {
        Err(format!("Command failed: oci os object get\n{}", o.stderr))
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn read_tier(sha: &str, tier: i64) -> Res<Option<Value>> {
    match oci_get(&format!("verify/{sha}.tier{tier}.json"))? {
        Some(text) if !text.is_empty() => serde_json::from_str(&text).map(Some).map_err(|e| e.to_string()),
        _ => Ok(None),
    }
}

/* ---------- tier 1 ---------- */

fn shell_in(cwd: &Path, command: &str) -> (i32, String) {
    let dir = match scratch_dir() {
        Ok(d) => d,
        Err(e) => return (1, e),
    };
    let log = dir.join("out.log");
    let o = capture("sh", &["-c".into(), format!("{command} > '{}' 2>&1", log.display())], Some(cwd), Some(Duration::from_secs(30 * 60)));
    let out = std::fs::read_to_string(&log).unwrap_or(o.stderr);
    let _ = std::fs::remove_dir_all(&dir);
    (o.code, format!("{}{}", tail_chars(&out, OUTPUT_TAIL), if o.timed_out { "\n(timed out)" } else { "" }))
}

fn tier1_item(name: &str, code: i32, output: &str, ms: i64) -> Value {
    let mut m = item(name, if code == 0 { "pass" } else { "fail" });
    m.insert("ms".into(), json!(ms));
    if code == 0 {
        Value::Object(m)
    } else {
        with_detail(m, format!("exit {code}: {}", tail_chars(output, OUTPUT_TAIL)))
    }
}

fn run_tier1(root: &Path, sha: &str, changed: &[String]) -> Res<Value> {
    let files: Vec<String> = changed.iter().filter(|f| is_code(f)).cloned().collect();
    let started = now_ms();
    if files.is_empty() {
        return Ok(tier_of(1, vec![], 0));
    }
    let dir = tempfile::Builder::new().prefix("wf-verify-tier1-").tempdir().map_err(|e| e.to_string())?.keep();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    git(root, &["worktree", "add", "--detach", &dir.to_string_lossy(), sha])?;
    let mut items: Vec<Value> = vec![];
    let result = (|| -> Res<()> {
        let mut timed = |name: &str, command: &str| -> i32 {
            let t0 = now_ms();
            let (code, output) = shell_in(&dir, command);
            items.push(tier1_item(name, code, &output, now_ms() - t0));
            code
        };
        if timed(INSTALL_STEP, INSTALL_STEP) != 0 {
            return Ok(());
        }
        let packages = workspace_packages(&dir)?;
        let hit = affected(&files, &packages);
        if hit.is_empty() {
            return Ok(());
        }
        let f = filters(&hit);
        let commands = [
            format!("pnpm {f} run --if-present typecheck"),
            format!(
                "env -u GIT_INDEX_FILE -u GIT_DIR -u GIT_WORK_TREE -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES -u GIT_COMMON_DIR -u GIT_PREFIX pnpm {f} run --if-present \"/^test:(unit|flow)$/\""
            ),
        ];
        for (i, command) in commands.iter().enumerate() {
            if timed(GATE_STEPS[i], command) != 0 {
                break;
            }
        }
        Ok(())
    })();
    let _ = capture("git", &s(&["worktree", "remove", "--force", &dir.to_string_lossy()]), Some(root), None);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = capture("git", &s(&["worktree", "prune"]), Some(root), None);
    result?;
    Ok(tier_of(1, items, now_ms() - started))
}

/* ---------- the deploy gate ---------- */

enum Probe {
    Ok(String),
    Down(String),
    None,
}

fn probe_health(app: &str) -> Probe {
    if app == "vm-ops-api" {
        return match request_json("/health", &Opts::get()) {
            Ok(h) => match h["commit"].as_str().filter(|c| !c.is_empty()) {
                Some(c) => Probe::Ok(c.to_string()),
                None => Probe::Down("no commit in /health".into()),
            },
            Err(e) => Probe::Down(e),
        };
    }
    let Some(url) = health_url(app) else { return Probe::None };
    let answered = cli_http::Request::get(url).via(tailnet_via()).timeout(Duration::from_secs(5)).send();
    let body = match answered {
        Ok(response) => response.text(),
        Err(e) => return Probe::Down(e),
    };
    match serde_json::from_str::<Value>(&body) {
        Ok(v) => match v["commit"].as_str() {
            Some(c) => Probe::Ok(c.to_string()),
            None => Probe::Down("no commit in /health".into()),
        },
        Err(e) => Probe::Down(e.to_string()),
    }
}

/// `commit` has `sha` in its history (fetching dev once when the checkout does not know `commit` yet).
fn descends_from(commit: &str, sha: &str) -> bool {
    let is_ancestor = || std::process::Command::new("git").args(["merge-base", "--is-ancestor", sha, commit]).stderr(std::process::Stdio::null()).status().map(|s| s.code());
    match is_ancestor() {
        Ok(Some(0)) => true,
        Ok(Some(1)) => false,
        _ => {
            let _ = std::process::Command::new("git").args(["fetch", "-q", "origin", "dev"]).stderr(std::process::Stdio::null()).status();
            matches!(is_ancestor(), Ok(Some(0)))
        }
    }
}

/// The commit `app` serves now. An app with a `/health` reports it there; one without is registered in `IMAGE_CONFIRMED` and confirmed by
/// the `sha-<short>` tag of its running image, which `wf ship` sets to the shipped commit for a rebuilt and a re-tagged image alike, so
/// nothing is baked into the app to answer for it.
fn serving_commit(app: &str) -> Result<Option<String>, String> {
    if app == VM_OPS_API {
        return Ok(running_commit(app));
    }
    match probe_health(app) {
        Probe::Ok(c) => Ok(Some(c)),
        Probe::Down(_) => Ok(None),
        Probe::None if IMAGE_CONFIRMED.contains(&app) => Ok(running_commit(app)),
        Probe::None => Err(format!("{app} has no /health that reports its commit")),
    }
}

/// Apps without a `/health` whose deploy is confirmed by their running image's tag (see `serving_commit`).
const IMAGE_CONFIRMED: &[&str] = &["vnc-auth-session", "leetcode-auth-cookie-capture"];

/// Waits until `app` serves `sha` or a commit after it: a deploy is done when the service runs what was pushed. `failed` is asked each
/// round and ends the wait with its text.
pub(crate) fn wait_serving(app: &str, sha: &str, timeout: Duration, failed: impl Fn() -> Option<String>) -> Result<String, String> {
    let started = Instant::now();
    loop {
        let reported = match serving_commit(app) {
            Ok(c) => c,
            Err(why) => return Ok(format!("not confirmed: {why}")),
        };
        if let Some(c) = reported.as_deref().filter(|c| commit_matches(Some(c), sha) || descends_from(c, sha) || own_image_unchanged(app, c, sha)) {
            return Ok(format!("serving {c} after {} s", started.elapsed().as_secs()));
        }
        if let Some(why) = failed() {
            return Err(why);
        }
        if started.elapsed() > timeout {
            return Err(format!("{app} still serves {} after {} s, not {sha}", reported.as_deref().unwrap_or("nothing"), timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

/// Whether the commit `app` serves was built from the same inputs of the app's own image as `sha`: a gradual redeploy that moved only a sidecar
/// leaves the app's own service on the commit it was built from, and that is the commit it should serve.
fn own_image_unchanged(app: &str, serving: &str, sha: &str) -> bool {
    let Some(own) = pipeline::image_hash::service_images(app).first() else { return false };
    pipeline::image_hash::unchanged_between(None, serving, sha, own.image)
}

fn commit_matches(reported: Option<&str>, sha: &str) -> bool {
    match reported {
        Some(r) if r.len() >= 7 => sha.starts_with(r) || r.starts_with(sha),
        _ => false,
    }
}

fn gate_decision(sha: &str, health: &[(String, Probe)]) -> Vec<String> {
    health
        .iter()
        .filter(|(_, p)| !matches!(p, Probe::None) && !matches!(p, Probe::Ok(c) if commit_matches(Some(c), sha)))
        .map(|(app, p)| {
            let why = match p {
                Probe::Ok(c) => format!("commit {c}"),
                Probe::Down(d) => d.clone(),
                Probe::None => String::new(),
            };
            format!("{app} ({why})")
        })
        .collect()
}

/* ---------- tier 3 suites ---------- */

fn run_command_suite(root: &Path, name: &str, command: &str) -> Value {
    let o = capture("sh", &s(&["-c", command]), Some(root), Some(Duration::from_secs(15 * 60)));
    if o.code == 0 {
        return Value::Object(item(name, "pass"));
    }
    let text = if o.stderr.is_empty() { &o.stdout } else { &o.stderr };
    with_detail(item(name, "fail"), format!("exit {}: {}", o.code, tail_chars(text.trim(), 400)))
}

fn backend_suite(app: &str) -> Option<&'static str> {
    match app {
        "remote-chat-agent" => Some("REMOTE_CHAT_AGENT_BASE_URL=\"${REMOTE_CHAT_AGENT_BASE_URL:-http://remote-chat-agent-prod:8787}\" pnpm --filter @app/remote-chat-agent run test:e2e"),
        _ => None,
    }
}

fn eas_publish(root: &Path, branch: &str, route: &str, message: &str) -> Value {
    let done = crate::ship_eas::publish(root, "mla-tamagui", branch, route, message);
    if done.code == 0 {
        return with_detail(item("eas update", "pass"), format!("branch {branch}, initial route {route}"));
    }
    with_detail(item("eas update", "fail"), format!("exit {}: {}", done.code, tail_chars(done.text.trim(), 400)))
}

/* ---------- the report ---------- */

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn render_report_html(report: &Value) -> String {
    let sha8: String = report["sha"].as_str().unwrap_or("").chars().take(8).collect();
    let tiers: Vec<String> = report["tiers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let rows: String = t["items"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| format!("<tr><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>", esc(i["name"].as_str().unwrap_or("")), i["status"].as_str().unwrap_or(""), i["status"].as_str().unwrap_or(""), esc(i["detail"].as_str().unwrap_or(""))))
                .collect();
            format!("<h2>Tier {}: <span class=\"{}\">{}</span> <small>{} ms</small></h2><table>{rows}</table>", t["tier"], t["status"].as_str().unwrap_or(""), t["status"].as_str().unwrap_or(""), t["ms"])
        })
        .collect();
    let deployed: String = report["deployed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| format!("<li>{} @ {} {}</li>", esc(d["app"].as_str().unwrap_or("")), esc(d["commit"].as_str().unwrap_or("")), if d["healthOk"] == json!(true) { "ok" } else { "NOT OK" }))
        .collect();
    let units: Vec<&str> = report["units"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let units = if units.is_empty() { "none".to_string() } else { units.join(", ") };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>Verify {sha8}</title>\n<style>body{{font:15px system-ui;margin:16px;background:#fff;color:#111}}td{{padding:2px 8px;border-bottom:1px solid #ddd}}.pass{{color:#137333}}.fail{{color:#c5221f}}.skip{{color:#666}}\n@media (prefers-color-scheme:dark){{body{{background:#111;color:#eee}}}}</style></head><body>\n<h1>Verify dev@{sha8}</h1><p>{}</p>\n<p>Units: {}</p><ul>{deployed}</ul>\n{}\n</body></html>\n",
        update_html(&report["update"]),
        esc(&units),
        tiers.join("\n")
    )
}

fn mark(t: &Value) -> &'static str {
    match t["status"].as_str() {
        Some("pass") => "✅",
        Some("fail") => "❌",
        _ => "—",
    }
}

/// The update the branch's latest-update pointer names, as the report holds it: its link, the `exp://` deep link beside it when
/// the app config names the project, and the commit it was built from. Null when no update is stored, `error` when the read failed.
fn latest_update(root: &Path, branch: &str) -> Value {
    match pipeline::eas::latest_pointer(branch) {
        Ok(p) => match p["link"].as_str() {
            Some(link) => {
                let project = pipeline::eas::config_field(&root.join("frontend").join(FRONTEND_APP), "projectId");
                let deep = p["group"].as_str().zip(project).map(|(group, id)| format!("exp://u.expo.dev/{id}/group/{group}"));
                json!({ "link": link, "deepLink": deep, "commit": p["commit"], "group": p["group"] })
            }
            None => Value::Null,
        },
        Err(e) if e.contains("HTTP 404") => Value::Null,
        Err(e) => json!({ "error": e }),
    }
}

fn built_from(update: &Value) -> Option<String> {
    update["commit"].as_str().filter(|c| !c.is_empty()).map(|c| c.chars().take(8).collect())
}

/// The PR block's way to try the app: the update's link (GitHub strips `exp://`, so the deep link rides in a code span).
fn update_md(update: &Value) -> String {
    if let Some(link) = update["link"].as_str() {
        let deep = update["deepLink"].as_str().map_or(String::new(), |d| format!(" · `{d}`"));
        let from = built_from(update).map_or(String::new(), |c| format!(" · built from `{c}`"));
        return format!("[Open the update]({link}){deep}{from}");
    }
    match update["error"].as_str() {
        Some(e) => format!("update link unavailable ({e})"),
        None => "no update published for this commit".to_string(),
    }
}

fn update_html(update: &Value) -> String {
    if let Some(link) = update["link"].as_str() {
        let deep = update["deepLink"].as_str().map_or(String::new(), |d| format!(" · <code>{}</code>", esc(d)));
        let from = built_from(update).map_or(String::new(), |c| format!(" · built from <code>{}</code>", esc(&c)));
        return format!("<a href=\"{}\">Open the update</a>{deep}{from}", esc(link));
    }
    esc(&match update["error"].as_str() {
        Some(e) => format!("update link unavailable ({e})"),
        None => "no update published for this commit".to_string(),
    })
}

fn pr_block(report: &Value, url: Option<&str>) -> String {
    let tiers = &report["tiers"];
    let sha8: String = report["sha"].as_str().unwrap_or("").chars().take(8).collect();
    let link = url.map_or(String::new(), |u| format!(" · [report]({u})"));
    [
        PR_BLOCK_START.to_string(),
        format!("**Try it:** {}", update_md(&report["update"])),
        format!("**Verify:** `dev@{sha8}` · flow {} · visual {} · e2e {}{link}", mark(&tiers[0]), mark(&tiers[1]), mark(&tiers[2])),
        PR_BLOCK_END.to_string(),
    ]
    .join("\n")
}

fn replace_pr_block(body: &str, block: &str) -> String {
    if let (Some(start), Some(end)) = (body.find(PR_BLOCK_START), body.find(PR_BLOCK_END)) {
        if end > start {
            return format!("{}{block}{}", &body[..start], &body[end + PR_BLOCK_END.len()..]);
        }
    }
    if body.trim().is_empty() {
        block.to_string()
    } else {
        format!("{block}\n\n{body}")
    }
}

fn gh(args: &[String]) -> Out {
    capture("gh", args, None, None)
}

/// Returns why the PR body was not updated, or None when it was.
fn rewrite_pr_block(pr: u64, block: &str) -> Option<String> {
    let got = gh(&s(&["api", &format!("repos/:owner/:repo/pulls/{pr}"), "--jq", ".body // \"\""]));
    if got.code != 0 {
        return Some(if got.stderr.trim().is_empty() { "gh unavailable".into() } else { got.stderr.trim().to_string() });
    }
    let body = got.stdout.strip_suffix('\n').unwrap_or(&got.stdout).to_string();
    let path = format!("repos/:owner/:repo/pulls/{pr}");
    let put = gh(&["api".into(), "--method".into(), "PATCH".into(), path.clone(), "-f".into(), format!("body={}", replace_pr_block(&body, block))]);
    if put.code != 0 {
        return Some(format!("gh api --method PATCH {path} failed: {}", put.stderr));
    }
    None
}

/* ---------- publishing the report ---------- */

fn publish_report(report: &Value, par: &str) -> Res<String> {
    let sha = report["sha"].as_str().unwrap_or("");
    let rels = [("index.html", render_report_html(report)), ("report.json", serde_json::to_string_pretty(report).map_err(|e| e.to_string())?)];
    for (rel, body) in &rels {
        let key = format!("verification/{REPORT_EPIC}/{REPORT_BRANCH}/{sha}/{rel}");
        put_par(&format!("{par}{key}"), body.as_bytes(), content_type_for(rel))?;
    }
    let url = format!("{OCI_BUCKET_BASE}/verification/{REPORT_EPIC}/{REPORT_BRANCH}/{sha}/index.html");
    let manifest_key = format!("verification/{REPORT_EPIC}/index.json");
    let existing: Vec<Value> = read_public(&format!("{}/{manifest_key}", public_base()))?
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            let runs = v["runs"].as_array()?.clone();
            runs.iter().all(|r| ["branch", "runId", "publishedAt", "url"].iter().all(|k| r[*k].is_string())).then_some(runs)
        })
        .unwrap_or_default();
    let entry = json!({ "branch": REPORT_BRANCH, "runId": sha, "publishedAt": rca_core::time::iso(now_ms()), "url": url });
    let mut runs = vec![entry];
    runs.extend(existing.into_iter().filter(|r| !(r["branch"] == json!(REPORT_BRANCH) && r["runId"] == json!(sha))));
    put_par(&format!("{par}{manifest_key}"), json!({ "runs": runs }).to_string().as_bytes(), "application/json")?;
    put_par(&format!("{par}verification/{REPORT_EPIC}/index.html"), epic_index_html(&runs).as_bytes(), "text/html")?;
    Ok(url)
}

fn escape_html(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn epic_index_html(runs: &[Value]) -> String {
    let mut sorted: Vec<&Value> = runs.iter().collect();
    sorted.sort_by(|a, b| b["publishedAt"].as_str().cmp(&a["publishedAt"].as_str()));
    let links: Vec<String> = sorted
        .iter()
        .map(|r| format!("      <li><a href=\"{}\">{} / {} ({})</a></li>", escape_html(r["url"].as_str().unwrap_or("")), escape_html(r["branch"].as_str().unwrap_or("")), escape_html(r["runId"].as_str().unwrap_or("")), escape_html(r["publishedAt"].as_str().unwrap_or(""))))
        .collect();
    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\">\n    <title>Verification artifacts: {e}</title>\n  </head>\n  <body>\n    <h1>Verification artifacts: {e}</h1>\n    <ul>\n{}\n    </ul>\n  </body>\n</html>\n",
        links.join("\n"),
        e = escape_html(REPORT_EPIC)
    )
}

/* ---------- the fix unit ---------- */

fn fix_plan_path(sha: &str) -> String {
    format!("docs/plans/fix-{}/units.jsonl", &sha[..8.min(sha.len())])
}

fn fix_unit_files(failing_apps: &[&str], changed: &[String]) -> Vec<String> {
    let owned: Vec<String> = changed.iter().filter(|p| failing_apps.iter().any(|a| apps_for_paths(std::slice::from_ref(p)).contains(a))).take(3).cloned().collect();
    if !owned.is_empty() {
        return owned;
    }
    let roots: Vec<String> = failing_apps.iter().filter_map(|a| APP_PATHS.iter().find(|(_, app)| app == a).map(|(d, _)| format!("{d}/package.json"))).take(3).collect();
    if roots.is_empty() {
        vec!["frontend/mla-tamagui/package.json".into()]
    } else {
        roots
    }
}

// ---------- the run ----------

struct Flags {
    sha: String,
    base: String,
    timeout_ms: f64,
    poll_ms: f64,
    publish_eas: Option<bool>,
    eas_branch: String,
    initial_route: String,
    publish: bool,
    pr: Option<u64>,
    skip_tier1: bool,
}

fn seconds(v: &str, name: &str) -> Result<f64, String> {
    match v.trim().parse::<f64>() {
        Ok(n) if n.is_finite() && n > 0.0 => Ok(n * 1000.0),
        _ => Err(format!("{name} must be a positive number of seconds")),
    }
}

struct Validated {
    sha: String,
    timeout_ms: f64,
    poll_ms: f64,
    pr: Option<u64>,
}

/// The arguments both the real run and `--dry-run` check, with the same words for each failure (exit code, message).
fn validate(given: &str, p: &wf_kit::args::Parsed, root: &Path) -> Result<Validated, (i32, String)> {
    let sha = git_out(root, &["rev-parse", "--verify", &format!("{given}^{{commit}}")]).map(|o| o.trim().to_string()).ok_or_else(|| (2, format!("verify: {given} is not a commit in this checkout (git fetch origin dev?)")))?;
    let timeout_ms = seconds(p.opts.get("gate-timeout").map_or("1800", String::as_str), "--gate-timeout").map_err(|e| (2, e))?;
    let poll_ms = seconds(p.opts.get("poll").map_or("15", String::as_str), "--poll").map_err(|e| (2, e))?;
    let pr = match p.opts.get("pr") {
        None => None,
        Some(v) => match v.trim().parse::<f64>() {
            Ok(n) if n.fract() == 0.0 && n >= 0.0 => Some(n as u64),
            _ => return Err((2, "--pr must be a PR number".into())),
        },
    };
    Ok(Validated { sha, timeout_ms, poll_ms, pr })
}

struct Outcome {
    report: Option<Value>,
    report_key: Option<String>,
    report_url: Option<String>,
    apps: Vec<String>,
    notes: Vec<String>,
    fix_unit: Option<(String, String, Vec<String>)>,
    exit_code: i32,
}

fn run_tier3(root: &Path, f: &Flags, say: &mut dyn FnMut(&str)) -> Res<Outcome> {
    let sha = &f.sha;
    let paths: Vec<String> = git(root, &["diff", "--name-only", &f.base, sha])?.split('\n').filter(|p| !p.is_empty()).map(str::to_string).collect();
    let apps: Vec<&'static str> = apps_for_paths(&paths);
    let publish_eas = f.publish_eas.unwrap_or_else(|| apps.contains(&FRONTEND_APP));
    let mut outcome = Outcome { report: None, report_key: None, report_url: None, apps: apps.iter().map(|a| a.to_string()).collect(), notes: vec![], fix_unit: None, exit_code: 0 };

    let tier1 = if f.skip_tier1 { None } else { Some(run_tier1(root, sha, &paths)?) };
    if let Some(t) = &tier1 {
        oci_put(&format!("verify/{sha}.tier1.json"), &t.to_string())?;
    }
    let started = now_ms();

    // The deploy gate.
    let gated: Vec<&str> = apps.iter().copied().filter(|a| HTTP_HEALTH_APPS.contains(a)).collect();
    let gate_started = now_ms();
    let (gate_item, health) = loop {
        let health: Vec<(String, Probe)> = gated.iter().map(|a| (a.to_string(), probe_health(a))).collect();
        let pending = gate_decision(sha, &health);
        let waited = now_ms() - gate_started;
        if pending.is_empty() {
            break (with_detail(item("deploy gate", "pass"), format!("{waited} ms")), health);
        }
        if waited as f64 >= f.timeout_ms {
            break (with_detail(item("deploy gate", "fail"), format!("timeout after {waited} ms; pending: {}", pending.join(", "))), health);
        }
        say(&format!("verify: waiting on {}", pending.join(", ")));
        std::thread::sleep(Duration::from_millis(f.poll_ms as u64));
    };
    let gate_passed = gate_item["status"] == json!("pass");
    let mut items = if gated.is_empty() { vec![] } else { vec![gate_item] };
    let mut failing_apps: Vec<&str> = if gate_passed { vec![] } else { apps.clone() };
    if gate_passed {
        for app in &apps {
            if let Some(command) = backend_suite(app) {
                let result = run_command_suite(root, &format!("{app}:e2e"), command);
                if result["status"] == json!("fail") {
                    failing_apps.push(app);
                }
                items.push(result);
            }
        }
        if publish_eas {
            let has_token = secrets::load_values(&["EXPO_TOKEN"]).get("EXPO_TOKEN").cloned().flatten().is_some();
            if !has_token {
                outcome.notes.push("EAS Update skipped: EXPO_TOKEN not resolvable".into());
            } else {
                items.push(eas_publish(root, &f.eas_branch, &f.initial_route, &format!("dev@{} tier 3", &sha[..8.min(sha.len())])));
            }
        }
    }
    let tier3 = tier_of(3, items.clone(), now_ms() - started);

    // The report.
    let units: Vec<String> = git(root, &["log", &format!("{}..{sha}", f.base), "--format=%(trailers:key=Unit,valueonly)"])?.split('\n').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    let tier1_record = match &tier1 {
        Some(t) => Some(t.clone()),
        None => read_tier(sha, 1)?,
    };
    let tier2_record = read_tier(sha, 2)?;
    let landed = git(root, &["show", "-s", "--format=%ct", sha]).ok().and_then(|t| t.trim().parse::<i64>().ok()).map(|s| s * 1000);
    let deployed: Vec<Value> = health
        .iter()
        .filter(|(_, p)| !matches!(p, Probe::None))
        .map(|(app, p)| {
            let (commit, ok) = match p {
                Probe::Ok(c) => (c.clone(), commit_matches(Some(c), sha)),
                _ => ("unreachable".to_string(), false),
            };
            json!({ "app": app, "commit": commit, "healthOk": ok })
        })
        .collect();
    let green = tier3["status"] == json!("pass") && landed.is_some();
    let update = latest_update(root, &f.eas_branch);
    let skip = |n: i64| tier_of(n, vec![], 0);
    let report = json!({
        "sha": sha,
        "units": units,
        "tiers": [tier1_record.unwrap_or_else(|| skip(1)), tier2_record.unwrap_or_else(|| skip(2)), tier3.clone()],
        "leadTime": { "preLandMs": [], "deployVerifiedMs": if green { json!(now_ms() - landed.unwrap_or(0)) } else { Value::Null } },
        "deployed": deployed,
        "update": update,
    });
    let key = format!("verify/{sha}.json");
    oci_put(&key, &serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?)?;
    outcome.report_key = Some(key);
    if f.publish {
        let par = secrets::load_values(&["VERIFICATION_ARTIFACTS_PAR_URL"]).get("VERIFICATION_ARTIFACTS_PAR_URL").cloned().flatten().ok_or("VERIFICATION_ARTIFACTS_PAR_URL could not be resolved (`wf secret set VERIFICATION_ARTIFACTS_PAR_URL`)")?;
        outcome.report_url = Some(publish_report(&report, &par)?);
    }
    if let Some(pr) = f.pr {
        let block = pr_block(&report, outcome.report_url.as_deref());
        if let Some(why) = rewrite_pr_block(pr, &block) {
            outcome.notes.push(format!("PR #{pr} not updated ({why}); block printed instead:\n{block}"));
        }
    }
    if tier1.as_ref().is_some_and(|t| t["status"] == json!("fail")) {
        outcome.exit_code = 1;
    }
    if tier3["status"] == json!("fail") {
        outcome.exit_code = 1;
        let path = fix_plan_path(sha);
        let existing = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
        let sha8 = &sha[..8.min(sha.len())];
        let n = existing.split('\n').filter(|l| !l.trim().is_empty()).count() + 1;
        let id = format!("fix-{sha8}-{n:02}");
        let failed: Vec<String> = failing(&items).iter().map(|i| format!("- {}{}", i["name"].as_str().unwrap_or(""), i["detail"].as_str().map_or(String::new(), |d| format!(": {d}")))).collect();
        let files: Vec<String> = fix_unit_files(&failing_apps, &paths).into_iter().take(3).collect();
        let unit = json!({
            "id": id,
            "title": format!("Fix tier-3 failure on dev@{sha8}"),
            "tier": "e2e",
            "files": files,
            "brief": format!("Stack: see the files listed.\nTASK: make tier 3 green again for dev@{sha8}.\nFailures:\n{}\nCONSTRAINTS: fix the cause, never weaken the suite; Given/When/Then tests.", failed.join("\n")),
            "deps": [],
            "gate": "wf check --deployed \"$(git rev-parse HEAD)\" --no-publish-eas",
            "needs": "deployed",
        });
        let abs = root.join(&path);
        if let Some(dir) = abs.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&abs).map_err(|e| e.to_string())?;
        writeln!(file, "{unit}").map_err(|e| e.to_string())?;
        outcome.fix_unit = Some((path.clone(), id.clone(), vec![
            "git fetch origin dev && git switch dev && git rebase origin/dev".to_string(),
            format!("git add {path}"),
            format!("git commit -m \"plan(fix-{sha8}): enqueue {id}\""),
            "git push origin dev".to_string(),
        ]));
    }
    outcome.report = Some(report);
    Ok(outcome)
}

fn render(o: &Outcome) -> String {
    let mut out = format!("changed apps: {}\n", if o.apps.is_empty() { "none".to_string() } else { o.apps.join(", ") });
    for t in o.report.as_ref().and_then(|r| r["tiers"].as_array()).into_iter().flatten() {
        out.push_str(&format!("tier {}: {}", t["tier"], t["status"].as_str().unwrap_or("")));
        for i in failing(&t["items"].as_array().cloned().unwrap_or_default()) {
            out.push_str(&format!("\n  x {}: {}", i["name"].as_str().unwrap_or(""), i["detail"].as_str().unwrap_or("")));
        }
        out.push('\n');
    }
    if let Some(k) = &o.report_key {
        out.push_str(&format!("report: oci {k}\n"));
    }
    if let Some(u) = &o.report_url {
        out.push_str(&format!("report html: {u}\n"));
    }
    for n in &o.notes {
        out.push_str(&format!("note: {n}\n"));
    }
    if let Some((path, id, commands)) = &o.fix_unit {
        out.push_str(&format!("fix unit {id} appended to {path}; land it on dev with:\n"));
        for c in commands {
            out.push_str(&format!("  {c}\n"));
        }
    }
    out
}

fn outcome_json(o: &Outcome) -> Value {
    json!({
        "ok": true,
        "report": o.report.clone().unwrap_or(Value::Null),
        "reportKey": o.report_key,
        "reportUrl": o.report_url,
        "apps": o.apps,
        "plan": [],
        "notes": o.notes,
        "fixUnit": o.fix_unit.as_ref().map(|(path, id, commands)| json!({ "path": path, "id": id, "commands": commands })),
        "exitCode": o.exit_code,
    })
}

/// What the real (not dry) run takes.
pub const SPEC: Spec = Spec {
    values: &["base", "gate-timeout", "poll", "eas-branch", "initial-route", "pr"],
    flags: &["json", "publish-eas", "no-publish-eas", "publish", "skip-tier1"],
    positional: Positional::Exactly(1),
    ..Spec::NONE
};

/// The real (not dry) run; `None` for anything it does not serve.
pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let json_mode = p.flag("json");
    let [given] = p.positional.as_slice() else { return None };
    let fail = |code: i32, message: String| {
        if json_mode {
            println!("{}", json!({ "ok": false, "error": message }));
        } else {
            wf_kit::wf_err!("wf: {message}");
        }
        Some(code)
    };
    let root = repo_root();
    let v = match validate(given, &p, &root) {
        Ok(v) => v,
        Err((code, message)) => return fail(code, message),
    };
    let flags = Flags {
        base: p.opts.get("base").cloned().unwrap_or_else(|| format!("{}~1", v.sha)),
        sha: v.sha,
        timeout_ms: v.timeout_ms,
        poll_ms: v.poll_ms,
        publish_eas: p.flags.iter().rev().find(|f| *f == "publish-eas" || *f == "no-publish-eas").map(|f| f == "publish-eas"),
        eas_branch: p.opts.get("eas-branch").cloned().unwrap_or_else(|| "dev".into()),
        initial_route: p.opts.get("initial-route").cloned().unwrap_or_else(|| "/".into()),
        publish: p.flags.iter().any(|f| f == "publish"),
        pr: v.pr,
        skip_tier1: p.flags.iter().any(|f| f == "skip-tier1"),
    };
    let mut say = |line: &str| eprintln!("{line}");
    match run_tier3(&root, &flags, &mut say) {
        Ok(outcome) => {
            if json_mode {
                println!("{}", outcome_json(&outcome));
            } else {
                print!("{}", render(&outcome));
            }
            Some(outcome.exit_code)
        }
        Err(e) => fail(1, e),
    }
}

/* ---------- <sha> --dry-run ---------- */

fn tier1_plan(root: &Path, sha: &str, changed: &[String]) -> Option<Vec<String>> {
    let files: Vec<String> = changed.iter().filter(|f| is_code(f)).cloned().collect();
    if files.is_empty() {
        return Some(vec!["tier 1: skipped (no code file changed)".into()]);
    }
    let packages = workspace_packages(root).ok()?;
    let hit = affected(&files, &packages);
    let mut plan = vec![
        format!("tier 1: git worktree add --detach <tmpdir>/wf-verify-tier1-* {} (removed after)", &sha[..8.min(sha.len())]),
        "tier 1: pnpm install --prefer-offline --frozen-lockfile".to_string(),
    ];
    if hit.is_empty() {
        plan.push("tier 1: no workspace package affected".into());
    } else {
        let f = filters(&hit);
        let commands = [
            format!("pnpm {f} run --if-present typecheck"),
            format!(
                "env -u GIT_INDEX_FILE -u GIT_DIR -u GIT_WORK_TREE -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES -u GIT_COMMON_DIR -u GIT_PREFIX pnpm {f} run --if-present \"/^test:(unit|flow)$/\""
            ),
        ];
        for (i, c) in commands.iter().enumerate() {
            plan.push(format!("tier 1: {}: {c}", GATE_STEPS[i]));
        }
    }
    plan.push(format!("oci os object put verify/{sha}.tier1.json"));
    Some(plan)
}

/// What `--dry-run` takes: the real run's options and the flag.
pub const DRY_RUN: Spec = Spec {
    values: &["base", "gate-timeout", "poll", "eas-branch", "initial-route", "pr"],
    flags: &["dry-run", "publish-eas", "no-publish-eas", "publish", "skip-tier1"],
    positional: Positional::Exactly(1),
    ..Spec::NONE
};

/// `check <sha> --dry-run`: the plan of a deployed commit's verification, printed without any network.
pub fn dry_run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &DRY_RUN)?;
    let [given] = p.positional.as_slice() else { return None };
    let root = repo_root();
    let v = match validate(given, &p, &root) {
        Ok(v) => v,
        Err((code, message)) => {
            wf_kit::wf_err!("wf: {message}");
            return Some(code);
        }
    };
    let (sha, timeout, poll, pr) = (v.sha, v.timeout_ms, v.poll_ms, v.pr);
    let base = p.opts.get("base").cloned().unwrap_or_else(|| format!("{sha}~1"));
    let paths: Vec<String> = git_out(&root, &["diff", "--name-only", &base, &sha])?.split('\n').filter(|s| !s.is_empty()).map(str::to_string).collect();
    let apps = apps_for_paths(&paths);
    let publish_eas = p
        .flags
        .iter()
        .rev()
        .find(|f| *f == "publish-eas" || *f == "no-publish-eas")
        .map_or_else(|| apps.contains(&FRONTEND_APP), |f| f == "publish-eas");
    let skip_tier1 = p.flags.iter().any(|f| f == "skip-tier1");
    let eas_branch = p.opts.get("eas-branch").map_or("dev", String::as_str);
    let route = p.opts.get("initial-route").map_or("/", String::as_str);
    let short = &sha[..8.min(sha.len())];

    let mut plan = if skip_tier1 { vec!["tier 1: skipped (--skip-tier1)".to_string()] } else { tier1_plan(&root, &sha, &paths)? };
    for a in &apps {
        plan.push(if HTTP_HEALTH_APPS.contains(a) { format!("GET {a} /health until commit == {short} (timeout {timeout} ms, every {poll} ms)") } else { format!("{a}: no HTTP /health, not gated") });
    }
    if apps.contains(&"remote-chat-agent") {
        plan.push("sh: REMOTE_CHAT_AGENT_BASE_URL=\"${REMOTE_CHAT_AGENT_BASE_URL:-http://remote-chat-agent-prod:8787}\" pnpm --filter @app/remote-chat-agent run test:e2e".into());
    }
    plan.push(if publish_eas {
        format!("eas update via publish:eas --branch {eas_branch} --initial-route {route}")
    } else {
        "eas update: skipped (no frontend change or --no-publish-eas)".into()
    });
    plan.push(if skip_tier1 {
        "read tier 1 + tier 2 records: OCI verify/<sha>.tier1.json, verify/<sha>.tier2.json".into()
    } else {
        "read tier 2 record: OCI verify/<sha>.tier2.json".into()
    });
    plan.push(format!("oci os object put verify/{sha}.json"));
    if p.flags.iter().any(|f| f == "publish") {
        plan.push(format!("PUT report HTML to verification/verify/dev/{sha}/index.html"));
    }
    if let Some(n) = pr {
        plan.push(format!("rewrite PR #{n} description block"));
    }
    plan.push(format!("on failure: append a fix unit to docs/plans/fix-{short}/units.jsonl"));

    let mut out = format!("changed apps: {}\nplan (dry run, no network):\n", if apps.is_empty() { "none".to_string() } else { apps.join(", ") });
    for step in plan {
        out.push_str(&format!("  - {step}\n"));
    }
    print!("{out}");
    Some(0)
}
