//! The desktop's source control rows: review branch commits, ignored files and repository stars, answered through the agent API.
use super::queries::{field, QueryFailure};
use super::service::Service;
use super::vcs::{not_a_repo, outside_repository, review_outside};
use crate::ports::{AgentError, Op};
use serde_json::{json, Value};
use t3_contracts::time::iso;
use t3_contracts::wire;

/// The most commits one review lists, and the longest subject of one.
const COMMITS_MAX: usize = 100;
const SUBJECT_MAX: usize = 300;
const DEFAULT_BRANCHES: [&str; 2] = ["main", "master"];

pub fn handles(tag: &str) -> bool {
    matches!(tag, "review.getBranchCommits" | "projects.listIgnoredFiles" | "sourceControl.star" | "sourceControl.starred")
}

fn detection_error(cwd: &str, detail: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "VcsRepositoryDetectionError", "operation": "ReviewService.getBranchCommits", "cwd": cwd, "detail": detail}))
}

impl Service {
    pub fn desk_vcs(&self, tag: &str, payload: &Value) -> Result<Value, QueryFailure> {
        match tag {
            "review.getBranchCommits" => self.review_branch_commits(payload),
            "projects.listIgnoredFiles" => self.list_ignored_files(payload),
            "sourceControl.starred" => self.repository_star(payload, "sourceControl.starred"),
            _ => self.repository_star(payload, "sourceControl.star"),
        }
    }

    /// `review.getBranchCommits`: commits beyond the base (`baseRef`, else origin HEAD, else main or master); no base or no repository lists none.
    fn review_branch_commits(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        if outside_repository(root) {
            return Err(review_outside("getBranchCommits", "commit list", root));
        }
        let none = || Ok(json!({"commits": [], "truncated": false}));
        let upstream = match self.agent().run(&Op::GitUpstream { root: root.into() }) {
            Ok(upstream) => upstream,
            Err(e) if not_a_repo(&e) => return none(),
            Err(e) => return Err(detection_error(root, &e.message())),
        };
        let explicit = payload.get("baseRef").and_then(Value::as_str).filter(|b| !b.is_empty());
        let branch = upstream["branch"].as_str();
        let implicit: Vec<String> = upstream["origin_head"].as_str().map(str::to_string).into_iter().chain(DEFAULT_BRANCHES.iter().filter(|b| Some(**b) != branch).map(|b| b.to_string())).collect();
        let bases: Vec<String> = explicit.map_or(implicit, |b| vec![b.to_string()]);
        for base in bases {
            let limit = u32::try_from(COMMITS_MAX + 1).unwrap_or(u32::MAX);
            match self.agent().run(&Op::GitCompare { root: root.into(), base: base.clone(), head: "HEAD".into(), limit }) {
                Ok(compared) => return Ok(commits_of(&compared)),
                Err(AgentError::Refused { code, .. }) if code == "NOT_FOUND" && explicit.is_none() => continue,
                Err(e) if not_a_repo(&e) => return none(),
                Err(e) => return Err(detection_error(root, &e.message())),
            }
        }
        none()
    }

    /// `projects.listIgnoredFiles`: ignored files with sizes; an unlistable directory is the missing workspace, a git failure lists none.
    fn list_ignored_files(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        if outside_repository(root) || self.root_missing(root) {
            return Err(QueryFailure::Error(wire::workspace_root_missing("ProjectListEntriesError", "list", root, json!({}))));
        }
        match self.agent().run(&Op::GitIgnored { root: root.into() }) {
            Ok(found) => {
                let files: Vec<Value> = found["files"].as_array().into_iter().flatten().map(|f| json!({"path": f["path"], "byteLength": f["size"]})).collect();
                Ok(json!({"files": files, "truncated": found["truncated"].as_bool().unwrap_or(false)}))
            }
            Err(e) => {
                self.log("desk.ignored.failed", &[("error", e.message())]);
                Ok(json!({"files": [], "truncated": false}))
            }
        }
    }

    /// `sourceControl.starred` and `sourceControl.star`: whether the executor's forge account has starred the repository, and starring it. Only
    /// GitHub is starred; any other provider, and any refusal, is `false` (the pinned server's answer for a forge that cannot say).
    fn repository_star(&self, payload: &Value, method: &'static str) -> Result<Value, QueryFailure> {
        let (provider, host, repository) = (field(payload, "provider")?, field(payload, "host")?, field(payload, "repository")?);
        if provider != "github" {
            return Ok(json!(false));
        }
        let mut params = json!({"provider": provider, "repo": repository});
        if host != "github.com" {
            params["host"] = json!(host);
        }
        match self.agent().run(&Op::Hosted { method, root: String::new(), params }) {
            Ok(answer) => Ok(json!(answer["starred"].as_bool().unwrap_or(false))),
            Err(e) => {
                self.log("desk.star.refused", &[("method", method.to_string()), ("error", e.message())]);
                Ok(json!(false))
            }
        }
    }
}

fn commits_of(compared: &Value) -> Value {
    let rows: Vec<&Value> = compared["commits"].as_array().into_iter().flatten().collect();
    let commits: Vec<Value> = rows
        .iter()
        .take(COMMITS_MAX)
        .map(|c| json!({"sha": c["sha"], "subject": c["subject"].as_str().unwrap_or_default().chars().take(SUBJECT_MAX).collect::<String>(), "committedAt": iso(c["authored_ms"].as_i64().unwrap_or(0))}))
        .collect();
    json!({"commits": commits, "truncated": compared["truncated"].as_bool().unwrap_or(false) || rows.len() > COMMITS_MAX})
}
