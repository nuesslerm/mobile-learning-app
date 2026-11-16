//! `hostedReview.forBranch`, `github.prComments` and `github.prFileContents`: the review reads of the phone's pull request screen.
use crate::{github, number, refusal, text, time, with_repo, Target};
use agent_api_git::{BLOB, FETCH};
use agent_api_hosted::pr;
use base64::Engine;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentError;
use serde_json::{json, Value};

/// The most a side may weigh: what `gh api` buffers in the pinned host, and what rca's blob reads.
const MAX_BLOB: u64 = 8 << 20;
const PROBE: usize = 2048;
const STATUSES: &[&str] = &["added", "removed", "modified", "renamed", "copied", "changed", "unchanged"];

fn linked(params: &Value, key: &str) -> Option<u64> {
    params.get(key).and_then(Value::as_u64).filter(|n| *n > 0)
}

/// `hostedReview.forBranch`: the GitHub review of the branch in the provider-neutral shape (a linked number is read directly, the fallback only when
/// that read fails). A GitLab link is refused: no GitLab account is served yet (#84), so no answer could be read from the provider.
pub fn for_branch(t: &Target, params: &Value) -> Result<Value, Failure> {
    let branch = text(params, "branch").ok_or_else(|| Failure::invalid("Missing branch"))?.trim_start_matches("refs/heads/");
    let (exact, fallback) = (linked(params, "linkedGitHubPR"), linked(params, "fallbackGitHubPR").filter(|_| linked(params, "linkedGitHubPR").is_none()));
    if exact.is_none() && linked(params, "linkedGitLabMR").is_some() {
        return Err(Failure::new(RUNTIME_ERROR, "GitLab reviews are not offered by this host"));
    }
    let found = match (exact, fallback) {
        (Some(n), _) => Some(github::summary(t, n)?),
        (None, Some(n)) => match github::summary(t, n) {
            Ok(info) => Some(info),
            Err(_) => by_branch(t, branch)?,
        },
        (None, None) => by_branch(t, branch)?,
    };
    let review = found.map_or(Value::Null, |info| {
        let mut out = json!({"provider": "github"});
        for (key, value) in info.as_object().into_iter().flatten().filter(|(k, _)| *k != "headRefName") {
            out[if key == "checksStatus" { "status" } else { key }] = value.clone();
        }
        out
    });
    Ok(if params["reportReadOutcome"] == true { json!({"review": review, "readOutcome": "provider"}) } else { review })
}

fn by_branch(t: &Target, branch: &str) -> Result<Option<Value>, Failure> {
    if branch.is_empty() {
        return Ok(None);
    }
    github::find_for_branch(t, branch)?.map(|n| github::summary(t, n)).transpose()
}

fn when(ms: &Value) -> String {
    ms.as_u64().map(time::iso).unwrap_or_default()
}

/// A comment's number in the host's sense (the database id): rca's thread comments carry it, a conversation comment's id is a node id and its number is
/// the end of its address (`#issuecomment-<n>`). A comment with neither is left out, as the host leaves out one with no database id.
fn numeric_id(c: &Value) -> Option<u64> {
    c["id"].as_u64().or_else(|| c["id"].as_str().and_then(|s| s.parse().ok())).or_else(|| c["url"].as_str().and_then(|u| u.rsplit_once('-')).and_then(|(_, n)| n.parse().ok()))
}

fn comment_row(c: &Value) -> Option<(u64, Value)> {
    let id = numeric_id(c)?;
    let at = c["created_at_ms"].as_u64().unwrap_or(0);
    Some((at, json!({"id": id, "author": c["author"].as_str().unwrap_or("ghost"), "authorAvatarUrl": "", "body": c["body"].as_str().unwrap_or(""), "createdAt": when(&c["created_at_ms"]), "url": c["url"].as_str().unwrap_or("")})))
}

/// `github.prComments`: the conversation comments and the review thread comments of the pull request, oldest first. rca reports no avatar, reaction, bot mark
/// or diff hunk, and no review summary with a number, so those parts of the host's answer are absent.
pub fn pr_comments(t: &Target, params: &Value) -> Result<Value, Failure> {
    let args = with_repo(json!({"number": number(params, "prNumber", "PR")?}), params, "prRepo")?;
    let conversation = t.ask(pr::COMMENTS, args.clone())?;
    let threads = t.ask(pr::THREADS, args)?;
    let mut rows: Vec<(u64, Value)> = conversation["comments"].as_array().ok_or_else(|| Failure::new(RUNTIME_ERROR, "rca's pr.comments answered no comments list"))?.iter().filter_map(comment_row).collect();
    for thread in threads["threads"].as_array().ok_or_else(|| Failure::new(RUNTIME_ERROR, "rca's pr.threads answered no threads list"))? {
        let Some(id) = thread["id"].as_str() else { continue };
        for (at, mut row) in thread["comments"].as_array().into_iter().flatten().filter_map(comment_row) {
            row["path"] = thread["path"].clone();
            row["threadId"] = json!(id);
            row["isResolved"] = json!(thread["resolved"] == true);
            row["isOutdated"] = json!(thread["outdated"] == true);
            if let Some(line) = thread["line"].as_u64().filter(|l| *l > 0) {
                row["line"] = json!(line);
            }
            rows.push((at, row));
        }
    }
    rows.sort_by_key(|(at, _)| *at);
    Ok(Value::Array(rows.into_iter().map(|(_, row)| row).collect()))
}

struct Side {
    content: String,
    binary: bool,
    too_large: bool,
}

fn empty() -> Side {
    Side { content: String::new(), binary: false, too_large: false }
}

/// The file at `rev` of the fetched pull request: one the commit lacks, or any read the executor refuses, is an empty side as in the host.
fn read_side(t: &Target, rev: &str, path: &str) -> Result<Side, Failure> {
    let blob = match t.try_ask(BLOB, json!({"rev": rev, "path": path, "max_bytes": MAX_BLOB})) {
        Ok(blob) => blob,
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => return Ok(empty()),
        Err(other) => return Err(refusal(other)),
    };
    if blob["truncated"] == true || blob["size"].as_u64().unwrap_or(0) > MAX_BLOB {
        return Ok(Side { too_large: true, ..empty() });
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(blob["data"].as_str().unwrap_or("")).unwrap_or_default();
    if bytes.iter().take(PROBE).any(|b| *b == 0) {
        return Ok(Side { binary: true, ..empty() });
    }
    Ok(Side { content: String::from_utf8_lossy(&bytes).into_owned(), ..empty() })
}

/// `github.prFileContents`: a changed file before and after, read at the base and head commits once the pull request's head and target branch are
/// fetched into refs of their own (those of `worktree.resolvePrBase`). A pull request that cannot be read or fetched answers empty sides, as the host does.
pub fn file_contents(t: &Target, params: &Value) -> Result<Value, Failure> {
    let n = number(params, "prNumber", "PR")?;
    let path = text(params, "path").ok_or_else(|| Failure::invalid("Missing file path"))?;
    let status = text(params, "status").filter(|s| STATUSES.contains(s)).ok_or_else(|| Failure::invalid("Invalid option: expected one of \"added\"|\"removed\"|\"modified\"|\"renamed\"|\"copied\"|\"changed\"|\"unchanged\""))?;
    let head = text(params, "headSha").ok_or_else(|| Failure::invalid("Missing head SHA"))?;
    let base = text(params, "baseSha").ok_or_else(|| Failure::invalid("Missing base SHA"))?;
    let old_path = text(params, "oldPath").unwrap_or(path);
    let nothing = || json!({"original": "", "modified": "", "originalIsBinary": false, "modifiedIsBinary": false});
    let view = match t.try_ask(pr::VIEW, with_repo(json!({"number": n}), params, "prRepo")?) {
        Ok(view) => view,
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" && code != "INVALID_ARGS" => return Ok(nothing()),
        Err(other) => return Err(refusal(other)),
    };
    let Some(base_ref) = view["pr"]["base_ref"].as_str().filter(|b| !b.is_empty()) else { return Ok(nothing()) };
    let refspecs = [format!("+refs/pull/{n}/head:refs/rca/pr/{n}/head"), format!("+refs/heads/{base_ref}:refs/rca/pr/{n}/base")];
    match t.try_ask(FETCH, json!({"refspecs": refspecs})) {
        Ok(_) => {}
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => return Ok(nothing()),
        Err(other) => return Err(refusal(other)),
    }
    let original = if status == "added" { empty() } else { read_side(t, base, old_path)? };
    let modified = if status == "removed" { empty() } else { read_side(t, head, path)? };
    let mut out = json!({"original": original.content, "modified": modified.content, "originalIsBinary": original.binary, "modifiedIsBinary": modified.binary});
    for (key, side) in [("originalTooLarge", &original), ("modifiedTooLarge", &modified)] {
        if side.too_large {
            out[key] = json!(true);
        }
    }
    Ok(out)
}
