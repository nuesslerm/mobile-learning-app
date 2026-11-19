//! The programs a validated hosted request starts: for each call the exact argv of `gh` or `glab` (or the `git push` a GitLab publish ends
//! with, or the local `git remote -v`), the text sent on stdin and the exit codes that still carry an answer. Pure: nothing is run here. Every value reaches the CLI as the
//! value of a `--flag=value` or a `-f key=value` field, or as a path, an id or a number the contract already checked, so none can be an option.
use super::shape::Output;
use exec_hosted::{CommentKind, HostedCall, HostedOp, MergeMethod, PrState, Provider, ReactTarget, Verdict, Visibility};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Gh,
    Glab,
    Git,
}

impl Program {
    pub fn name(self) -> &'static str {
        match self {
            Program::Gh => "gh",
            Program::Glab => "glab",
            Program::Git => "git",
        }
    }
}

/// Where a step runs: the workspace root, or a directory the adapter confined.
#[derive(Debug, Clone, PartialEq)]
pub enum Cwd {
    Root,
    Dir(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub program: Program,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    /// Exit codes whose output is an answer: a CLI that exits 8 for checks still pending.
    pub ok_exits: Vec<i32>,
    pub cwd: Cwd,
}

/// The paths the adapter confined before planning.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    pub dest: Option<PathBuf>,
    pub dir: Option<PathBuf>,
}

pub const PR_FIELDS: &str = "number,title,body,state,isDraft,author,headRefName,baseRefName,headRefOid,labels,createdAt,updatedAt,mergedAt,closedAt,url,reviewDecision,mergeable,additions,deletions,changedFiles,reviewRequests,assignees";
pub const PR_LIST_FIELDS: &str = "number,title,state,isDraft,author,headRefName,baseRefName,labels,createdAt,updatedAt,url,reviewDecision,reviewRequests";
pub const RUN_FIELDS: &str = "databaseId,displayTitle,name,workflowName,status,conclusion,headBranch,headSha,event,createdAt,updatedAt,url";
pub const REPO_FIELDS: &str = "name,nameWithOwner,description,url,sshUrl,isPrivate,defaultBranchRef,viewerPermission,isFork";
const COMMENT_FIELDS: &str = "id databaseId author{__typename login avatarUrl(size:48)} body createdAt url viewerDidAuthor reactionGroups{content viewerHasReacted reactors{totalCount}}";
const QUERY_HEAD: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){";
const QUERY_TAIL: &str = "}}}";
const PAGE_INFO: &str = "pageInfo{hasNextPage endCursor}";

pub fn comments_query() -> String {
    format!("{QUERY_HEAD}comments(first:100,after:$cursor){{nodes{{{COMMENT_FIELDS}}}{PAGE_INFO}}}{QUERY_TAIL}")
}

pub fn reviews_query() -> String {
    format!("{QUERY_HEAD}reviews(first:100,after:$cursor){{nodes{{{COMMENT_FIELDS} state submittedAt}}{PAGE_INFO}}}{QUERY_TAIL}")
}

pub fn threads_query() -> String {
    format!("{QUERY_HEAD}reviewThreads(first:100,after:$cursor){{nodes{{id isResolved isOutdated path line startLine originalLine originalStartLine diffSide rootHunk:comments(first:1){{nodes{{diffHunk}}}} comments(first:100){{nodes{{{COMMENT_FIELDS}}}{PAGE_INFO}}}}}{PAGE_INFO}}}{QUERY_TAIL}")
}

/// The comments of one thread past the first hundred the threads query returned for it.
pub fn thread_replies_query() -> String {
    format!("query($thread:ID!,$cursor:String){{node(id:$thread){{... on PullRequestReviewThread{{id comments(first:100,after:$cursor){{nodes{{{COMMENT_FIELDS}}}{PAGE_INFO}}}}}}}}}")
}
pub const REPLY_MUTATION: &str = "mutation($thread:ID!,$body:String!){addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$thread,body:$body}){comment{id url}}}";
pub const RESOLVE_MUTATION: &str = "mutation($thread:ID!){resolveReviewThread(input:{threadId:$thread}){thread{id isResolved}}}";
pub const UNRESOLVE_MUTATION: &str = "mutation($thread:ID!){unresolveReviewThread(input:{threadId:$thread}){thread{id isResolved}}}";
pub const MARK_VIEWED_MUTATION: &str = "mutation($pullRequestId:ID!,$path:String!){markFileAsViewed(input:{pullRequestId:$pullRequestId,path:$path}){pullRequest{id}}}";
pub const UNMARK_VIEWED_MUTATION: &str = "mutation($pullRequestId:ID!,$path:String!){unmarkFileAsViewed(input:{pullRequestId:$pullRequestId,path:$path}){pullRequest{id}}}";
pub const EDIT_ISSUE_COMMENT_MUTATION: &str = "mutation($id:ID!,$body:String!){updateIssueComment(input:{id:$id,body:$body}){issueComment{id}}}";
pub const EDIT_REVIEW_COMMENT_MUTATION: &str = "mutation($id:ID!,$body:String!){updatePullRequestReviewComment(input:{pullRequestReviewCommentId:$id,body:$body}){pullRequestReviewComment{id}}}";

fn step(program: Program, args: Vec<String>) -> Step {
    Step { program, args, stdin: None, ok_exits: vec![0], cwd: Cwd::Root }
}

fn repo_graphql(op: &HostedOp, number: u64, cursor: Option<&str>, query: String) -> Step {
    let mut args = strs(&["api", "graphql"]);
    match op.repo.as_deref().and_then(|r| r.split_once('/')) {
        Some((owner, name)) => args.extend([format!("-f=owner={owner}"), format!("-f=name={name}")]),
        None => args.extend(strs(&["-F=owner={owner}", "-F=name={repo}"])),
    }
    args.push(format!("-F=number={number}"));
    args.extend(cursor.map(|c| format!("-f=cursor={c}")));
    args.push(format!("-f=query={query}"));
    step(Program::Gh, args)
}

fn thread_replies(thread: &str, cursor: &str) -> Step {
    step(Program::Gh, vec!["api".into(), "graphql".into(), format!("-f=thread={thread}"), format!("-f=cursor={cursor}"), format!("-f=query={}", thread_replies_query())])
}

/// Whether a connection has another page and where it ends.
fn page_of(connection: &Value) -> (bool, Option<String>) {
    let info = &connection["pageInfo"];
    (info["hasNextPage"].as_bool().unwrap_or(false), info["endCursor"].as_str().map(str::to_string))
}

/// The cursor of the next page of a connection read with `sent` (none for the first page), none when it was the last. A page that ends where the
/// request began would be read forever, so it is refused.
fn advance(connection: &Value, sent: Option<&str>, what: &str) -> Result<Option<String>, String> {
    match page_of(connection) {
        (false, _) => Ok(None),
        (true, Some(end)) if Some(end.as_str()) != sent => Ok(Some(end)),
        (true, _) => Err(format!("gh answered a page of the {what} whose cursor did not advance, so the read stops rather than answer a partial or endless conversation")),
    }
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Percent-encodes everything but the unreserved characters, so a value is one query value or one path segment.
pub fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn json_header() -> String {
    "--header=Content-Type: application/json".to_string()
}

fn numeric(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}

/// GitLab's name for a reaction the contract spells the GitHub way.
fn award_name(content: &str) -> &str {
    match content {
        "+1" => "thumbsup",
        "-1" => "thumbsdown",
        "laugh" => "laughing",
        "hooray" => "tada",
        other => other,
    }
}

/// The REST collection a reaction is added to or listed from: the pull request's, or a comment's.
fn reaction_collection(op: &HostedOp, number: u64, target: ReactTarget, comment_id: Option<u64>) -> Result<String, String> {
    match op.provider {
        Provider::Github => {
            let repo = op.repo.as_ref().map_or("repos/{owner}/{repo}".to_string(), |r| format!("repos/{r}"));
            match (target, comment_id) {
                (ReactTarget::Pr, _) => Ok(format!("{repo}/issues/{number}/reactions")),
                (ReactTarget::Comment, Some(id)) => Ok(format!("{repo}/issues/comments/{id}/reactions")),
                (ReactTarget::ReviewComment, Some(id)) => Ok(format!("{repo}/pulls/comments/{id}/reactions")),
                _ => Err("a comment reaction names the comment".into()),
            }
        }
        Provider::Gitlab => {
            let project = op.repo.as_ref().map_or("projects/:fullpath".to_string(), |r| format!("projects/{}", encode(r)));
            match (target, comment_id) {
                (ReactTarget::Pr, _) => Ok(format!("{project}/merge_requests/{number}/award_emoji")),
                (_, Some(id)) => Ok(format!("{project}/merge_requests/{number}/notes/{id}/award_emoji")),
                _ => Err("a comment reaction names the comment".into()),
            }
        }
    }
}

/// A comment's new text as the request body of `gh api` or `glab api`: on stdin, as JSON.
fn json_body(mut s: Step, body: &Value) -> Step {
    s.args.push("--input=-".into());
    s.args.push(json_header());
    s.stdin = Some(body.to_string());
    s
}

/// The step that follows the steps run so far, when what it asks depends on what they printed: the stack's own read after the list that found
/// it, and a reaction taken back (the signed-in account, then its reaction in the list, then the delete by that reaction's id).
pub fn follow(op: &HostedOp, outputs: &[Output]) -> Result<Option<Step>, String> {
    let parse = |out: &Output| serde_json::from_str::<Value>(&out.stdout).map_err(|e| format!("{} printed no JSON ({e})", op.provider.program()));
    match (&op.call, op.provider, outputs.len()) {
        (HostedCall::PrStack { .. }, Provider::Github, 1) => {
            let listed = parse(&outputs[0])?;
            let Some(number) = listed.as_array().and_then(|a| a.first()).and_then(|s| s["number"].as_u64()) else { return Ok(None) };
            let repo = op.repo.as_ref().map_or("repos/{owner}/{repo}".to_string(), |r| format!("repos/{r}"));
            Ok(Some(step(Program::Gh, vec!["api".into(), format!("{repo}/stacks/{number}")])))
        }
        (HostedCall::PrComments { number }, Provider::Github, n) if n >= 1 => next_page(op, *number, outputs, "comments", comments_query),
        (HostedCall::PrReviews { number }, Provider::Github, n) if n >= 1 => next_page(op, *number, outputs, "reviews", reviews_query),
        (HostedCall::PrThreads { number }, Provider::Github, n) if n >= 1 => next_thread_step(op, *number, outputs),
        (HostedCall::PrReact { number, target, comment_id, content, undo: true }, provider, 1 | 2) => {
            let collection = reaction_collection(op, *number, *target, *comment_id)?;
            let (login_key, program) = if provider == Provider::Github { ("login", Program::Gh) } else { ("username", Program::Glab) };
            let viewer = parse(&outputs[0])?.get(login_key).and_then(Value::as_str).map(str::to_lowercase).ok_or("the CLI named no signed-in account")?;
            if outputs.len() == 1 {
                let query = if provider == Provider::Github { format!("?content={}&per_page=100", encode(content)) } else { "?per_page=100".to_string() };
                return Ok(Some(step(program, vec!["api".into(), format!("{collection}{query}")])));
            }
            let wanted = if provider == Provider::Github { content.as_str() } else { award_name(content) };
            let mine = parse(&outputs[1])?.as_array().and_then(|list| {
                list.iter().find(|r| {
                    let who = r["user"].get(login_key).and_then(Value::as_str).map(str::to_lowercase);
                    let what = r.get(if provider == Provider::Github { "content" } else { "name" }).and_then(Value::as_str);
                    who.as_deref() == Some(viewer.as_str()) && what == Some(wanted)
                })
            }).and_then(|r| r["id"].as_u64());
            Ok(mine.map(|id| step(program, vec!["api".into(), "--method=DELETE".into(), format!("{collection}/{id}")])))
        }
        _ => Ok(None),
    }
}

fn page_json(op: &HostedOp, out: &Output) -> Result<Value, String> {
    serde_json::from_str::<Value>(&out.stdout).map_err(|e| format!("{} printed no JSON ({e})", op.provider.program()))
}

fn next_page(op: &HostedOp, number: u64, outputs: &[Output], key: &str, query: fn() -> String) -> Result<Option<Step>, String> {
    let connection = |out: &Output| page_json(op, out).map(|v| v["data"]["repository"]["pullRequest"][key].clone());
    let sent = match outputs.len() {
        1 => None,
        n => page_of(&connection(&outputs[n - 2])?).1,
    };
    let next = advance(&connection(&outputs[outputs.len() - 1])?, sent.as_deref(), key)?;
    Ok(next.map(|c| repo_graphql(op, number, Some(&c), query())))
}

/// The threads pages first, then the comments past the first hundred of each thread that has more, one thread after the other.
fn next_thread_step(op: &HostedOp, number: u64, outputs: &[Output]) -> Result<Option<Step>, String> {
    let mut sent: Option<String> = None;
    let mut pages = Vec::new();
    let mut done = false;
    for out in outputs {
        let connection = page_json(op, out)?["data"]["repository"]["pullRequest"]["reviewThreads"].clone();
        let next = advance(&connection, sent.as_deref(), "threads")?;
        pages.push(connection);
        match next {
            Some(cursor) => sent = Some(cursor),
            None => {
                done = true;
                break;
            }
        }
    }
    if !done {
        return Ok(sent.map(|c| repo_graphql(op, number, Some(&c), threads_query())));
    }
    let mut at = pages.len();
    for page in &pages {
        for thread in page["nodes"].as_array().map_or(&[][..], Vec::as_slice) {
            let (more, first) = page_of(&thread["comments"]);
            let (Some(id), true) = (thread["id"].as_str(), more) else { continue };
            let mut cursor = first.ok_or("gh answered a thread with more comments and no cursor")?;
            loop {
                let Some(out) = outputs.get(at) else { return Ok(Some(thread_replies(id, &cursor))) };
                at += 1;
                match advance(&page_json(op, out)?["data"]["node"]["comments"], Some(&cursor), "comments of a thread")? {
                    Some(next) => cursor = next,
                    None => break,
                }
            }
        }
    }
    Ok(None)
}

/// The variables a step adds to the CLI's environment: the instance it talks to.
pub fn env_of(op: &HostedOp) -> Vec<(String, String)> {
    match (&op.host, op.provider) {
        (Some(h), Provider::Github) => vec![("GH_HOST".into(), h.clone())],
        (Some(h), Provider::Gitlab) => vec![("GITLAB_HOST".into(), h.clone())],
        (None, _) => Vec::new(),
    }
}

/// The steps of `op` in order; the answer is shaped from their outputs. A refusal says why this provider cannot do the call.
pub fn plan(op: &HostedOp, paths: &Paths) -> Result<Vec<Step>, String> {
    match op.provider {
        Provider::Github => github(op, paths),
        Provider::Gitlab => gitlab(op, paths),
    }
}

fn github(op: &HostedOp, paths: &Paths) -> Result<Vec<Step>, String> {
    let gh = |args: Vec<String>| step(Program::Gh, args);
    let repo_flag = |args: &mut Vec<String>| {
        if let Some(r) = &op.repo {
            args.push(format!("--repo={r}"));
        }
    };
    let pr = |verb: &[&str], number: u64, extra: Vec<String>| {
        let mut args = strs(&["pr"]);
        args.extend(verb.iter().map(|s| s.to_string()));
        args.push(number.to_string());
        repo_flag(&mut args);
        args.extend(extra);
        args
    };
    let view = |number: u64, fields: &str| gh(pr(&["view"], number, vec![format!("--json={fields}")]));
    let graphql_read = |number: u64, query: String| repo_graphql(op, number, None, query);
    let rest_repo = || match &op.repo {
        Some(r) => format!("repos/{r}"),
        None => "repos/{owner}/{repo}".to_string(),
    };
    let one = |s: Step| Ok(vec![s]);
    match &op.call {
        HostedCall::RepoView => {
            let mut args = strs(&["repo", "view"]);
            args.extend(op.repo.clone());
            args.push(format!("--json={REPO_FIELDS}"));
            one(gh(args))
        }
        HostedCall::RepoRemotes => one(step(Program::Git, strs(&["remote", "-v"]))),
        HostedCall::RepoLabels { limit } => {
            let mut args = strs(&["label", "list"]);
            repo_flag(&mut args);
            args.extend([format!("--limit={limit}"), "--json=name,color,description".into()]);
            one(gh(args))
        }
        HostedCall::RepoReviewers { limit } => one(gh(vec!["api".into(), format!("{}/assignees?per_page={limit}", rest_repo())])),
        HostedCall::RepoClone { .. } => {
            let dest = paths.dest.as_ref().ok_or("no destination")?;
            one(gh(vec!["repo".into(), "clone".into(), op.repo.clone().unwrap_or_default(), dest.to_string_lossy().into_owned()]))
        }
        HostedCall::RepoPublish { visibility, description, .. } => {
            let dir = paths.dir.as_ref().ok_or("no directory")?;
            let mut args = vec!["repo".into(), "create".into(), op.repo.clone().unwrap_or_default(), if *visibility == Visibility::Public { "--public".into() } else { "--private".into() }];
            args.extend(description.iter().map(|d| format!("--description={d}")));
            args.extend([format!("--source={}", dir.display()), "--remote=origin".into(), "--push".into()]);
            one(gh(args))
        }
        HostedCall::PrList { state, author, label, base, head, search, limit } => {
            let mut args = strs(&["pr", "list"]);
            repo_flag(&mut args);
            let state = match state {
                PrState::Open => "open",
                PrState::Closed => "closed",
                PrState::Merged => "merged",
                PrState::All => "all",
            };
            args.extend([format!("--state={state}"), format!("--limit={limit}"), format!("--json={PR_LIST_FIELDS}")]);
            for (flag, value) in [("author", author), ("label", label), ("base", base), ("head", head), ("search", search)] {
                args.extend(value.iter().map(|v| format!("--{flag}={v}")));
            }
            one(gh(args))
        }
        HostedCall::PrView { number } => one(view(*number, PR_FIELDS)),
        HostedCall::PrDiff { number } => one(gh(pr(&["diff"], *number, strs(&["--color=never"])))),
        HostedCall::PrChecks { number } => {
            let mut s = gh(pr(&["checks"], *number, strs(&["--json=name,state,bucket,workflow,link,startedAt,completedAt"])));
            s.ok_exits = vec![0, 1, 8];
            one(s)
        }
        HostedCall::PrComments { number } => one(graphql_read(*number, comments_query())),
        HostedCall::PrReviews { number } => one(graphql_read(*number, reviews_query())),
        HostedCall::PrFiles { number } => one(view(*number, "files")),
        HostedCall::PrThreads { number } => one(graphql_read(*number, threads_query())),
        HostedCall::PrCreate { title, body, head, base, draft, labels, reviewers } => {
            let mut args = strs(&["pr", "create"]);
            repo_flag(&mut args);
            args.extend([format!("--title={title}"), format!("--head={head}")]);
            args.extend(base.iter().map(|b| format!("--base={b}")));
            args.extend(labels.iter().map(|l| format!("--label={l}")));
            args.extend(reviewers.iter().map(|r| format!("--reviewer={r}")));
            if *draft {
                args.push("--draft".into());
            }
            let mut s = gh(args);
            match body {
                Some(b) => {
                    s.args.push("--body-file=-".into());
                    s.stdin = Some(b.clone());
                }
                None => s.args.push("--body=".into()),
            }
            one(s)
        }
        HostedCall::PrEdit { number, title, body, base } => {
            let mut extra: Vec<String> = Vec::new();
            extra.extend(title.iter().map(|t| format!("--title={t}")));
            extra.extend(base.iter().map(|b| format!("--base={b}")));
            let mut s = gh(pr(&["edit"], *number, extra));
            if let Some(b) = body {
                s.args.push("--body-file=-".into());
                s.stdin = Some(b.clone());
            }
            one(s)
        }
        HostedCall::PrReady { number, undo } => one(gh(pr(&["ready"], *number, if *undo { strs(&["--undo"]) } else { vec![] }))),
        HostedCall::PrState { number, closed } => one(gh(pr(&[if *closed { "close" } else { "reopen" }], *number, vec![]))),
        HostedCall::PrComment { number, body } => {
            let mut s = gh(pr(&["comment"], *number, strs(&["--body-file=-"])));
            s.stdin = Some(body.clone());
            one(s)
        }
        HostedCall::PrReply { number, comment_id: Some(id), body, .. } => one(json_body(gh(vec!["api".into(), "--method=POST".into(), format!("{}/pulls/{number}/comments/{id}/replies", rest_repo())]), &json!({"body": body}))),
        HostedCall::PrReviewComment { number, body, commit_id, path, line, start_line, .. } => {
            let mut v = json!({"body": body, "commit_id": commit_id, "path": path, "line": line, "side": "RIGHT"});
            if let Some(start) = start_line {
                v["start_line"] = json!(start);
                v["start_side"] = json!("RIGHT");
            }
            one(json_body(gh(vec!["api".into(), "--method=POST".into(), format!("{}/pulls/{number}/comments", rest_repo())]), &v))
        }
        HostedCall::PrReply { thread, body, .. } => {
            let thread = thread.as_deref().unwrap_or_default();
            let args = vec!["api".into(), "graphql".into(), format!("-f=thread={thread}"), format!("-f=body={body}"), format!("-f=query={REPLY_MUTATION}")];
            one(gh(args))
        }
        HostedCall::PrReview { number, verdict, body } => {
            let flag = match verdict {
                Verdict::Approve => "--approve",
                Verdict::RequestChanges => "--request-changes",
                Verdict::Comment => "--comment",
            };
            let mut s = gh(pr(&["review"], *number, strs(&[flag])));
            if let Some(b) = body {
                s.args.push("--body-file=-".into());
                s.stdin = Some(b.clone());
            }
            one(s)
        }
        HostedCall::PrRequestReviewers { number, add, remove } => {
            let extra = add.iter().map(|a| format!("--add-reviewer={a}")).chain(remove.iter().map(|r| format!("--remove-reviewer={r}"))).collect();
            one(gh(pr(&["edit"], *number, extra)))
        }
        HostedCall::PrLabels { number, add, remove } => {
            let extra = add.iter().map(|a| format!("--add-label={a}")).chain(remove.iter().map(|r| format!("--remove-label={r}"))).collect();
            one(gh(pr(&["edit"], *number, extra)))
        }
        HostedCall::PrReact { number, target, comment_id, content, undo } => {
            if *undo {
                return one(gh(strs(&["api", "user"])));
            }
            let path = reaction_collection(op, *number, *target, *comment_id)?;
            one(gh(vec!["api".into(), "--method=POST".into(), path, format!("-f=content={content}")]))
        }
        HostedCall::PrResolveThread { thread, resolved, .. } => {
            let query = if *resolved { RESOLVE_MUTATION } else { UNRESOLVE_MUTATION };
            one(gh(vec!["api".into(), "graphql".into(), format!("-f=thread={thread}"), format!("-f=query={query}")]))
        }
        HostedCall::PrFileViewed { pull_request, path, viewed } => {
            let query = if *viewed { MARK_VIEWED_MUTATION } else { UNMARK_VIEWED_MUTATION };
            one(json_body(gh(strs(&["api", "graphql"])), &json!({"query": query, "variables": {"pullRequestId": pull_request, "path": path}})))
        }
        HostedCall::PrUpdateBranch { number, rebase } => one(gh(pr(&["update-branch"], *number, if *rebase { strs(&["--rebase"]) } else { vec![] }))),
        HostedCall::PrMerge { number, method, auto, delete_branch, match_head_commit } => {
            let mut extra = vec![match method {
                MergeMethod::Merge => "--merge".to_string(),
                MergeMethod::Squash => "--squash".to_string(),
                MergeMethod::Rebase => "--rebase".to_string(),
            }];
            if *auto {
                extra.push("--auto".into());
            }
            if *delete_branch {
                extra.push("--delete-branch".into());
            }
            extra.extend(match_head_commit.iter().map(|s| format!("--match-head-commit={s}")));
            one(gh(pr(&["merge"], *number, extra)))
        }
        HostedCall::PrStack { number } => one(gh(vec!["api".into(), format!("{}/stacks?pull_request={number}", rest_repo())])),
        HostedCall::PrCommentEdit { comment, kind, body, .. } => {
            let by_number = numeric(comment);
            match (kind, by_number) {
                (CommentKind::Issue, false) => one(json_body(gh(strs(&["api", "graphql"])), &json!({"query": EDIT_ISSUE_COMMENT_MUTATION, "variables": {"id": comment, "body": body}}))),
                (CommentKind::Review, false) => one(json_body(gh(strs(&["api", "graphql"])), &json!({"query": EDIT_REVIEW_COMMENT_MUTATION, "variables": {"id": comment, "body": body}}))),
                (CommentKind::Issue, true) => one(json_body(gh(vec!["api".into(), "--method=PATCH".into(), format!("{}/issues/comments/{comment}", rest_repo())]), &json!({"body": body}))),
                (CommentKind::Review, true) => one(json_body(gh(vec!["api".into(), "--method=PATCH".into(), format!("{}/pulls/comments/{comment}", rest_repo())]), &json!({"body": body}))),
            }
        }
        HostedCall::RunList { branch, limit } => {
            let mut args = strs(&["run", "list"]);
            repo_flag(&mut args);
            args.extend([format!("--limit={limit}"), format!("--json={RUN_FIELDS}")]);
            args.extend(branch.iter().map(|b| format!("--branch={b}")));
            one(gh(args))
        }
        HostedCall::RunView { run } => {
            let mut args = strs(&["run", "view", &run.to_string()]);
            repo_flag(&mut args);
            args.push(format!("--json={RUN_FIELDS},jobs"));
            one(gh(args))
        }
        HostedCall::AuthStatus => {
            let mut s = gh(strs(&["auth", "status"]));
            s.args.extend(op.host.iter().map(|h| format!("--hostname={h}")));
            s.ok_exits = vec![0, 1];
            one(s)
        }
        HostedCall::LoginStart => Ok(vec![gh(login_args(op))]),
        HostedCall::LoginWait { .. } | HostedCall::LoginCancel { .. } => Err("a sign-in flow is not a program of its own".into()),
    }
}

/// The CLI's web sign-in: `--web` shows a one-time code and an address and waits for the approval.
pub fn login_args(op: &HostedOp) -> Vec<String> {
    let host = op.host.clone().unwrap_or_else(|| op.provider.default_host().to_string());
    match op.provider {
        Provider::Github => vec!["auth".into(), "login".into(), "--web".into(), format!("--hostname={host}"), "--git-protocol=https".into()],
        Provider::Gitlab => vec!["auth".into(), "login".into(), "--web".into(), format!("--hostname={host}")],
    }
}

fn gitlab(op: &HostedOp, paths: &Paths) -> Result<Vec<Step>, String> {
    let project = || match &op.repo {
        Some(r) => format!("projects/{}", encode(r)),
        None => "projects/:fullpath".to_string(),
    };
    let api = |method: &str, path: String| {
        let mut args = strs(&["api"]);
        if method != "GET" {
            args.push(format!("--method={method}"));
        }
        args.push(path);
        step(Program::Glab, args)
    };
    let with_json = |mut s: Step, body: serde_json::Value| {
        s.args.push("--input=-".into());
        s.args.push("--header=Content-Type: application/json".into());
        s.stdin = Some(body.to_string());
        s
    };
    let mr = |n: u64, tail: &str| format!("{}/merge_requests/{n}{tail}", project());
    let glab = |args: Vec<String>| step(Program::Glab, args);
    let repo_flag = |args: &mut Vec<String>| {
        if let Some(r) = &op.repo {
            args.push(format!("--repo={r}"));
        }
    };
    let mr_cmd = |verb: &[&str], n: u64, extra: Vec<String>| {
        let mut args = strs(&["mr"]);
        args.extend(verb.iter().map(|s| s.to_string()));
        args.push(n.to_string());
        repo_flag(&mut args);
        args.extend(extra);
        args
    };
    let one = |s: Step| Ok(vec![s]);
    match &op.call {
        HostedCall::RepoView => one(api("GET", project())),
        HostedCall::RepoRemotes => one(step(Program::Git, strs(&["remote", "-v"]))),
        HostedCall::RepoLabels { limit } => one(api("GET", format!("{}/labels?per_page={limit}", project()))),
        HostedCall::RepoReviewers { limit } => one(api("GET", format!("{}/members/all?per_page={limit}", project()))),
        HostedCall::RepoClone { .. } => {
            let dest = paths.dest.as_ref().ok_or("no destination")?;
            one(glab(vec!["repo".into(), "clone".into(), op.repo.clone().unwrap_or_default(), dest.to_string_lossy().into_owned()]))
        }
        HostedCall::RepoPublish { visibility, description, .. } => {
            let dir = paths.dir.as_ref().ok_or("no directory")?;
            let mut create = vec!["repo".into(), "create".into(), op.repo.clone().unwrap_or_default(), if *visibility == Visibility::Public { "--public".into() } else { "--private".into() }];
            create.extend(description.iter().map(|d| format!("--description={d}")));
            let mut first = glab(create);
            first.cwd = Cwd::Dir(dir.clone());
            let mut push = step(Program::Git, strs(&["-c", "core.hooksPath=/dev/null", "push", "--set-upstream", "origin", "HEAD"]));
            push.cwd = Cwd::Dir(dir.clone());
            Ok(vec![first, push])
        }
        HostedCall::PrList { state, author, label, base, head, search, limit } => {
            let state = match state {
                PrState::Open => "opened",
                PrState::Closed => "closed",
                PrState::Merged => "merged",
                PrState::All => "all",
            };
            let mut query = vec![format!("state={state}"), format!("per_page={limit}"), "order_by=updated_at".into(), "sort=desc".into()];
            for (key, value) in [("author_username", author), ("labels", label), ("target_branch", base), ("source_branch", head), ("search", search)] {
                query.extend(value.iter().map(|v| format!("{key}={}", encode(v))));
            }
            one(api("GET", format!("{}/merge_requests?{}", project(), query.join("&"))))
        }
        HostedCall::PrView { number } => one(api("GET", mr(*number, ""))),
        HostedCall::PrDiff { number } => one(glab(mr_cmd(&["diff"], *number, strs(&["--color=never"])))),
        HostedCall::PrChecks { number } => one(api("GET", mr(*number, "/pipelines"))),
        HostedCall::PrComments { number } => one(api("GET", mr(*number, "/notes?per_page=100&sort=asc&order_by=created_at"))),
        HostedCall::PrReviews { number } => one(api("GET", mr(*number, "/approvals"))),
        HostedCall::PrThreads { number } => one(api("GET", mr(*number, "/discussions?per_page=100"))),
        HostedCall::PrFiles { number } => one(api("GET", mr(*number, "/changes"))),
        HostedCall::PrCreate { title, body, head, base, draft, labels, reviewers } => {
            let mut args = strs(&["mr", "create"]);
            repo_flag(&mut args);
            args.extend([format!("--title={title}"), format!("--source-branch={head}"), format!("--description={}", body.clone().unwrap_or_default()), "--yes".into()]);
            args.extend(base.iter().map(|b| format!("--target-branch={b}")));
            args.extend(labels.iter().map(|l| format!("--label={l}")));
            args.extend(reviewers.iter().map(|r| format!("--reviewer={r}")));
            if *draft {
                args.push("--draft".into());
            }
            one(glab(args))
        }
        HostedCall::PrEdit { number, title, body, base } => {
            let mut extra: Vec<String> = Vec::new();
            extra.extend(title.iter().map(|t| format!("--title={t}")));
            extra.extend(body.iter().map(|b| format!("--description={b}")));
            extra.extend(base.iter().map(|b| format!("--target-branch={b}")));
            one(glab(mr_cmd(&["update"], *number, extra)))
        }
        HostedCall::PrReady { number, undo } => one(glab(mr_cmd(&["update"], *number, strs(&[if *undo { "--draft" } else { "--ready" }])))),
        HostedCall::PrState { number, closed } => one(with_json(api("PUT", mr(*number, "")), serde_json::json!({"state_event": if *closed { "close" } else { "reopen" }}))),
        HostedCall::PrComment { number, body } => one(with_json(api("POST", mr(*number, "/notes")), serde_json::json!({"body": body}))),
        HostedCall::PrReply { number, thread, body, .. } => {
            let thread = thread.as_deref().ok_or("GitLab replies in a discussion named by `thread`")?;
            one(with_json(api("POST", mr(*number, &format!("/discussions/{thread}/notes"))), serde_json::json!({"body": body})))
        }
        HostedCall::PrReviewComment { number, body, commit_id, path, line, base_sha, start_sha, old_path, .. } => {
            let position = serde_json::json!({"position_type": "text", "base_sha": base_sha, "start_sha": start_sha, "head_sha": commit_id, "old_path": old_path.as_ref().unwrap_or(path), "new_path": path, "new_line": line});
            one(with_json(api("POST", mr(*number, "/discussions")), serde_json::json!({"body": body, "position": position})))
        }
        HostedCall::PrReview { number, verdict, body } => match verdict {
            Verdict::Approve => {
                let mut steps = vec![glab(mr_cmd(&["approve"], *number, vec![]))];
                if let Some(b) = body {
                    steps.push(glab(mr_cmd(&["note"], *number, vec![format!("--message={b}")])));
                }
                Ok(steps)
            }
            Verdict::Comment => one(glab(mr_cmd(&["note"], *number, vec![format!("--message={}", body.clone().unwrap_or_default())]))),
            Verdict::RequestChanges => Err("GitLab has no request-changes review: use `comment`".into()),
        },
        HostedCall::PrRequestReviewers { number, add, remove } => {
            let values: Vec<String> = add.iter().map(|a| format!("+{a}")).chain(remove.iter().map(|r| format!("-{r}"))).collect();
            one(glab(mr_cmd(&["update"], *number, vec![format!("--reviewer={}", values.join(","))])))
        }
        HostedCall::PrLabels { number, add, remove } => {
            let mut extra = Vec::new();
            if !add.is_empty() {
                extra.push(format!("--label={}", add.join(",")));
            }
            if !remove.is_empty() {
                extra.push(format!("--unlabel={}", remove.join(",")));
            }
            one(glab(mr_cmd(&["update"], *number, extra)))
        }
        HostedCall::PrReact { number, target, comment_id, content, undo } => {
            if *undo {
                return one(api("GET", "user".into()));
            }
            let path = reaction_collection(op, *number, *target, *comment_id)?;
            one(api("POST", format!("{path}?name={}", award_name(content))))
        }
        HostedCall::PrResolveThread { number, thread, resolved } => {
            let number = number.ok_or("`number` is required on GitLab: a discussion belongs to one merge request")?;
            one(api("PUT", mr(number, &format!("/discussions/{thread}?resolved={resolved}"))))
        }
        HostedCall::PrFileViewed { .. } => Err("GitLab keeps whether a file was viewed in the browser; no API marks it".into()),
        HostedCall::PrUpdateBranch { number, .. } => one(api("PUT", mr(*number, "/rebase"))),
        HostedCall::PrMerge { number, method, auto, delete_branch, match_head_commit } => {
            if *method == MergeMethod::Rebase {
                return Err("GitLab merges as the project is set to (merge or squash); a rebase merge is a project setting".into());
            }
            let mut body = serde_json::json!({"squash": *method == MergeMethod::Squash, "should_remove_source_branch": delete_branch, "merge_when_pipeline_succeeds": auto});
            if let Some(sha) = match_head_commit {
                body["sha"] = serde_json::json!(sha);
            }
            one(with_json(api("PUT", mr(*number, "/merge")), body))
        }
        HostedCall::PrStack { .. } => Ok(Vec::new()),
        HostedCall::PrCommentEdit { number, comment, body, .. } => {
            if !numeric(comment) {
                return Err("a GitLab note is named by its numeric id".into());
            }
            one(json_body(api("PUT", mr(*number, &format!("/notes/{comment}"))), &json!({"body": body})))
        }
        HostedCall::RunList { branch, limit } => {
            let mut query = format!("per_page={limit}&order_by=updated_at&sort=desc");
            if let Some(b) = branch {
                query.push_str(&format!("&ref={}", encode(b)));
            }
            one(api("GET", format!("{}/pipelines?{query}", project())))
        }
        HostedCall::RunView { run } => Ok(vec![api("GET", format!("{}/pipelines/{run}", project())), api("GET", format!("{}/pipelines/{run}/jobs?per_page=100", project()))]),
        HostedCall::AuthStatus => {
            let mut s = glab(strs(&["auth", "status"]));
            s.args.extend(op.host.iter().map(|h| format!("--hostname={h}")));
            s.ok_exits = vec![0, 1];
            one(s)
        }
        HostedCall::LoginStart => Ok(vec![glab(login_args(op))]),
        HostedCall::LoginWait { .. } | HostedCall::LoginCancel { .. } => Err("a sign-in flow is not a program of its own".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_text_with_reserved_characters_when_encoded_then_it_is_one_query_value_or_path_segment() {
        assert_eq!(encode("grp/sub/proj"), "grp%2Fsub%2Fproj");
        assert_eq!(encode("a&b=c d"), "a%26b%3Dc%20d");
        assert_eq!(encode("keep-_.~09azAZ"), "keep-_.~09azAZ");
        assert_eq!(encode("é"), "%C3%A9");
    }
}
