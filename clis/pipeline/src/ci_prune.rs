//! `wf ci prune [--pr N] [--dry-run] [--commits]`: removes the CI comments of heads the pull request has moved past, so it carries only the
//! latest head's comment of each workflow. Only `github-actions[bot]` comments whose first line is exactly a CI marker are candidates; a
//! comment whose run cannot be resolved is kept. `sync_live` runs the same pass after it writes its own comment.
use crate::ci_comment::{json_of, open_pr, redact, send, text, var, Env};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["dry-run", "commits"], values: &["pr"], positional: Positional::None, ..Spec::NONE };
const MAX_DELETES: usize = 25;
const MAX_LOOKUPS: usize = 30;
const BOT: &str = "github-actions[bot]";

/// The flavor and run id of a CI comment whose first line is exactly a marker; `None` for every other comment.
pub(crate) fn marker_parts(body: &str) -> Option<(String, u64)> {
    static MARKER: OnceLock<Regex> = OnceLock::new();
    let marker = MARKER.get_or_init(|| Regex::new(r"^<!-- (?:ci-live|ci-comment)(:[a-z0-9-]+)? run:(\d+)(?: runner:\S+)? -->$").expect("a valid pattern"));
    let first = body.lines().next()?.trim_end();
    let c = marker.captures(first)?;
    Some((c.get(1).map_or(String::new(), |m| m.as_str().to_string()), c[2].parse().ok()?))
}

struct Seen {
    id: u64,
    scope: &'static str,
    flavor: String,
    run: u64,
    sha: Option<String>,
}

pub(crate) fn is_bot(c: &Value) -> bool {
    c["user"]["login"] == BOT && c["user"]["type"] == "Bot"
}

fn pages(env: &Env, path: &str, max: usize) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for page in 1..=max {
        let sep = if path.contains('?') { '&' } else { '?' };
        let got = json_of(send(env, "GET", &format!("{}{path}{sep}per_page=100&page={page}", env.api), None)?, "listing")?;
        let got = got.as_array().cloned().unwrap_or_default();
        let full = got.len() >= 100;
        out.extend(got);
        if !full {
            break;
        }
    }
    Ok(out)
}

/// Judges and deletes (unless `dry`) the stale CI comments of the pull request; the answer lists what went and what stayed.
pub(crate) fn prune_pr(env: &Env, pr: u64, head: &str, commits: bool, dry: bool) -> Value {
    let (mut deleted, mut kept, mut errors) = (Vec::<Value>::new(), Vec::<Value>::new(), Vec::<String>::new());
    if var("WF_CI_PRUNE").is_some_and(|v| v == "off") {
        return json!({ "ok": true, "dryRun": dry, "head": head, "deleted": [], "kept": [], "errors": ["pruning is off (WF_CI_PRUNE=off)"] });
    }
    let (mine_flavor, mine_run) = (crate::ci_comment::flavor(env), env.run.parse::<u64>().unwrap_or(0));
    let mut seen: Vec<Seen> = Vec::new();
    match pages(env, &format!("/repos/{}/issues/{pr}/comments", env.repo), 10) {
        Ok(list) => {
            for c in list.iter().filter(|c| is_bot(c)) {
                if let Some((flavor, run)) = c["body"].as_str().and_then(marker_parts) {
                    seen.push(Seen { id: c["id"].as_u64().unwrap_or(0), scope: "pr", flavor, run, sha: None });
                }
            }
        }
        Err(e) => errors.push(e),
    }
    if commits {
        match pages(env, &format!("/repos/{}/pulls/{pr}/commits", env.repo), 1) {
            Ok(list) => {
                for commit in list.iter().rev().take(50).filter(|c| c["commit"]["comment_count"].as_u64().unwrap_or(0) > 0) {
                    let sha = text(commit, "sha");
                    match pages(env, &format!("/repos/{}/commits/{sha}/comments", env.repo), 2) {
                        Ok(comments) => {
                            for c in comments.iter().filter(|c| is_bot(c)) {
                                if let Some((flavor, run)) = c["body"].as_str().and_then(marker_parts) {
                                    seen.push(Seen { id: c["id"].as_u64().unwrap_or(0), scope: "commit", flavor, run, sha: Some(sha.clone()) });
                                }
                            }
                        }
                        Err(e) => errors.push(e),
                    }
                }
            }
            Err(e) => errors.push(e),
        }
    }
    let mut lookups: BTreeMap<u64, Option<String>> = BTreeMap::new();
    let mut deletes = 0usize;
    for s in &seen {
        if s.scope == "pr" && s.flavor == mine_flavor && s.run == mine_run {
            continue;
        }
        let sha = match &s.sha {
            Some(sha) => Some(sha.clone()),
            None => {
                if !lookups.contains_key(&s.run) && lookups.len() < MAX_LOOKUPS {
                    let found = send(env, "GET", &format!("{}/repos/{}/actions/runs/{}", env.api, env.repo, s.run), None).ok().and_then(|r| json_of(r, "looking up the run").ok()).and_then(|v| v["head_sha"].as_str().map(str::to_string));
                    lookups.insert(s.run, found);
                }
                lookups.get(&s.run).cloned().flatten()
            }
        };
        let keep = |why: &str, kept: &mut Vec<Value>| kept.push(json!({ "id": s.id, "why": why }));
        let Some(sha) = sha else {
            keep("its run could not be resolved", &mut kept);
            continue;
        };
        let ours = s.flavor == mine_flavor;
        let why = if sha != head {
            Some("an older head of the pull request")
        } else if sha == head && ours && s.scope == "pr" && s.run < mine_run {
            Some("an older run of the same workflow at the same head")
        } else {
            None
        };
        let Some(why) = why else {
            keep("the current head", &mut kept);
            continue;
        };
        if deletes >= MAX_DELETES {
            errors.push("cap reached".to_string());
            break;
        }
        deletes += 1;
        if !dry {
            let path = if s.scope == "pr" { "issues/comments" } else { "comments" };
            let reply = send(env, "DELETE", &format!("{}/repos/{}/{path}/{}", env.api, env.repo, s.id), None);
            if let Err(e) | Ok(Err(e)) = reply.map(|r| if r.ok() || r.status == 404 { Ok(()) } else { Err(format!("DELETE {}: HTTP {}", s.id, r.status)) }) {
                errors.push(e);
                continue;
            }
        }
        deleted.push(json!({ "id": s.id, "scope": s.scope, "flavor": s.flavor, "run": s.run, "sha": sha, "why": why }));
    }
    json!({ "ok": true, "dryRun": dry, "head": head, "deleted": deleted, "kept": kept, "errors": errors.iter().map(|e| redact(e, &env.token)).collect::<Vec<_>>() })
}

/// `wf ci prune`: always exits 0.
pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let out = Env::lenient().and_then(|env| {
        let found = open_pr(&env)?;
        let pr = match p.val("pr").and_then(|v| v.parse::<u64>().ok()) {
            Some(n) => n,
            None => found.as_ref().map(|f| f.0).ok_or("no open pull request for the branch; pass --pr")?,
        };
        let head = match found.as_ref().filter(|f| f.0 == pr).and_then(|f| f.1.clone()) {
            Some(h) => h,
            None => json_of(send(&env, "GET", &format!("{}/repos/{}/pulls/{pr}", env.api, env.repo), None)?, "reading the pull request")?["head"]["sha"].as_str().ok_or("the pull request has no head sha")?.to_string(),
        };
        Ok(prune_pr(&env, pr, &head, p.flag("commits"), p.flag("dry-run")))
    });
    let out = out.unwrap_or_else(|reason: String| json!({ "ok": false, "reason": redact(&reason, &var("GITHUB_TOKEN").unwrap_or_default()) }));
    println!("{out}");
    Some(0)
}
