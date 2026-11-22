//! `wf ci comment` through the built binary, against a loopback fake of the GitHub API: the jobs of a run, a job log behind the
//! redirect GitHub answers with, and the commit comments. No request leaves the machine.
use crate::support::{last_json, run_wf, MockReply, MockRequest, MockServer, RunOpts, RunResult};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh, empty log cache directory for each wf run. A name made from the process id is reused by a later test process, which then reads the log an earlier run cached under the same run and job ids.
fn log_cache() -> String {
    tempfile::Builder::new().prefix("ci-log-cache-").tempdir().unwrap().keep().display().to_string()
}

/// The options of a run with the labelling variables of the machine the tests run on (a hosted pipeline sets `WF_CI_LABEL`) cleared: a test that wants one sets it.
fn ci_opts() -> RunOpts {
    let empty = std::env::temp_dir().join("ci-comment-no-workflows");
    std::fs::create_dir_all(&empty).ok();
    RunOpts::default().env("WF_CI_LABEL", "").env("WF_CI_RUNNER_STATS_DIR", "").env("WF_CI_LEDGERS", "").env("GITHUB_WORKSPACE", &empty.display().to_string())
}

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const TOKEN: &str = "flowtesttoken-0123456789";
const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMENTS: &str = "/repos/o/r/commits/0123456789abcdef0123456789abcdef01234567/comments";
const PR_COMMENTS: &str = "/repos/o/r/issues/7/comments";
const PASS_LOG: &str = "2026-10-23T10:00:05.0000000Z > nx run crate-wf-report:cargo-flow  [local cache]
2026-10-23T10:00:06.0000000Z > nx run crate-wf-plan:cargo-flow
2026-10-23T10:00:07.0000000Z verify: nx lint over 2 projects: 1 of 2 tasks replayed from the cache
2026-10-23T10:00:08.0000000Z wf: build cache {\"hits\":9,\"misses\":1,\"puts\":1,\"skipped\":0,\"requests\":12,\"bytes\":100,\"degraded\":null,\"latencyMs\":{\"get\":{\"n\":10,\"sumMs\":630.0,\"maxMs\":210.0},\"put\":{\"n\":1,\"sumMs\":90.0,\"maxMs\":90.0},\"exists\":{\"n\":0,\"sumMs\":0.0,\"maxMs\":0.0}}}
2026-10-23T10:00:09.0000000Z #4 CACHED
2026-10-23T10:00:10.0000000Z #5 DONE 3.1s
2026-10-23T10:00:11.0000000Z {\"exportS\": 219.5, \"uploadS\": 39.3, \"servedS\": 0.2}
";

const FLOW_LOG: &str = "2026-10-23T10:00:01.1234567Z ##[group]Run wf test run --stage flow-tests
2026-10-23T10:00:01.1234567Z echo \"wf: an echoed script line is not a failure\"
2026-10-23T10:00:01.1234567Z ##[endgroup]
2026-10-23T10:01:00.0000000Z \u{1b}[32m   Compiling wf v0.1.0\u{1b}[0m
2026-10-23T10:02:00.0000000Z test ci::keeps_the_table ... ok
2026-10-23T10:02:01.0000000Z test ci::posts_one_comment ... FAILED
2026-10-23T10:02:02.0000000Z thread 'ci::posts_one_comment' panicked at clis/wf/tests/flow/ci.rs:42:9:
2026-10-23T10:02:02.0000000Z assertion `left == right` failed: one comment
2026-10-23T10:02:02.0000000Z   left: 2
2026-10-23T10:02:02.0000000Z  right: 1
2026-10-23T10:02:03.0000000Z using credentials flowtesttoken-0123456789 for the call
2026-10-23T10:02:03.0000000Z leaked ghp_abcdefghijklmnopqrstuvwxyz0123456789 and Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345
2026-10-23T10:02:03.0000000Z fetched https://blob.example/log?sig=SECRETSIG123&se=2026
2026-10-23T10:02:04.0000000Z test result: FAILED. 1 passed; 1 failed; 0 ignored
2026-10-23T10:02:05.0000000Z wf: GET https://objectstorage.eu-frankfurt-1.oraclecloud.com/p/Zm9vYmFyYmF6cXV4Y29yZ2VncmF1bHRnYXJwbHk/n/ns/b/v/o/report.html returned HTTP 403
2026-10-23T10:02:06.0000000Z 2e9df672078b Pulling fs layer
2026-10-23T10:02:06.0000000Z 2e9df672078b Pull complete
2026-10-23T10:02:07.0000000Z Error response from daemon: removal of container 3fb6baf70133 is already in progress
2026-10-23T10:03:03.0000000Z wf: build cache {\"hits\":0,\"misses\":12,\"puts\":0}
2026-10-23T10:03:03.5000000Z wf: sccache {\"hits\":\"0\",\"misses\":\"22\"}
2026-10-23T10:03:04.0000000Z ##[error]Process completed with exit code 101.
2026-10-23T10:03:05.0000000Z Post job cleanup.
";

struct Github {
    server: MockServer,
    /// What the pulls listing answers: one open pull request at the run's head, unless a test removes it.
    prs: std::cell::RefCell<Value>,
    extra: std::cell::RefCell<Vec<(String, MockReply)>>,
}

impl Github {
    /// `wf ci <args>` (not only `comment`) with the run's environment.
    fn ci_wf(&self, args: &[&str]) -> RunResult {
        self.ci_wf_with(args, &[])
    }

    /// The same with more variables of the run (`WF_CI_LABEL`).
    fn ci_wf_with(&self, args: &[&str], extra: &[(&str, &str)]) -> RunResult {
        let mut argv = vec!["ci"];
        argv.extend(args);
        let mut opts = ci_opts();
        for (k, v) in extra {
            opts = opts.env(k, v);
        }
        run_wf(
            &argv,
            &opts
                .env("GITHUB_TOKEN", TOKEN)
                .env("GITHUB_REPOSITORY", "o/r")
                .env("GITHUB_RUN_ID", "77")
                .env("GITHUB_SHA", SHA)
                .env("GITHUB_API_URL", &self.url())
                .env("GITHUB_SERVER_URL", "https://github.com")
                .env("GITHUB_REF_NAME", "dev")
                .env("WF_CI_LOG_CACHE", &log_cache())
                .env("NO_PROXY", "127.0.0.1")
                .env("no_proxy", "127.0.0.1"),
        )
    }

    fn start() -> Github {
        Github { server: MockServer::start(None), prs: std::cell::RefCell::new(json!([{ "number": 7, "head": { "sha": SHA } }])), extra: std::cell::RefCell::new(Vec::new()) }
    }

    /// The heads the runs behind older comments were for, as the run lookup answers them.
    fn extra_run_heads(&self, runs: &[(u64, &str)]) {
        let mut extra = self.extra.borrow_mut();
        for (run, sha) in runs {
            extra.push((format!("GET /repos/o/r/actions/runs/{run}"), MockReply::json(200, json!({ "head_sha": sha }))));
        }
    }

    /// The branch has no open pull request.
    fn without_pr(&self) {
        *self.prs.borrow_mut() = json!([]);
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.port)
    }

    /// The run's jobs, the logs behind a redirect, the commit's comments, and what creating or updating one answers.
    fn serve(&self, jobs: &Value, comments: Value, post: MockReply, patch: MockReply) {
        self.serve_with(jobs, comments, json!([]), post, patch)
    }

    /// As `serve`, with the comments already on the pull request.
    fn serve_with(&self, jobs: &Value, comments: Value, pr_comments: Value, post: MockReply, patch: MockReply) {
        self.serve_log(jobs, comments, pr_comments, post, patch, FLOW_LOG)
    }

    /// As `serve_with`, with the text of the failed job's log.
    fn serve_log(&self, jobs: &Value, comments: Value, pr_comments: Value, post: MockReply, patch: MockReply, flow_log: &str) {
        let delete_ids: Vec<u64> = pr_comments.as_array().map(|a| a.iter().filter_map(|c| c["id"].as_u64()).collect()).unwrap_or_default();
        let mut table = vec![
            ("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1".to_string(), MockReply::json(200, self.prs.borrow().clone())),
            (format!("GET {PR_COMMENTS}?per_page=100&page=1"), MockReply::json(200, pr_comments)),
            (format!("POST {PR_COMMENTS}"), MockReply::json(201, json!({ "id": 9100 }))),
            ("PATCH /repos/o/r/issues/comments/9100".to_string(), MockReply::json(200, json!({ "id": 9100 }))),
            ("GET /repos/o/r/actions/jobs/500/logs".to_string(), MockReply::text(200, PASS_LOG)),
            ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1".to_string(), MockReply::json(200, json!({ "total_count": jobs.as_array().unwrap().len(), "jobs": jobs }))),
            (format!("GET {COMMENTS}?per_page=100&page=1"), MockReply::json(200, comments)),
            (format!("POST {COMMENTS}"), post),
            ("PATCH /repos/o/r/comments/9001".to_string(), patch),
        ];
        for id in delete_ids {
            table.push((format!("DELETE /repos/o/r/issues/comments/{id}"), MockReply::json(204, json!({}))));
        }
        table.extend(self.extra.borrow().iter().cloned());
        let mut redirect = MockReply::json(302, json!({}));
        redirect.headers.insert("Location".into(), format!("{}/blob/501", self.url()));
        let mut log = MockReply::text(200, flow_log);
        log.headers.insert("Content-Type".into(), "text/plain".into());
        table.extend([("GET /repos/o/r/actions/jobs/501/logs".to_string(), redirect), ("GET /blob/501".to_string(), log)]);
        self.server.routes(&table.iter().map(|(key, reply)| (key.as_str(), reply.clone())).collect::<Vec<_>>());
    }

    fn sent(&self, method: &str) -> Vec<MockRequest> {
        self.server.requests().into_iter().filter(|r| r.method == method).collect()
    }

    fn comment_wf(&self, args: &[&str]) -> RunResult {
        self.comment_wf_with(args, &[])
    }

    fn comment_wf_with(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut argv = vec!["ci", "comment"];
        argv.extend(args);
        let opts = ci_opts()
            .env("GITHUB_TOKEN", TOKEN)
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GITHUB_RUN_ID", "77")
            .env("GITHUB_SHA", SHA)
            .env("GITHUB_API_URL", &self.url())
            .env("GITHUB_SERVER_URL", "https://github.com")
            .env("GITHUB_REF_NAME", "dev")
            .env("RUNNER_NAME", "github-actions-1")
            .env("WF_CI_LOG_CACHE", &log_cache())
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1");
        let opts = env.iter().fold(opts, |opts, (key, value)| opts.env(key, value));
        run_wf(&argv, &opts)
    }
}

fn job(id: u64, name: &str, conclusion: &str, done: &str, steps: Value) -> Value {
    json!({ "id": id, "name": name, "status": "completed", "conclusion": conclusion, "started_at": "2026-10-23T10:00:00Z", "completed_at": done, "html_url": format!("https://github.com/o/r/actions/runs/77/job/{id}"), "steps": steps })
}

/// plan passed, flow-tests failed at its second step, clis and the deploy were skipped after it, and one job had been skipped by the plan before.
fn red_run() -> Value {
    let steps = json!([{ "name": "Set up job", "conclusion": "success" }, { "name": "Flow tests", "conclusion": "failure" }]);
    json!([
        job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])),
        job(501, "flow-tests", "failure", "2026-10-23T10:03:05Z", steps),
        job(502, "clis", "skipped", "2026-10-23T10:03:06Z", json!([])),
        job(503, "deploy (vm-ops-api)", "skipped", "2026-10-23T10:03:06Z", json!([])),
        job(504, "android-e2e-tests", "skipped", "2026-10-23T10:00:20Z", json!([]))
    ])
}

/// A comment of the Actions bot, the only author whose CI comments are ever pruned.
fn bot(id: u64, body: &str) -> Value {
    json!({ "id": id, "body": body, "user": { "login": "github-actions[bot]", "type": "Bot" } })
}

fn created() -> MockReply {
    MockReply::json(201, json!({ "id": 9001 }))
}

fn updated() -> MockReply {
    MockReply::json(200, json!({ "id": 9001 }))
}

fn posted_body(request: &MockRequest) -> String {
    serde_json::from_str::<Value>(&request.body).unwrap()["body"].as_str().unwrap().to_string()
}

#[test]
fn a_red_run_gets_one_commit_comment_with_a_table_of_what_failed_and_no_secret_in_it() {
    // Given a run whose flow-tests job failed, with its log behind a redirect, a commit with no comment yet and no open pull request
    let github = Github::start();
    github.without_pr();
    github.serve(&red_run(), json!([]), created(), updated());

    // When the comment is previewed with --dry-run
    let preview = github.comment_wf(&["--dry-run"]);
    // Then it names the failed job and shows the body, and nothing is posted
    let shown = last_json(&preview);
    assert_eq!((preview.exit_code, shown["ok"].clone(), shown["posted"].clone(), shown["dryRun"].clone(), shown["failedJobs"].clone()), (0, json!(true), json!(false), json!(true), json!(["flow-tests"])), "{}{}", preview.stdout, preview.stderr);
    assert!(github.sent("POST").is_empty() && github.sent("PATCH").is_empty());

    // When the comment is posted
    github.server.clear();
    let r = github.comment_wf(&[]);
    // Then exactly one comment is created on the pushed commit, and the line says so
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone(), out["posted"].clone(), out["action"].clone(), out["commentId"].clone(), out["failedJobs"].clone()), (0, json!(true), json!(true), json!("created"), json!(9001), json!(["flow-tests"])), "{}{}", r.stdout, r.stderr);
    let posts: Vec<MockRequest> = github.sent("POST").into_iter().filter(|p| p.url == COMMENTS).collect();
    assert_eq!(posts.len(), 1, "{posts:?}");
    let body = posted_body(&posts[0]);
    let after_marker = |text: &str| text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    assert_eq!(after_marker(shown["body"].as_str().unwrap()), after_marker(&body), "the preview is the body that is posted");

    // And a reader finds the verdict, the commit and the run, and the report: each job's cache hit rate, the Nx replays, the docker
    // layers, the EAS seconds, and for each crate whether its tests ran or were replayed
    assert!(body.starts_with("<!-- ci-comment run:77 runner:github-actions-1 -->\n"), "{body}");
    assert!(body.lines().nth(1).is_some_and(|title| title.contains("failed") && title.contains("0123456")), "{body}");
    for link in ["https://github.com/o/r/commit/0123456789abcdef0123456789abcdef01234567", "https://github.com/o/r/actions/runs/77"] {
        assert!(body.contains(link), "{link} missing: {body}");
    }
    for fact in ["90% hit", "Nx replayed 1 of 2", "docker layers cached 1 of 2", "EAS export 219.5 s, upload 39.3 s, served 0.2 s"] {
        assert!(body.contains(fact), "{fact} missing: {body}");
    }
    let line_of = |needle: &str| body.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} missing: {body}")).to_string();
    assert!(line_of("crate-wf-report").contains("replayed") && line_of("crate-wf-plan").contains(" ran"), "{body}");
    assert!(body.contains("flow-tests") && body.contains("Flow tests") && body.contains("3m 05s"), "the failed job, its step and how long it took: {body}");
    let failure = body.split("### Jobs").next().unwrap();
    assert!(failure.contains("test ci::posts_one_comment ... FAILED") && !failure.contains("sccache {"), "the cache figures printed at the end of the log are not the cause: {failure}");
    let first_cause = body.find("test ci::posts_one_comment ... FAILED").expect("the cause");
    assert!(first_cause < body.find("Nx replayed").expect("the report"), "what failed comes before the figures: {body}");
    for cause in ["test ci::posts_one_comment ... FAILED", "panicked at clis/wf/tests/flow/ci.rs:42:9:", "assertion `left == right` failed: one comment", "test result: FAILED. 1 passed; 1 failed; 0 ignored"] {
        assert!(body.contains(cause), "{cause} missing from {body}");
    }
    assert!(body.contains("Error response from daemon: removal of container 3fb6baf70133 is already in progress") && !body.contains("Pull complete"), "a docker daemon error is a cause and pull progress is noise: {body}");
    let skipped = line_of("Skipped because of the failure");
    assert!(skipped.contains("`clis`") && skipped.contains("`deploy (vm-ops-api)`") && !skipped.contains("android-e2e-tests") && !body.contains("an echoed script line"), "{body}");
    // and a collapsed block with more of the log, stripped of its timestamps and colours
    assert!(body.contains("<details><summary>flow-tests: ") && body.contains("Compiling wf v0.1.0") && body.contains("[Open the job](https://github.com/o/r/actions/runs/77/job/501)"), "{body}");
    assert!(!body.contains("2026-10-23T10:01") && !body.contains('\u{1b}'), "{body}");

    // And no token and no pre-authenticated path is in it, in the comment or in the output
    for secret in [TOKEN, "ghp_abcdef", "abcdefghijklmnopqrstuvwxyz012345", "Zm9vYmFyYmF6", "SECRETSIG123"] {
        assert!(!body.contains(secret) && !r.stdout.contains(secret) && !r.stderr.contains(secret), "{secret} leaked: {body}");
    }
    assert!(body.contains("/p/[REDACTED]/n/ns/b/v/o/report.html returned HTTP 403"), "{body}");

    // And the API calls carry the token while the log's redirect target never sees it
    let (calls, bearer) = (github.server.requests(), format!("Bearer {TOKEN}"));
    assert!(calls.iter().filter(|c| !c.url.starts_with("/blob/")).all(|c| c.auth.as_deref() == Some(bearer.as_str())), "{calls:?}");
    assert!(calls.iter().filter(|c| c.url.starts_with("/blob/")).all(|c| c.auth.is_none()) && calls.iter().any(|c| c.url == "/blob/501"), "{calls:?}");
}

/// The first 26 lines of a job log of runner 2.338.2 as GitHub stored it (a UTF-8 byte order mark in front of the first line, the runner
/// header, the token permissions group, the checkout guard), copied from run 41208868377.
const RUNNER_2_338_2_HEAD: &str = include_str!("runner_2_338_2_head.log");

#[test]
fn a_failed_job_of_runner_2_338_2_shows_its_cause_not_the_runner_header() {
    // Given a failed flow-tests job whose log is the real head of a 2.338.2 log (byte order mark included) and then a failing step
    let log = format!(
        "{RUNNER_2_338_2_HEAD}2026-10-31T14:08:30.1000000Z ##[group]Run wf test flow\n\
         2026-10-31T14:08:30.1100000Z wf test flow --all\n\
         2026-10-31T14:08:30.1200000Z ##[endgroup]\n\
         2026-10-31T14:09:10.2000000Z test deploy::ship::a_redeploy_is_sent_once ... FAILED\n\
         2026-10-31T14:09:10.2100000Z thread 'deploy::ship::a_redeploy_is_sent_once' panicked at clis/deploy/tests/flow/ship.rs:88:9:\n\
         2026-10-31T14:09:10.2200000Z assertion `left == right` failed: one redeploy\n\
         2026-10-31T14:09:10.2300000Z ##[error]Process completed with exit code 101.\n"
    );
    assert!(log.starts_with('\u{feff}'), "the fixture keeps the byte order mark GitHub writes");
    let github = Github::start();
    github.serve_log(&red_run(), json!([]), json!([]), created(), updated(), &log);

    // When the comment is previewed
    let preview = github.comment_wf(&["--dry-run"]);

    // Then the failed job is named and the lines that say why are in the comment, ahead of the runner header and the permissions group
    let shown = last_json(&preview);
    assert_eq!((preview.exit_code, shown["failedJobs"].clone()), (0, json!(["flow-tests"])), "{}{}", preview.stdout, preview.stderr);
    let body = shown["body"].as_str().unwrap();
    for cause in ["test deploy::ship::a_redeploy_is_sent_once ... FAILED", "panicked at clis/deploy/tests/flow/ship.rs:88:9:", "assertion `left == right` failed: one redeploy"] {
        assert!(body.contains(cause), "{cause} missing from {body}");
    }
    let failure = body.split("### Jobs").next().unwrap();
    assert!(!failure.contains("Process completed with exit code"), "GitHub's generic exit line says nothing about why: {failure}");
    assert!(!failure.contains("Current runner version") && !failure.contains("GITHUB_TOKEN Permissions"), "the runner header is not the cause: {failure}");
    assert!(!body.contains("2026-10-31T14:09") && !body.contains('\u{feff}'), "timestamps and the byte order mark are stripped: {body}");
}

#[test]
fn a_re_run_of_the_same_run_updates_its_comment_instead_of_posting_a_second() {
    // Given a red run, on a branch with no pull request, that has posted its comment once
    let github = Github::start();
    github.without_pr();
    github.serve(&red_run(), json!([]), created(), updated());
    assert_eq!(github.comment_wf(&[]).exit_code, 0);
    let first = posted_body(&github.sent("POST")[0]);
    // and the commit now lists that comment beside another one
    github.serve(&red_run(), json!([{ "id": 5, "body": "looks good" }, { "id": 9001, "body": first, "user": { "login": "github-actions[bot]", "type": "Bot" } }]), created(), updated());
    github.server.clear();

    // When the same run fails again and wf comments
    let r = github.comment_wf(&[]);

    // Then the comment that carries the run's marker is updated in place and no second one is posted
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["posted"].clone(), out["action"].clone(), out["commentId"].clone()), (0, json!(true), json!("updated"), json!(9001)), "{}{}", r.stdout, r.stderr);
    assert!(github.sent("POST").iter().all(|p| p.url == PR_COMMENTS), "only the pull request gets a new comment: it has none of this run's yet");
    let patches = github.sent("PATCH");
    assert_eq!(patches.len(), 1, "{patches:?}");
    assert_eq!(patches[0].url, "/repos/o/r/comments/9001");
    let body = posted_body(&patches[0]);
    assert!(body.starts_with("<!-- ci-comment run:77 runner:github-actions-1 -->") && body.contains("flow-tests") && body.contains("3m 05s"), "{body}");
}

#[test]
fn a_green_run_reports_on_the_pull_request_and_in_the_summary_and_a_refused_comment_never_fails_the_ci() {
    // Given a run whose jobs all passed or were skipped, and a summary file
    let github = Github::start();
    let green = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(502, "clis", "skipped", "2026-10-23T10:00:21Z", json!([]))]);
    github.serve(&green, json!([]), created(), updated());
    let summary = std::env::temp_dir().join(format!("ci-summary-{}", std::process::id()));
    let _ = std::fs::remove_file(&summary);

    // When wf reports
    let r = github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", summary.to_str().unwrap())]);

    // Then no commit comment is made, the pull request gets the report, and the summary carries it
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone(), out["posted"].clone(), out["prComment"].clone(), out["summary"].clone()), (0, json!(true), json!(false), json!({ "number": 7, "action": "created", "commentId": 9100 }), json!(true)), "{}{}", r.stdout, r.stderr);
    assert!(github.sent("POST").iter().all(|p| p.url == PR_COMMENTS));
    let report = std::fs::read_to_string(&summary).unwrap();
    assert!(report.lines().nth(1).is_some_and(|title| title.contains("passed") && title.contains("0123456")) && report.contains("plan") && report.contains("20s"), "{report}");
    assert_eq!(posted_body(&github.sent("POST")[0]).trim_end(), report.trim_end());

    // Given a red run, with no pull request, whose comment GitHub refuses
    github.without_pr();
    github.serve(&red_run(), json!([]), MockReply::json(403, json!({ "message": "Resource not accessible by integration" })), updated());
    // When wf comments
    let refused = github.comment_wf(&[]);
    // Then it still exits 0, one stderr line gives the reason, and the line says nothing was posted
    let out = last_json(&refused);
    assert_eq!((refused.exit_code, out["ok"].clone(), out["posted"].clone(), out["failedJobs"].clone()), (0, json!(false), json!(false), json!(["flow-tests"])), "{}{}", refused.stdout, refused.stderr);
    assert!(out["reason"].as_str().unwrap().contains("posting the comment: HTTP 403: Resource not accessible by integration"), "{out}");
    assert_eq!(refused.stderr.lines().count(), 1, "{}", refused.stderr);
    assert!(refused.stderr.starts_with("wf ci comment: posting the comment: HTTP 403"), "{}", refused.stderr);
}

const QUOTA_LOG: &str = "2026-10-23T10:00:08.0000000Z wf: build cache storage quota exceeded: the tenancy's Object Storage is full and no write can succeed
2026-10-23T10:00:09.0000000Z wf: build cache {\"hits\":4,\"misses\":2,\"puts\":0,\"skipped\":0,\"quotaExceeded\":2,\"requests\":8,\"bytes\":0,\"degraded\":null,\"latencyMs\":{\"get\":{\"n\":6,\"sumMs\":60.0,\"maxMs\":20.0},\"put\":{\"n\":2,\"sumMs\":40.0,\"maxMs\":30.0},\"exists\":{\"n\":0,\"sumMs\":0.0,\"maxMs\":0.0}}}
";

#[test]
fn a_green_run_whose_cache_writes_were_refused_for_a_full_tenancy_says_so_in_the_headline_and_on_the_job_and_a_clean_run_does_not() {
    // Given a green run whose flow-tests job logged two cache writes refused with StorageQuotaExceeded, and a green run that logged none
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    let summary = std::env::temp_dir().join(format!("ci-summary-quota-{}", std::process::id()));
    let _ = std::fs::remove_file(&summary);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), QUOTA_LOG);

    // When wf reports
    let r = github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", summary.to_str().unwrap())]);

    // Then the headline flags the full tenancy with the number of refused writes, the job row says it, and the run is not marked failed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report = std::fs::read_to_string(&summary).unwrap();
    assert!(report.contains("Object Storage full: 2 cache write(s) refused"), "{report}");
    assert!(report.contains("Object Storage quota exceeded: 2 cache write(s) refused"), "{report}");
    assert!(!report.contains("failed"), "{report}");

    // And a run with no refused write has no such flag
    let clean = std::env::temp_dir().join(format!("ci-summary-clean-{}", std::process::id()));
    let _ = std::fs::remove_file(&clean);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), PASS_LOG);
    github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", clean.to_str().unwrap())]);
    assert!(!std::fs::read_to_string(&clean).unwrap().to_lowercase().contains("quota"));
}

#[test]
fn the_pull_request_keeps_only_the_comment_of_the_run_that_reported_last() {
    // Given a pull request that carries the CI comments the bot left for two earlier heads and an unrelated comment
    let github = Github::start();
    let older = "<!-- ci-comment run:76 runner:github-actions-1 -->\nolder run";
    let live = "<!-- ci-live run:75 -->\nan earlier live comment";
    github.extra_run_heads(&[(76, OLD), (75, OLD)]);
    github.serve_with(&red_run(), json!([]), json!([bot(3, older), bot(4, live), { "id": 5, "body": "a human's remark" }]), created(), updated());

    // When this run reports
    let r = github.comment_wf(&[]);

    // Then its own comment is created at the bottom, the two CI comments of the older heads are deleted, and the remark is left alone
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["prComment"]["action"].clone()), (0, json!("created")), "{}{}", r.stdout, r.stderr);
    let mut deleted: Vec<String> = github.sent("DELETE").into_iter().map(|d| d.url).collect();
    deleted.sort();
    assert_eq!(deleted, ["/repos/o/r/issues/comments/3", "/repos/o/r/issues/comments/4"]);
}

/// An RFC 3339 instant `secs` seconds from the epoch, as the Actions API writes it.
fn iso(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let (doy, y) = (doe - (365 * yoe + yoe / 4 - yoe / 100), yoe + era * 400);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", if m <= 2 { y + 1 } else { y }, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A green run that finished a few minutes ago, with the VM's numbers served for that span (a gap of `hole_min` minutes in the middle).
fn vm_run(github: &Github, hole_min: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let (start, end) = (now - 600, now - 120);
    let mut recent = job(500, "plan", "success", &iso(end), json!([]));
    recent["started_at"] = json!(iso(start));
    github.serve(&json!([recent]), json!([]), created(), updated());
    let mut points = Vec::new();
    for minute in 0..8i64 {
        if hole_min > 0 && minute > 2 && minute <= 2 + hole_min {
            continue;
        }
        let ts = (start + minute * 60 + 30) * 1000;
        let host = |metric: &str, value: f64| json!({ "ts": ts, "scope": "host", "name": "host", "metric": metric, "value": value });
        points.extend([host("cpu", 20.0 + minute as f64), host("memory_used", 5.0e9), host("memory_total", 12.5e9), host("psi_io", if minute == 4 { 62.0 } else { 3.0 })]);
    }
    let vm = MockServer::start(None);
    vm.routes(&[("GET /api/metrics/series?window=24h&scope=host", MockReply::json(200, json!({ "window": "24h", "points": points })))]);
    let url = format!("http://127.0.0.1:{}", vm.port);
    std::mem::forget(vm);
    url
}

#[test]
fn the_report_shows_what_the_vm_was_doing_while_the_jobs_ran_and_when_it_stopped_sampling() {
    // Given a run that finished minutes ago and a reporting-service that stored the VM's numbers for it
    let github = Github::start();
    let vm = vm_run(&github, 0);

    // When wf reports
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", &vm), ("WF_TAILNET_SOCKS", "")]);

    // Then the worst I/O wait of the run is in the report with its value, and the VM is said to have kept up
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    assert!(body.contains("62%"), "the I/O wait peak: {body}");
    assert!(body.to_lowercase().contains("no gaps"), "{body}");
    let charts: Vec<&str> = body.split("```mermaid\n").skip(1).map(|c| c.split("```").next().unwrap_or("")).collect();
    assert!(charts.iter().all(|c| c.starts_with("xychart-beta") && c.contains("bar [") && !c.contains("line [")), "only vertical bar charts, which a phone in portrait draws whole: {body}");
    assert!(!body.contains("gantt"), "{body}");
    assert!(charts.iter().any(|c| c.contains("62.0")), "a chart of the I/O wait carries its peak: {body}");

    // Given the VM stopped sampling for four minutes in the middle of the run
    let stalled = vm_run(&github, 4);
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", &stalled), ("WF_TAILNET_SOCKS", "")]);
    // Then the report says it stalled
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    assert!(body.to_lowercase().contains("stalled") && !body.to_lowercase().contains("no gaps"), "{body}");
}

#[test]
fn a_report_is_still_made_when_the_vm_cannot_be_asked_and_it_says_so() {
    // Given a run that finished minutes ago and nothing answering where the VM's numbers live
    let github = Github::start();
    let _ = vm_run(&github, 0);
    // When wf reports
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", "http://127.0.0.1:9"), ("WF_TAILNET_SOCKS", "")]);
    // Then the report is made, with a line that the VM's stats are unavailable
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (0, json!(true)), "{}{}", r.stdout, r.stderr);
    assert!(out["body"].as_str().unwrap().contains("unavailable"), "{out}");
}

#[test]
fn a_test_run_never_writes_into_the_real_job_summary() {
    // Given a runner whose summary file the test process inherits
    let summary = std::env::temp_dir().join(format!("real-summary-{}", std::process::id()));
    std::fs::write(&summary, "the real summary\n").unwrap();
    let github = Github::start();
    github.serve(&red_run(), json!([]), created(), updated());
    // When wf runs under the test harness with that variable present in the parent environment
    std::env::set_var("GITHUB_STEP_SUMMARY", &summary);
    let r = github.comment_wf(&[]);
    std::env::remove_var("GITHUB_STEP_SUMMARY");
    // Then the fixture's report never reached it
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(std::fs::read_to_string(&summary).unwrap(), "the real summary\n");
}

#[test]
fn jobs_that_failed_the_same_way_are_one_block_naming_each_of_them_and_the_cause_once() {
    // Given two jobs that failed in the same step with the same log
    let github = Github::start();
    let steps = json!([{ "name": "Set up job", "conclusion": "success" }, { "name": "Flow tests", "conclusion": "failure" }]);
    let twins = json!([job(501, "deploy (a)", "failure", "2026-10-23T10:03:05Z", steps.clone()), job(501, "deploy (b)", "failure", "2026-10-23T10:03:05Z", steps), job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([]))]);
    github.serve(&twins, json!([]), created(), updated());

    // When wf reports
    let r = github.comment_wf(&["--dry-run"]);

    // Then both jobs are named in what failed, and the failing test line is in it once
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    let failure = body.split("### Jobs").next().unwrap();
    assert!(failure.contains("deploy (a)") && failure.contains("deploy (b)"), "{failure}");
    assert_eq!(failure.matches("test ci::posts_one_comment ... FAILED").count(), 1, "{failure}");
}

const NX_LOG: &str = "2026-10-23T10:02:00.0000000Z crate-wf-login: error: process didn't exit successfully: `sccache rustc -vV` (exit status: 2)
2026-10-23T10:02:00.1000000Z crate-wf-login: --- stderr
2026-10-23T10:02:00.2000000Z crate-wf-login: sccache: error: Connection to server timed out: Connection refused
2026-10-23T10:02:05.0000000Z {\"ok\":false,\"stage\":\"flow-tests\",\"fresh\":false}
2026-10-23T10:02:06.0000000Z wf: build cache {\"hits\":0,\"misses\":12}
2026-10-23T10:02:06.5000000Z wf: flow-tests: unit in crate-a, crate-b
";

#[test]
fn a_failure_a_task_prints_behind_nxs_project_label_is_the_cause_and_the_stage_summary_is_not() {
    // Given a failed job whose log has the error behind Nx's `crate-x: ` label, then the run's summary lines
    let github = Github::start();
    github.serve_log(&red_run(), json!([]), json!([]), created(), updated(), NX_LOG);

    // When wf reports
    let r = github.comment_wf(&["--dry-run"]);

    // Then the cause is named, and the summary line is not shown as one
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    let failure = body.split("### Jobs").next().unwrap();
    assert!(failure.contains("Connection to server timed out"), "{failure}");
    assert!(!failure.contains("\"stage\"") && !failure.contains("build cache {") && !failure.contains("unit in crate-a"), "{failure}");
}

const TIMED_LOG: &str = "2026-10-23T10:00:00.0000000Z crate-slow: test ci::very_slow ... 
2026-10-23T10:03:20.0000000Z crate-slow: ok
2026-10-23T10:03:21.0000000Z crate-slow: test result: ok. 1 passed; 0 failed; finished in 200.00s
2026-10-23T10:03:30.0000000Z Task Execution Timings:
2026-10-23T10:03:30.0000000Z {
2026-10-23T10:03:30.0000000Z   \"crate-slow:cargo-flow\": 200000
2026-10-23T10:03:30.0000000Z }
2026-10-23T10:03:30.0000000Z   Run duration:      3m 30s
2026-10-23T10:03:30.0000000Z   Cache:             0/1 hit (0%)
2026-10-23T10:03:30.0000000Z   Critical path:     3m 20s (1 task)
2026-10-23T10:03:30.0000000Z   Recoverable time:  10.0s (5% of the run)
2026-10-23T10:03:31.0000000Z wf: sccache {\"hits\":\"3\",\"misses\":\"7\",\"writeErrors\":\"0\"}
2026-10-23T10:03:31.0000000Z wf: nx cost {\"target\":\"cargo-flow\",\"cpuSeconds\":120.0,\"writtenBytes\":2000000000,\"seconds\":100.0}
";

#[test]
fn the_report_says_where_the_time_went_what_waited_who_wrote_the_disk_and_what_the_caches_missed() {
    // Given a run whose second job waited 100 s for the runner and ran a 200 s test, and a VM that stored a container's disk writes
    let github = Github::start();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let start = now - 900;
    let mut plan = job(500, "plan", "success", &iso(start + 100), json!([]));
    (plan["created_at"], plan["started_at"]) = (json!(iso(start)), json!(iso(start)));
    let steps = json!([{ "name": "Flow tests", "conclusion": "success", "started_at": iso(start + 200), "completed_at": iso(start + 600) }]);
    let mut flow = job(501, "flow-tests", "success", &iso(start + 650), steps);
    (flow["created_at"], flow["started_at"]) = (json!(iso(start + 100)), json!(iso(start + 200)));
    github.serve_log(&json!([plan, flow]), json!([]), json!([]), created(), updated(), TIMED_LOG);
    let container = |ts: i64, metric: &str, value: f64| json!({ "ts": ts * 1000, "scope": "container", "name": "agent-shell", "metric": metric, "value": value });
    let containers = json!([container(start + 210, "disk_write_bytes", 1.0e9), container(start + 500, "disk_write_bytes", 3.5e9), container(start + 300, "cpu", 80.0)]);
    let host = |m: i64, metric: &str, value: f64| json!({ "ts": (start + m * 60 + 30) * 1000, "scope": "host", "name": "host", "metric": metric, "value": value });
    let hosts: Vec<Value> = (0..10).flat_map(|m| [host(m, "cpu", 90.0), host(m, "cpu_steal", 10.0)]).collect();
    let vm = MockServer::start(None);
    let since = (start - 60) * 1000;
    vm.routes(&[
        ("GET /api/metrics/series?window=24h&scope=host", MockReply::json(200, json!({ "points": hosts }))),
        (&format!("GET /api/metrics/series?window=24h&scope=container&metric=cpu,memory,disk_read_bytes,disk_write_bytes&since={since}"), MockReply::json(200, json!({ "points": containers }))),
    ]);
    let url = format!("http://127.0.0.1:{}", vm.port);

    // When wf reports
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", &url), ("WF_TAILNET_SOCKS", "")]);

    // Then the wait is named, the slow test and task are named, the disk writer is named with its bytes, and the cache misses are listed
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    assert!(body.contains("Waited for the runner: **flow-tests** 1m 40s"), "{body}");
    assert!(body.contains("ci::very_slow") && body.contains("crate-slow:cargo-flow"), "{body}");
    assert!(body.contains("agent-shell") && body.contains("2.5 GB"), "the container that wrote 2.5 GB: {body}");
    assert!(body.contains("sccache 30% (3/10)") && body.contains("Nx runs"), "{body}");
    assert!(body.contains("| flow-tests | cargo-flow | 120 s | 60% | 2.0 GB | 100 s |"), "the stage's CPU seconds, share of both cores and bytes written: {body}");
    assert!(body.contains("the host was busy 1.80 = containers 0.80 + the hypervisor 0.20 + not in any container 0.80 (44% of the busy time)"), "the two-core split of the busy time: {body}");
}

const BASE: i64 = 1_793_000_000;

fn timed_job(id: u64, name: &str, conclusion: &str, secs: i64) -> Value {
    let mut j = job(id, name, conclusion, &iso(BASE + secs), json!([]));
    (j["created_at"], j["started_at"]) = (json!(iso(BASE)), json!(iso(BASE)));
    j
}

fn ledger_line(tasks: &[(&str, &str, &str, i64)]) -> String {
    let list: Vec<Value> = tasks.iter().map(|(task, hash, outcome, ms)| json!({ "task": task, "hash": hash, "outcome": outcome, "ms": ms, "passed": true })).collect();
    format!("2026-10-30T10:00:00.0000000Z wf: nx ledger {}\n", json!({ "tasks": list }))
}

/// This run (77) with one job whose log is `log`, and the other runs of the workflow as `(started, updated)` seconds from BASE.
fn audit_github(current: &Value, log: &str, others: &[(i64, i64)]) -> Github {
    audit_github_of(std::slice::from_ref(current), log, others)
}

/// The same with several jobs; job 500's log is `log`.
fn audit_github_of(jobs: &[Value], log: &str, others: &[(i64, i64)]) -> Github {
    let github = Github::start();
    let runs: Vec<Value> = others.iter().enumerate().map(|(i, (a, b))| json!({ "id": 900 + i, "run_started_at": iso(BASE + a), "updated_at": iso(BASE + b) })).collect();
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/ci-hosted-backend-core.yml/runs?branch=dev&per_page=30", MockReply::json(200, json!({ "workflow_runs": runs }))),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": jobs.len(), "jobs": jobs }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, log)),
    ]);
    github
}

struct Audit {
    ledger: std::path::PathBuf,
    nx: std::path::PathBuf,
    limits: std::path::PathBuf,
}

impl Audit {
    /// A ledger file holding `earlier` (as JSON lines), an `nx` that says `affected` changed, and the thresholds file `limits`.
    fn new(name: &str, earlier: &[Value], affected: &str, limits: Value) -> Audit {
        let dir = std::env::temp_dir().join(format!("ci-audit-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (ledger, nx, limits_file) = (dir.join("ledger.jsonl"), dir.join("nx"), dir.join("limits.json"));
        std::fs::write(&ledger, earlier.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        std::fs::write(&nx, format!("#!/bin/sh\nprintf '%s' '{affected}'\n")).unwrap();
        std::process::Command::new("chmod").args(["755", nx.to_str().unwrap()]).status().unwrap();
        std::fs::write(&limits_file, limits.to_string()).unwrap();
        Audit { ledger, nx, limits: limits_file }
    }

    /// Audits the one run 77 (`--run`), or with `extra` whatever else the test passes.
    fn run_with(&self, github: &Github, extra: &[&str]) -> RunResult {
        let mut argv = vec!["ci", "audit"];
        argv.extend_from_slice(extra);
        run_wf(
            &argv,
            &ci_opts()
                .env("GITHUB_TOKEN", TOKEN)
                .env("GITHUB_REPOSITORY", "o/r")
                .env("GITHUB_RUN_ID", "1")
                .env("GITHUB_SHA", SHA)
                .env("GITHUB_API_URL", &github.url())
                .env("WF_CI_LOG_CACHE", &log_cache())
                .env("WF_CI_LEDGER_FILE", self.ledger.to_str().unwrap())
                .env("WF_NX_BIN", self.nx.to_str().unwrap())
                .env("WF_CI_AUDIT_FILE", self.limits.to_str().unwrap())
                .env("WF_VM_STATS_URL", "http://127.0.0.1:9")
                .env("WF_TAILNET_SOCKS", "")
                .env("NO_PROXY", "127.0.0.1")
                .env("no_proxy", "127.0.0.1"),
        )
    }

    fn run(&self, github: &Github, dry: bool) -> RunResult {
        let mut extra = vec!["--run", "77"];
        if dry {
            extra.push("--dry-run");
        }
        self.run_with(github, &extra)
    }
}

fn earlier_task(run: &str, ts: i64, task: &str, hash: &str, outcome: &str, ms: i64) -> Value {
    json!({ "t": "task", "run": run, "sha": format!("sha{run}"), "ts": ts, "job": "flow-tests", "task": task, "hash": hash, "outcome": outcome, "ms": ms, "passed": true })
}

fn kinds(out: &Value) -> Vec<String> {
    out["findings"].as_array().unwrap().iter().map(|b| b["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_task_that_ran_on_inputs_an_earlier_run_already_cached_is_named_with_its_seconds_and_the_next_step_and_fails_nothing() {
    // Given a task that was replayed from the cache before and ran again now under the same input hash
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "h1", "ran", 90_000), earlier_task("71", now - 5000, "app:lint", "h1", "hit", 50)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "h1", "ran", 95_000)]), &[]);
    let audit = Audit::new("hit", &earlier, "", json!({}));

    // When the run is audited
    let r = audit.run(&github, true);

    // Then the cache's miss is a correctness finding with a next step, and the command still exits 0
    let out = last_json(&r);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(kinds(&out), ["should-have-hit"], "{out}");
    let finding = &out["findings"][0];
    let message = finding["message"].as_str().unwrap();
    assert!(message.contains("app:lint") && message.contains("1m 35s"), "{message}");
    assert_eq!((finding["hard"].clone(), finding["run"].clone(), out["ok"].clone()), (json!(true), json!("77"), json!(false)), "{out}");
    assert!(!finding["next"].as_str().unwrap().is_empty(), "{out}");
}

#[test]
fn a_task_that_ran_because_its_inputs_changed_is_not_waste() {
    // Given a task whose hash is new (its inputs changed), with the project reported as changed
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "h1", "hit", 50)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "h2", "ran", 95_000)]), &[]);
    let audit = Audit::new("changed", &earlier, "app\n", json!({}));
    // When the run is audited
    let r = audit.run(&github, true);
    // Then nothing is found
    let out = last_json(&r);
    assert_eq!((r.exit_code, kinds(&out), out["ok"].clone()), (0, Vec::<String>::new(), json!(true)), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_hash_that_moved_although_nothing_it_depends_on_changed_is_named_as_drift() {
    // Given a task whose hash differs from the previous run's while nx says no project changed
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "old", "hit", 50)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "new", "hit", 40)]), &[]);
    // When the run is audited
    let r = Audit::new("drift", &earlier, "", json!({})).run(&github, true);
    // Then the drift is named and the command passes
    assert_eq!((r.exit_code, kinds(&last_json(&r))), (0, vec!["key-drift".to_string()]), "{}{}", r.stdout, r.stderr);
}

#[test]
fn the_same_work_taking_far_longer_than_before_is_reported_as_the_machine_being_slow() {
    // Given a task that ran twice before under this hash in about ten seconds and now took a minute
    let now = BASE * 1000;
    let earlier = [earlier_task("68", now - 30_000, "app:t", "h", "ran", 10_000), earlier_task("69", now - 20_000, "app:t", "h", "ran", 11_000)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:t", "h", "ran", 60_000)]), &[]);
    // When the run is audited
    let r = Audit::new("slow", &earlier, "app\n", json!({})).run(&github, true);
    // Then the slowdown under the same inputs is named
    assert!(kinds(&last_json(&r)).contains(&"slower-same-work".to_string()), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_wait_behind_another_run_is_not_a_finding_and_an_unexplained_one_is() {
    // Given a job created at BASE that started 15 minutes later
    let mut waited = timed_job(500, "flow-tests", "success", 1500);
    waited["started_at"] = json!(iso(BASE + 900));
    // When another run was holding the runner the whole time
    let busy = audit_github(&waited, "ok\n", &[(0, 950)]);
    let r = Audit::new("wait-a", &[], "", json!({})).run(&busy, true);
    // Then the wait is not reported
    assert!(!kinds(&last_json(&r)).contains(&"runner-wait".to_string()), "{}{}", r.stdout, r.stderr);
    // When no other run was running
    let idle = audit_github(&waited, "ok\n", &[]);
    let r = Audit::new("wait-b", &[], "", json!({})).run(&idle, true);
    // Then the wait is reported
    assert!(kinds(&last_json(&r)).contains(&"runner-wait".to_string()), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_job_whose_cache_refused_to_write_is_a_correctness_finding_that_still_fails_nothing() {
    // Given a run whose job log says the build cache read only
    let log = "wf: build cache reads only for this run, dirty: the work tree has changes: ARCHITECTURE.md\n";
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), log, &[]);
    // When the run is audited
    let r = Audit::new("refused", &[], "", json!({})).run(&github, true);
    // Then the cache is named as a hard finding and the exit code is 0
    let out = last_json(&r);
    assert_eq!((r.exit_code, kinds(&out), out["findings"][0]["hard"].clone()), (0, vec!["cache-read-only".to_string()], json!(true)), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_run_with_a_failed_job_is_not_judged_for_time_because_its_timings_say_nothing() {
    let github = audit_github(&timed_job(500, "flow-tests", "failure", 1200), "ok\n", &[]);
    let r = Audit::new("failed", &[], "", json!({})).run(&github, true);
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["runs"][0]["judged"].clone()), (0, json!(false)), "{}{}", r.stdout, r.stderr);
}

#[test]
fn what_a_run_did_is_in_the_ledger_for_the_next_run_to_be_judged_against() {
    // Given a run whose task ran under a new hash
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "h9", "ran", 5000)]), &[]);
    let audit = Audit::new("record", &[], "app\n", json!({}));
    // When the audit runs for real
    let first = audit.run(&github, false);
    assert_eq!(first.exit_code, 0, "{}{}", first.stdout, first.stderr);
    // Then a later run that reruns the same task on the same hash after a replay is judged against it
    let stored = std::fs::read_to_string(&audit.ledger).unwrap();
    assert!(stored.contains("\"hash\":\"h9\"") && stored.contains("\"job\":\"flow-tests\""), "{stored}");
}

#[test]
fn the_cpu_seconds_threshold_is_what_names_a_run_and_a_pnpm_pack_outside_flow_tests_is_named() {
    // Given a run whose test job packed the pnpm store and whose window the VM was busy in (the reporting-service is unreachable here, so the
    // CPU-seconds cannot be measured: the threshold only decides when there is a number)
    let pack = "2026-10-23T10:00:10.0000000Z ts=1793000002 level=info source=ci-cache msg=\"cache.save\" kind=pnpm verb=save done=true bytes=100 reason=\"\" seconds=75.2\n";
    let github = audit_github(&timed_job(500, "typecheck-lint-unit-tests", "success", 600), pack, &[]);
    // When the run is audited
    let r = Audit::new("pnpm", &[], "", json!({ "cpuSecondsThreshold": 1 })).run(&github, true);
    // Then the pack is named with the job that did it, and only core-tests may do it
    let out = last_json(&r);
    let found: Vec<&Value> = out["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == "pnpm-packed-outside-flow-tests").collect();
    assert_eq!(found.len(), 1, "{out}");
    assert_eq!(found[0]["subject"], "typecheck-lint-unit-tests", "{out}");
    let own = audit_github(&timed_job(500, "core-tests", "success", 600), pack, &[]);
    let r = Audit::new("pnpm-own", &[], "", json!({})).run(&own, true);
    assert!(!kinds(&last_json(&r)).contains(&"pnpm-packed-outside-flow-tests".to_string()), "{}{}", r.stdout, r.stderr);
}

#[test]
fn the_audit_without_a_run_judges_every_completed_run_on_dev_since_the_given_time_oldest_first() {
    // Given three completed runs, one of them older than the window, and a task that reran in the newer run what the older one already cached
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "h1", "ran", 90_000), earlier_task("71", now - 5000, "app:lint", "h1", "hit", 50)];
    let github = Github::start();
    let run = |id: u64, started: i64, event: &str| json!({ "id": id, "head_sha": SHA, "event": event, "run_started_at": iso(BASE + started) });
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/ci-hosted-backend-core.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(200, json!({ "workflow_runs": [run(78, 5000, "push"), run(77, 100, "push"), run(60, -90_000, "push"), run(61, 200, "pull_request")] }))),
        ("GET /repos/o/r/actions/workflows/ci-hosted-backend-core.yml/runs?branch=dev&per_page=30", MockReply::json(200, json!({ "workflow_runs": [] }))),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(500, "flow-tests", "success", 600)] }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, &ledger_line(&[("app:lint", "h1", "ran", 95_000)]))),
        ("GET /repos/o/r/actions/runs/78/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(501, "flow-tests", "failure", 600)] }))),
        ("GET /repos/o/r/actions/jobs/501/logs", MockReply::text(200, "ok\n")),
    ]);
    // When the audit runs with a window start between the old run and the two newer ones
    let r = Audit::new("window", &earlier, "", json!({})).run_with(&github, &["--dry-run", "--since", &(BASE - 1000).to_string()]);
    // Then the two push runs are judged oldest first (the old run and the pull request's are not), and the miss is named on the first
    let out = last_json(&r);
    let runs: Vec<&str> = out["runs"].as_array().unwrap().iter().map(|r| r["run"].as_str().unwrap()).collect();
    assert_eq!((r.exit_code, runs), (0, vec!["77", "78"]), "{}{}", r.stdout, r.stderr);
    assert_eq!(kinds(&out), ["should-have-hit"], "{out}");
    assert_eq!(out["runs"][1]["judged"], json!(false), "{out}");
}

#[test]
fn the_audit_judges_the_hosted_workflow_too_and_calls_a_scheduled_or_dispatched_run_cold() {
    // Given a scheduled and a pushed run of the CI workflow
    let earlier: [Value; 0] = [];
    let github = Github::start();
    let run = |id: u64, started: i64, event: &str| json!({ "id": id, "head_sha": SHA, "event": event, "run_started_at": iso(BASE + started) });
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/ci-hosted-backend-core.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(200, json!({ "workflow_runs": [run(78, 200, "schedule"), run(79, 300, "push")] }))),
        ("GET /repos/o/r/actions/workflows/ci-hosted-backend-core.yml/runs?branch=dev&per_page=30", MockReply::json(200, json!({ "workflow_runs": [] }))),
        ("GET /repos/o/r/actions/runs/78/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(501, "flow-tests", "success", 600)] }))),
        ("GET /repos/o/r/actions/jobs/501/logs", MockReply::text(200, &ledger_line(&[("app:lint", "h1", "ran", 95_000)]))),
        ("GET /repos/o/r/actions/runs/79/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(502, "flow-tests", "failure", 600)] }))),
        ("GET /repos/o/r/actions/jobs/502/logs", MockReply::text(200, "ok\n")),
    ]);
    // When the audit runs over a window that holds both
    let r = Audit::new("hosted", &earlier, "", json!({})).run_with(&github, &["--dry-run", "--since", &(BASE - 1000).to_string()]);
    // Then both are listed oldest first, only the scheduled one is cold, both say they ran GitHub-hosted, and a note says the runs' times were not judged
    let out = last_json(&r);
    let seen: Vec<(String, bool, bool)> = out["runs"].as_array().unwrap().iter().map(|r| (r["run"].as_str().unwrap().to_string(), r["cold"] == true, r["hosted"] == true)).collect();
    assert_eq!((r.exit_code, seen), (0, vec![("78".to_string(), true, true), ("79".to_string(), false, true)]), "{}{}", r.stdout, r.stderr);
    assert!(out["notes"].to_string().contains("GitHub-hosted"), "{out}");
}

const HOSTED_FILES: [&str; 6] = ["ci-hosted-backend-core.yml", "ci-hosted-mobile-core.yml", "ci-hosted-cli-core.yml", "ci-hosted-cli-build.yml", "ci-hosted-backend-build.yml", "ci-hosted-mobile-build.yml"];

#[test]
fn the_audit_judges_the_runs_of_all_six_hosted_workflows_and_a_missing_one_is_a_note_not_a_failure() {
    // Given one push run in each of five workflows, and a sixth workflow (the e2e one) that GitHub answers 404 for
    let github = Github::start();
    let run = |id: u64, started: i64| json!({ "id": id, "head_sha": SHA, "event": "push", "run_started_at": iso(BASE + started) });
    let mut table: Vec<(String, MockReply)> = Vec::new();
    for (n, file) in HOSTED_FILES.iter().take(5).enumerate() {
        let id = 100 + n as u64;
        table.push((format!("GET /repos/o/r/actions/workflows/{file}/runs?branch=dev&status=completed&per_page=50"), MockReply::json(200, json!({ "workflow_runs": [run(id, 100 + n as i64 * 10)] }))));
        table.push((format!("GET /repos/o/r/actions/workflows/{file}/runs?branch=dev&per_page=30"), MockReply::json(200, json!({ "workflow_runs": [] }))));
        table.push((format!("GET /repos/o/r/actions/runs/{id}/jobs?per_page=100&page=1"), MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(600 + id, "flow-tests", "success", 600)] }))));
        table.push((format!("GET /repos/o/r/actions/jobs/{}/logs", 600 + id), MockReply::text(200, "ok\n")));
    }
    table.push(("GET /repos/o/r/actions/workflows/ci-hosted-mobile-build.yml/runs?branch=dev&status=completed&per_page=50".into(), MockReply::json(404, json!({ "message": "Not Found" }))));
    github.server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());

    // When the audit lists the window
    let r = Audit::new("six", &[], "", json!({})).run_with(&github, &["--dry-run", "--since", &(BASE - 1000).to_string()]);

    // Then the runs of the five workflows are judged oldest first, the exit code is 0 and only the sixth workflow is a note
    let out = last_json(&r);
    let runs: Vec<&str> = out["runs"].as_array().unwrap().iter().map(|r| r["run"].as_str().unwrap()).collect();
    assert_eq!((r.exit_code, runs), (0, vec!["100", "101", "102", "103", "104"]), "{}{}", r.stdout, r.stderr);
    let notes = out["notes"].to_string();
    assert!(notes.contains("ci-hosted-mobile-build.yml") && !notes.contains("ci-hosted-cli-core.yml could not"), "{notes}");
}

fn group_job(run: &str, job: &str, key: &str, started: i64, secs: i64, conclusion: &str) -> Value {
    json!({ "t": "job", "run": run, "sha": "s", "ts": (BASE + started) * 1000, "kind": "push", "job": job, "secs": secs, "queued": 0, "group_key": key, "started": BASE + started, "conclusion": conclusion })
}

fn audit_groups(name: &str, rows: &[Value]) -> Value {
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), "ok\n", &[]);
    let r = Audit::new(name, rows, "", json!({})).run_with(&github, &["--run", "77", "--dry-run", "--since", &(BASE - 1000).to_string()]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    last_json(&r)
}

#[test]
fn two_runs_that_held_one_concurrency_group_at_the_same_time_are_named_and_back_to_back_runs_are_not() {
    // Given two jobs of different runs in group k1 whose intervals overlap, and two in group k2 where the second starts when the first ends
    let rows = [
        group_job("70", "integration-tests (1/4)", "k1", 100, 300, "success"),
        group_job("71", "integration-tests (1/4)", "k1", 250, 200, "success"),
        group_job("70", "integration-tests (2/4)", "k2", 100, 100, "success"),
        group_job("71", "integration-tests (2/4)", "k2", 200, 100, "success"),
    ];

    // When the audit runs
    let out = audit_groups("overlap", &rows);

    // Then exactly the overlap of k1 is a hard finding, on the later run, naming both runs and the group
    assert_eq!(kinds(&out), ["group-overlap"], "{out}");
    let f = &out["findings"][0];
    let message = f["message"].as_str().unwrap();
    assert_eq!((f["run"].clone(), f["hard"].clone()), (json!("71"), json!(true)), "{out}");
    assert!(message.contains("run 70") && message.contains("run 71") && message.contains("`k1`"), "{message}");
    assert!(!f["next"].as_str().unwrap().is_empty(), "{out}");
}

#[test]
fn a_job_cancelled_after_it_started_is_named_and_one_replaced_while_waiting_is_not() {
    // Given a job of group k1 that was cancelled after 40 s of running, and one of group k2 that was cancelled while pending (it never started)
    let mut pending = group_job("71", "integration-tests (2/4)", "k2", 0, 0, "cancelled");
    pending["started"] = json!(0);
    let rows = [group_job("70", "integration-tests (1/4)", "k1", 100, 40, "cancelled"), pending];

    // When the audit runs
    let out = audit_groups("cancelled", &rows);

    // Then only the started one is a finding
    assert_eq!(kinds(&out), ["cancelled-while-running"], "{out}");
    assert!(out["findings"][0]["message"].as_str().unwrap().contains("run 70"), "{out}");
}

#[test]
fn a_pending_job_a_newer_one_replaced_is_expected_and_a_job_killed_while_running_is_still_a_finding() {
    // Given a shard of run 70 that GitHub cancelled one second after its recorded start (a replaced pending job) and a shard of run 71 cancelled after 90 s of running
    let rows = [group_job("70", "core-tests (1/3)", "hosted-core-tests-CI core-1-k1", 100, 1, "cancelled"), group_job("71", "integration-tests (1/4)", "hosted-integration-CI core-1-k2", 100, 90, "cancelled")];

    // When the audit runs
    let out = audit_groups("replaced", &rows);

    // Then only the killed one is a finding, on run 71; the replaced one is expected
    let found: Vec<(String, String)> = out["findings"].as_array().unwrap().iter().map(|f| (f["kind"].as_str().unwrap().to_string(), f["run"].as_str().unwrap().to_string())).collect();
    assert_eq!(found, [("cancelled-while-running".to_string(), "71".to_string())], "{out}");
}

#[test]
fn a_job_cancelled_two_seconds_after_its_start_is_replaced_and_one_cancelled_three_seconds_after_is_a_kill() {
    // Given a job of run 70 cancelled 2 s after its recorded start and one of run 71 cancelled 3 s after
    let rows = [group_job("70", "core-tests (1/3)", "k1", 100, 2, "cancelled"), group_job("71", "core-tests (2/3)", "k2", 100, 3, "cancelled")];

    // When the audit runs
    let out = audit_groups("boundary", &rows);

    // Then the 2 s one is expected and only the 3 s one is a running job killed
    assert_eq!(kinds(&out), ["cancelled-while-running"], "{out}");
    assert!(out["findings"][0]["message"].as_str().unwrap().contains("run 71"), "{out}");
}

#[test]
fn two_runs_of_the_same_shard_inputs_in_one_group_that_never_overlap_and_a_replaced_pending_job_are_no_finding() {
    // Given core-tests (1/2) of run 70 running 100 s in group k1, run 71's pending job of k1 replaced 1 s after its recorded start, and run 72's job of k1 starting after run 70 ended
    let rows = [
        group_job("70", "core-tests (1/2)", "hosted-core-tests-CI core-1-k1", 100, 100, "success"),
        group_job("71", "core-tests (1/2)", "hosted-core-tests-CI core-1-k1", 200, 1, "cancelled"),
        group_job("72", "core-tests (1/2)", "hosted-core-tests-CI core-1-k1", 201, 100, "success"),
    ];

    // When the audit runs
    let out = audit_groups("serialised", &rows);

    // Then nothing is found: the group serialised the runs and the replacement is expected
    assert!(kinds(&out).is_empty(), "{out}");
}

#[test]
fn a_run_cancelled_by_hand_ends_several_started_jobs_at_once_and_is_not_a_finding() {
    // Given run 70 whose three running jobs were all cancelled together, and run 71 with one running job cancelled alone
    let rows = [
        group_job("70", "integration-tests (1/4)", "k1", 100, 40, "cancelled"),
        group_job("70", "integration-tests (2/4)", "k2", 100, 41, "cancelled"),
        group_job("70", "core-tests (1/2)", "k3", 100, 40, "cancelled"),
        group_job("71", "integration-tests (1/4)", "k4", 100, 40, "cancelled"),
    ];

    // When the audit runs
    let out = audit_groups("operator-cancel", &rows);

    // Then only the lone kill of run 71 is named
    assert_eq!(kinds(&out), ["cancelled-while-running"], "{out}");
    assert!(out["findings"][0]["message"].as_str().unwrap().contains("run 71"), "{out}");
}

#[test]
fn two_workflows_that_plan_the_same_shard_key_run_side_by_side_and_are_not_one_group() {
    // Given core-tests (1/2) of two workflows recorded under their own full group strings (the same shard key `none`), running at the same time
    let rows = [
        group_job("70", "core-tests (1/2)", "hosted-core-tests-CI core-1-none", 100, 300, "success"),
        group_job("71", "core-tests (1/2)", "hosted-core-tests-CI mobile-1-none", 120, 300, "success"),
    ];

    // When the audit runs
    let out = audit_groups("two-workflows", &rows);

    // Then nothing is found
    assert_eq!((kinds(&out), out["ok"].clone()), (Vec::<String>::new(), json!(true)), "{out}");
}

#[test]
fn jobs_of_different_groups_that_run_side_by_side_and_a_ledger_with_no_groups_are_clean() {
    // Given overlapping jobs in different groups, and a job with no group
    let mut ungrouped = group_job("72", "lint", "x", 100, 300, "success");
    ungrouped["group_key"] = Value::Null;
    let rows = [group_job("70", "integration-tests (1/4)", "k1", 100, 300, "success"), group_job("71", "integration-tests (2/4)", "k2", 120, 300, "success"), ungrouped];

    // When the audit runs
    let out = audit_groups("clean", &rows);

    // Then nothing is found
    assert_eq!((kinds(&out), out["ok"].clone()), (Vec::<String>::new(), json!(true)), "{out}");
}

#[test]
fn a_task_run_again_under_a_hash_another_run_already_ran_is_the_duplicate_the_single_flight_should_prevent() {
    // Given run 70 ran app:t under hash h1, the task was replayed from the cache under another hash before, and run 77 runs app:t under h1 as well
    let now = BASE * 1000;
    let earlier = [earlier_task("69", now - 20_000, "app:t", "h0", "hit", 50), earlier_task("70", now - 10_000, "app:t", "h1", "ran", 80_000)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:t", "h1", "ran", 82_000)]), &[]);

    // When the audit runs
    let r = Audit::new("duplicate", &earlier, "app\n", json!({})).run(&github, true);

    // Then the duplicate is a correctness finding naming the earlier run
    let out = last_json(&r);
    assert_eq!(kinds(&out), ["should-have-hit"], "{out}");
    let f = &out["findings"][0];
    assert!(f["message"].as_str().unwrap().contains("run 70") && f["hard"] == json!(true), "{out}");
}

#[test]
fn an_audit_whose_workflow_cannot_be_listed_exits_0_with_no_runs_and_says_so() {
    // Given a GitHub that answers 404 for the CI workflow
    let github = Github::start();
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/ci-hosted-backend-core.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(404, json!({ "message": "Not Found" }))),
    ]);
    // When the audit runs
    let r = Audit::new("no-hosted", &[], "", json!({})).run_with(&github, &["--dry-run", "--since", &(BASE - 1000).to_string()]);
    // Then no run is judged, the exit code is 0 and a note names the workflow that could not be listed
    let out = last_json(&r);
    let notes = out["notes"].to_string();
    assert_eq!((r.exit_code, out["runs"].as_array().unwrap().len()), (0, 0), "{}{}", r.stdout, r.stderr);
    assert!(notes.contains("ci-hosted-backend-core.yml"), "{notes}");
}

#[test]
fn preflight_names_every_check_that_failed_and_what_to_do_about_it() {
    // Given a GitHub that refuses the token, a service that is down and an environment with no cache links
    let github = Github::start();
    github.server.routes(&[("GET /rate_limit", MockReply::json(401, json!({ "message": "Bad credentials" })))]);
    let r = run_wf(
        &["ci", "preflight", "--skip", "workspace,toolchain,secrets,ledger"],
        &ci_opts()
            .env("GITHUB_TOKEN", TOKEN)
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GITHUB_RUN_ID", "77")
            .env("GITHUB_SHA", SHA)
            .env("GITHUB_API_URL", &github.url())
            .env("BUILD_CACHE_READ_PAR_URL", "")
            .env("BUILD_CACHE_WRITE_PAR_URL", "")
            .env("WF_PREFLIGHT_HEALTH", "svc=http://127.0.0.1:9/health")
            .env("WF_TAILNET_SOCKS", "")
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1"),
    );
    // When wf runs the preflight
    let out = last_json(&r);
    // Then it fails, naming the checks that failed, each with what to do
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let failed: Vec<&str> = out["failed"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(failed.contains(&"github") && failed.contains(&"svc") && failed.contains(&"build-cache"), "{out}");
    assert!(out["checks"].as_array().unwrap().iter().filter(|c| c["ok"] == false).all(|c| !c["next"].as_str().unwrap().is_empty()), "{out}");
}

fn build_cache_check(stores: &str) -> Value {
    let github = Github::start();
    github.server.routes(&[("GET /rate_limit", MockReply::json(200, json!({ "resources": { "core": { "remaining": 4000 } } })))]);
    let r = run_wf(
        &["ci", "preflight", "--skip", "workspace,toolchain,secrets,ledger"],
        &ci_opts()
            .env("GITHUB_TOKEN", TOKEN)
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GITHUB_RUN_ID", "77")
            .env("GITHUB_SHA", SHA)
            .env("GITHUB_API_URL", &github.url())
            .env("BUILD_CACHE_READ_STORES", stores)
            .env("BUILD_CACHE_WRITE_STORES", stores)
            .env("BUILD_CACHE_READ_PAR_URL", "")
            .env("BUILD_CACHE_WRITE_PAR_URL", "")
            .env("WF_PREFLIGHT_HEALTH", "svc=http://127.0.0.1:9/health")
            .env("WF_TAILNET_SOCKS", "")
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1"),
    );
    last_json(&r)["checks"].as_array().unwrap().iter().find(|c| c["check"] == "build-cache").cloned().unwrap_or_else(|| panic!("no build-cache check: {}{}", r.stdout, r.stderr))
}

#[test]
fn preflight_fails_on_a_declared_store_that_is_down_and_names_it_without_its_link() {
    // Given a declared store that answers nothing
    let list = "par+http://127.0.0.1:9/p/LINK-SECRET/o/";

    // When the preflight runs
    let check = build_cache_check(list);

    // Then the build-cache check failed, says which store and what to do, and repeats no link
    assert_eq!(check["ok"], false, "{check}");
    let text = check.to_string();
    assert!(text.contains("par+http://127.0.0.1:9") && !text.contains("LINK-SECRET") && !check["next"].as_str().unwrap().is_empty(), "{check}");
}

#[test]
fn preflight_passes_with_no_store_declared_when_every_entry_is_disabled() {
    // Given a store list whose only entry is disabled
    // When the preflight runs
    let check = build_cache_check("-par+http://127.0.0.1:9/p/LINK-SECRET/o/");

    // Then the check passes and says no store is declared, and the disabled store was not needed
    assert_eq!(check["ok"], true, "{check}");
    assert!(check["detail"].as_str().unwrap().contains("no store declared"), "{check}");
}

#[test]
fn preflight_passes_when_what_it_checks_is_live() {
    // Given a GitHub that accepts the token and a service that is up
    let github = Github::start();
    github.server.routes(&[("GET /rate_limit", MockReply::json(200, json!({ "resources": { "core": { "remaining": 4000 } } })))]);
    let service = MockServer::start(None);
    service.routes(&[("GET /health", MockReply::json(200, json!({ "ok": true })))]);
    let r = run_wf(
        &["ci", "preflight", "--skip", "workspace,toolchain,secrets,ledger,build-cache"],
        &ci_opts()
            .env("GITHUB_TOKEN", TOKEN)
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GITHUB_RUN_ID", "77")
            .env("GITHUB_SHA", SHA)
            .env("GITHUB_API_URL", &github.url())
            .env("WF_PREFLIGHT_HEALTH", &format!("svc=http://127.0.0.1:{}/health", service.port))
            .env("WF_TAILNET_SOCKS", "")
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1"),
    );
    // Then it passes
    assert_eq!((r.exit_code, last_json(&r)["ok"].clone()), (0, json!(true)), "{}{}", r.stdout, r.stderr);
}

fn live_jobs() -> Value {
    let mut preflight = timed_job(500, "preflight", "success", 12);
    preflight["status"] = json!("completed");
    let running = json!({ "id": 501, "name": "flow-tests", "status": "in_progress", "conclusion": null, "started_at": iso(BASE + 20), "completed_at": null, "html_url": "https://github.com/o/r/actions/runs/77/job/501", "steps": [{ "name": "Set up job", "status": "completed" }, { "name": "Flow tests", "status": "in_progress" }] });
    let queued = json!({ "id": 502, "name": "clis", "status": "queued", "conclusion": null, "started_at": null, "completed_at": null, "html_url": "https://github.com/o/r/actions/runs/77/job/502", "steps": [] });
    json!([preflight, running, queued])
}

fn live_github(comments: Value) -> Github {
    live_github_of(comments, live_jobs())
}

fn live_github_of(comments: Value, jobs: Value) -> Github {
    let github = Github::start();
    github.server.routes(&[
        ("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1", MockReply::json(200, json!([{ "number": 7, "head": { "sha": SHA } }]))),
        ("GET /repos/o/r/actions/runs/70", MockReply::json(200, json!({ "head_sha": OLD }))),
        (&format!("GET {PR_COMMENTS}?per_page=100&page=1"), MockReply::json(200, comments)),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": jobs.as_array().map_or(0, Vec::len), "jobs": jobs }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, "ok\n")),
        (&format!("POST {PR_COMMENTS}"), MockReply::json(201, json!({ "id": 9100 }))),
        ("PATCH /repos/o/r/issues/comments/9001", MockReply::json(200, json!({ "id": 9001 }))),
        ("DELETE /repos/o/r/issues/comments/8000", MockReply::json(204, json!({}))),
        ("DELETE /repos/o/r/issues/comments/8001", MockReply::json(204, json!({}))),
    ]);
    github
}

#[test]
fn the_live_comment_of_a_new_run_replaces_the_old_one_at_the_bottom_and_shows_what_is_running() {
    // Given an earlier run's live comment on the pull request and a run with one job done, one running and one waiting
    let github = live_github(json!([bot(8000, "<!-- ci-live run:70 -->\n## 🟡 CI running"), { "id": 8001, "body": "an unrelated comment" }]));

    // When the run's first job starts the live comment
    let r = github.ci_wf(&["live", "start"]);

    // Then the old one is deleted, a new one is created, and it names the job running with its step and the one queued
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(github.sent("DELETE").len(), 1, "the earlier run's comment goes, an unrelated one stays");
    let created = github.sent("POST");
    assert_eq!(created.len(), 1, "{:?}", github.server.requests());
    let body = posted_body(&created[0]);
    assert!(body.contains("CI (GitHub-hosted) running") && body.contains("1 running") && body.contains("1 queued"), "{body}");
    assert!(body.contains("flow-tests") && body.contains("in: Flow tests") && body.contains("clis") && body.contains("queued"), "{body}");
}

#[test]
fn the_live_comment_of_a_run_is_edited_in_place_not_recreated() {
    // Given this run's live comment already on the pull request
    let github = live_github(json!([bot(9001, "<!-- ci-live run:77 -->\nold")]));
    // When a later job updates it
    let r = github.ci_wf(&["live"]);
    // Then the comment is edited and no second one is made
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((github.sent("PATCH").len(), github.sent("POST").len(), github.sent("DELETE").len()), (1, 0, 0), "{:?}", github.server.requests());
    assert!(posted_body(&github.sent("PATCH")[0]).contains("flow-tests"));
}

#[test]
fn a_run_with_no_pull_request_makes_no_live_comment() {
    let github = Github::start();
    github.server.routes(&[("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1", MockReply::json(200, json!([])))]);
    let r = github.ci_wf(&["live", "start"]);
    assert_eq!((r.exit_code, github.sent("POST").len()), (0, 0), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_slow_github_makes_the_live_comment_give_up_within_the_bound_and_never_fail_the_step() {
    // Given a GitHub that answers every request only after 20 s, and a bound of 1.5 s
    let github = live_github(json!([]));
    github.server.delay(std::time::Duration::from_secs(20));
    let started = std::time::Instant::now();

    // When the run's job updates the live comment
    let r = github.ci_wf_with(&["live"], &[("WF_CI_LIVE_BOUND_MS", "1500")]);

    // Then it exits 0 well inside the 20 s, says in one line that it gave up and why, and posted nothing
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "took {:?}", started.elapsed());
    assert!(r.stderr.contains("wf ci live: gave up after 1.5s: GitHub did not finish answering"), "{}", r.stderr);
    assert!(r.stdout.contains("\"posted\":false"), "{}", r.stdout);
    assert_eq!(github.sent("POST").len() + github.sent("PATCH").len(), 0);
}

#[test]
fn a_github_that_answers_502_makes_the_live_comment_log_why_and_exit_0() {
    // Given a GitHub whose pull request lookup answers 502
    let github = Github::start();
    github.server.routes(&[("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1", MockReply::json(502, json!({ "message": "Bad Gateway" })))]);

    // When the run updates the live comment
    let r = github.ci_wf(&["live"]);

    // Then it exits 0 and names the failure
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("wf ci live:") && r.stderr.contains("502"), "{}", r.stderr);
}

fn write_sized(path: &std::path::Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn age_days(path: &std::path::Path, days: u32) {
    let status = std::process::Command::new("touch").args(["-d", &format!("{days} days ago"), path.to_str().unwrap()]).status().unwrap();
    assert!(status.success());
}

#[test]
fn a_filling_runner_volume_loses_what_can_be_rebuilt_and_nothing_else_and_an_empty_one_loses_nothing() {
    // Given a runner volume holding a crate's cargo target (incremental state and built dependencies), five installs of wf of which the newest
    // three and the published one are to stay, and a file that is none of those
    let root = std::env::temp_dir().join(format!("tidy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_sized(&root.join("crate/Cargo.toml"), 10);
    write_sized(&root.join("crate/target/CACHEDIR.TAG"), 10);
    write_sized(&root.join("crate/target/debug/.fingerprint/f"), 10);
    write_sized(&root.join("crate/target/debug/incremental/state"), 4096);
    write_sized(&root.join("crate/target/debug/deps/lib.rlib"), 4096);
    // and a built artifact of a version of the code that nothing has touched for ten days, beside the fresh one
    write_sized(&root.join("crate/target/debug/deps/old-1a2b3c.rlib"), 4096);
    age_days(&root.join("crate/target/debug/deps/old-1a2b3c.rlib"), 10);
    write_sized(&root.join("notes.txt"), 10);
    let installs = root.join("installs");
    for (i, name) in ["t1", "t2", "t3", "t4", "t5"].iter().enumerate() {
        write_sized(&installs.join(name).join("wf"), 100);
        age_days(&installs.join(name), 10 - i as u32);
    }
    std::os::unix::fs::symlink("t1", installs.join("current")).unwrap();
    let run = |high: &str| {
        let r = run_wf(&["ci", "tidy", "--root", root.to_str().unwrap(), "--installs", installs.to_str().unwrap(), "--high", high, "--low", "0"], &ci_opts());
        (r.exit_code, last_json(&r))
    };

    // When the volume is under the high mark
    let (code, out) = run("100");
    // Then nothing is removed
    assert_eq!((code, out["removed"].as_array().unwrap().len()), (0, 0), "{out}");
    assert!(root.join("crate/target/debug/incremental/state").exists());

    // When it is over the mark
    let (code, out) = run("0");
    // Then the incremental state and the oldest installs the rule does not keep go, and the built dependencies, the files and the kept installs stay
    assert_eq!(code, 0, "{out}");
    assert!(!root.join("crate/target/debug/incremental").exists(), "{out}");
    assert!(root.join("crate/target/debug/deps/lib.rlib").exists() && root.join("notes.txt").exists());
    assert!(!root.join("crate/target/debug/deps/old-1a2b3c.rlib").exists(), "the artifact nothing used for ten days goes while the fresh one stays: {out}");
    let kept: Vec<bool> = ["t1", "t2", "t3", "t4", "t5"].iter().map(|n| installs.join(n).exists()).collect();
    assert_eq!(kept, [true, false, true, true, true], "the published install and the newest three stay: {out}");
    let _ = std::fs::remove_dir_all(&root);
}

const SICK_LOG: &str = "2026-10-23T10:00:08.0000000Z ts=1793000000 level=warn source=ci-cache msg=\"cache.run\" kind=nx hits=3 misses=1 puts=4 skipped=0 requests=9 bytes=10 degraded=no seeded_checked=4 seeded_present=3
2026-10-23T10:00:09.0000000Z ts=1793000001 level=warn source=ci-cache msg=\"cache.put.dropped\" kind=sccache dropped=3 quota_exceeded=3 error_class=quota-exceeded error_status=400
2026-10-23T10:00:10.0000000Z ts=1793000002 level=info source=ci-cache msg=\"cache.save\" kind=pnpm verb=save done=true bytes=100 reason=\"\" seconds=75.2
2026-10-23T10:00:11.0000000Z ts=1793000003 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=cargo-flow ran=5 replayed=0 replay_ratio=0.00
2026-10-23T10:00:12.0000000Z ts=1793000004 level=warn source=ci-cache msg=\"cache.shim.disabled\" reason=\"WF_BLOB_CACHE=off\"
";

const HEALTHY_LOG: &str = "2026-10-23T10:00:08.0000000Z ts=1793000000 level=info source=ci-cache msg=\"cache.run\" kind=nx hits=3 misses=1 puts=4 skipped=0 requests=9 bytes=10 degraded=no seeded_checked=4 seeded_present=4
2026-10-23T10:00:11.0000000Z ts=1793000003 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=cargo-flow ran=0 replayed=5 replay_ratio=1.00
";

fn health_wf(github: &Github, event: &str) -> RunResult {
    health_wf_with(github, event, &[])
}

fn health_wf_with(github: &Github, event: &str, extra: &[(&str, &str)]) -> RunResult {
    let mut opts = ci_opts()
        .env("GITHUB_TOKEN", TOKEN)
        .env("GITHUB_REPOSITORY", "o/r")
        .env("GITHUB_RUN_ID", "77")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_API_URL", &github.url())
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REF_NAME", "dev")
        .env("GITHUB_EVENT_NAME", event)
        .env("WF_CI_PUSH_GATE", "on")
        .env("WF_CI_LOG_CACHE", &log_cache())
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");
    for (k, v) in extra {
        opts = opts.env(k, v);
    }
    run_wf(&["ci", "cache-health"], &opts)
}

#[test]
fn the_cache_health_verb_fails_a_run_whose_layers_lost_or_refused_writes_and_passes_a_healthy_one() {
    // Given a green run whose flow-tests job logged a lost write, refused sccache writes, a 75 s pnpm save, a stage that replayed nothing
    // and a disabled shim
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), SICK_LOG);

    // When the health of a push to dev is asked
    let r = health_wf(&github, "push");

    // Then it exits 1 and names each problem with its job, and the disabled cache is a note
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (1, json!(false)), "{}{}", r.stdout, r.stderr);
    let kinds: Vec<&str> = out["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["pnpm-save-slow", "rejected-write", "replayed-nothing", "seeded-lost"], "{out}");
    let seeded = out["findings"].as_array().unwrap().iter().find(|f| f["kind"] == "seeded-lost").unwrap();
    assert!(seeded["job"] == "flow-tests" && seeded["detail"].as_str().unwrap().contains("seeded 3 of 4"), "{out}");
    assert!(out["notes"][0].as_str().unwrap().contains("WF_BLOB_CACHE=off"), "{out}");

    // When the same logs belong to a run that is not a push to dev
    let other = health_wf(&github, "pull_request");
    // Then a stage that replayed nothing is no problem there, the others still are
    let kinds: Vec<String> = last_json(&other)["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, ["pnpm-save-slow", "rejected-write", "seeded-lost"]);

    // Given a run whose cache did everything it should
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), HEALTHY_LOG);
    let healthy = health_wf(&github, "push");
    // Then it exits 0 with the stages it saw
    let out = last_json(&healthy);
    assert_eq!((healthy.exit_code, out["ok"].clone(), out["findings"].clone()), (0, json!(true), json!([])), "{}{}", healthy.stdout, healthy.stderr);
    assert_eq!(out["stages"][0], json!({ "job": "flow-tests", "stage": "flow-tests", "replayed": 5, "tasks": 5 }));
}

fn typecheck_log(task: &str) -> String {
    format!("2026-10-23T10:00:08.0000000Z wf: nx ledger {{\"tasks\":[{{\"task\":\"{task}\",\"hash\":\"12495670247005852844\",\"outcome\":\"ran\",\"ms\":1260,\"passed\":true}}]}}
2026-10-23T10:00:09.0000000Z ts=1793000001 level=info source=ci-cache msg=\"nx.stage.summary\" stage=typecheck-lint-unit-tests target=typecheck ran=1 replayed=0 replay_ratio=0.00
")
}

#[test]
fn the_cache_health_verb_does_not_blame_the_gate_for_a_docs_only_push_whose_only_task_is_the_workspace_root_typecheck() {
    // Given a push to dev that changed only docs, so the stage's one task is the workspace root's typecheck and it ran, not replayed
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "typecheck-lint-unit-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), &typecheck_log("expo-hello-world-app-2-workspace:typecheck"));

    // When the health of the push is asked
    let r = health_wf(&github, "push");

    // Then no replayed-nothing finding is raised, and the stage is still reported
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["findings"].clone()), (0, json!([])), "{}{}", r.stdout, r.stderr);
    assert_eq!(out["stages"][0], json!({ "job": "typecheck-lint-unit-tests", "stage": "typecheck-lint-unit-tests", "replayed": 0, "tasks": 1 }));

    // Given the same stage whose one task is a package's typecheck, which the gate does seed
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), &typecheck_log("mla-tamagui:typecheck"));

    // When the health is asked again
    let blamed = health_wf(&github, "push");

    // Then the stage that replayed nothing is a replayed-nothing finding
    let out = last_json(&blamed);
    let kinds: Vec<&str> = out["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!((blamed.exit_code, kinds), (1, vec!["replayed-nothing"]), "{out}");

    // Given a repo whose pre-push hook seeds nothing (WF_CI_PUSH_GATE=off), and the same stage that replayed nothing
    let unseeded = health_wf_with(&github, "push", &[("WF_CI_PUSH_GATE", "off")]);

    // Then there is no replayed-nothing finding to blame the gate for
    let out = last_json(&unseeded);
    assert_eq!((unseeded.exit_code, out["findings"].clone()), (0, json!([])), "{}{}", unseeded.stdout, unseeded.stderr);
}

const COLD_LOG: &str = "2026-10-23T10:00:08.0000000Z ts=1793000000 level=warn source=ci-cache msg=\"cache.run\" kind=sccache hits=0 misses=0 puts=0 skipped=9 requests=0 bytes=0 degraded=unreachable seeded_checked=0 seeded_present=0
2026-10-23T10:00:09.0000000Z ts=1793000001 level=info source=ci-cache msg=\"cache.save\" kind=pnpm verb=save done=true bytes=100 reason=\"\" seconds=3.0 error_class=rejected error_status=503
2026-10-23T10:00:10.0000000Z ts=1793000002 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=cargo-flow ran=4 replayed=0 replay_ratio=0.00
2026-10-23T10:00:11.0000000Z ts=1793000003 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=test:flow ran=0 replayed=6 replay_ratio=1.00
2026-10-23T10:00:12.0000000Z ts=1793000004 level=warn source=ci-cache msg=\"drift.report\" compared=10 drifted=2 causes=\"env\"
wf: build cache {\"degraded\":\"budget\",\"droppedPuts\":0}
";

#[test]
fn the_cache_health_verb_names_a_cold_layer_and_drift_and_judges_a_stage_by_all_its_kinds_and_a_save_by_what_it_stored() {
    // Given a push to dev whose logs hold a cold sccache, a budget-degraded shim line, a drift report, a pnpm save that stored fine
    // after a 503 on an earlier lookup, and a stage whose cargo kind replayed nothing while its other kind replayed everything
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), COLD_LOG);

    // When the health is asked
    let r = health_wf(&github, "push");

    // Then the cold layer and the drift are findings, and neither the stored save nor the half-replayed stage is
    let out = last_json(&r);
    let kinds: Vec<&str> = out["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!((r.exit_code, kinds), (1, vec!["degraded", "drift"]), "{out}");
    let degraded = out["findings"][0]["detail"].as_str().unwrap();
    assert!(degraded.contains("unreachable"), "{out}");
    assert!(out["findings"][1]["detail"].as_str().unwrap().contains("2 of 10"), "{out}");
    assert_eq!(out["stages"][0], json!({ "job": "flow-tests", "stage": "flow-tests", "replayed": 6, "tasks": 10 }));
}

#[test]
fn the_cache_health_verb_does_not_fail_a_run_it_could_not_read() {
    // Given a GitHub API that is gone
    let opts = ci_opts()
        .env("GITHUB_TOKEN", TOKEN)
        .env("GITHUB_REPOSITORY", "o/r")
        .env("GITHUB_RUN_ID", "77")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_API_URL", "http://127.0.0.1:1")
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REF_NAME", "dev")
        .env("GITHUB_EVENT_NAME", "push")
        .env("WF_CI_LOG_CACHE", &log_cache())
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");

    // When the health is asked
    let r = run_wf(&["ci", "cache-health"], &opts);

    // Then the verdict says it could not be computed and the exit stays 0
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (0, json!(null)), "{}{}", r.stdout, r.stderr);
    assert!(out["reason"].is_string(), "{out}");
}

#[test]
fn the_live_comment_has_a_cache_health_section_naming_the_problems_and_a_healthy_run_says_so() {
    // Given a green run whose flow-tests job lost a write, and one whose cache was healthy
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    let summary = std::env::temp_dir().join(format!("ci-summary-health-{}", std::process::id()));
    let _ = std::fs::remove_file(&summary);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), SICK_LOG);

    // When wf reports
    let r = github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", summary.to_str().unwrap())]);

    // Then the comment has the section with each problem and the job it came from
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report = std::fs::read_to_string(&summary).unwrap();
    assert!(report.contains("### 🗄️ Cache health") && report.contains("problem(s)"), "{report}");
    assert!(report.contains("seeded 3 of 4") && report.contains("| rejected-write | flow-tests |") && report.contains("The shared build cache is off (WF_BLOB_CACHE=off) in 1 job"), "{report}");

    // And a healthy run says so
    let clean = std::env::temp_dir().join(format!("ci-summary-health-ok-{}", std::process::id()));
    let _ = std::fs::remove_file(&clean);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), HEALTHY_LOG);
    github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", clean.to_str().unwrap())]);
    let report = std::fs::read_to_string(&clean).unwrap();
    assert!(report.contains("🟢 Healthy") && report.contains("Nx replayed 5 of 5"), "{report}");
}

#[test]
fn a_labelled_pipeline_keeps_its_own_titled_comment_and_never_deletes_the_cis() {
    // Given the CI's live comment and an earlier run's comment of the labelled pipeline on the pull request
    let github = live_github(json!([
        bot(8000, "<!-- ci-live run:70 -->\n## 🟡 CI running"),
        bot(8001, "<!-- ci-live:github-hosted run:70 -->\n## 🟡 CI (GitHub-hosted) running"),
    ]));

    // When the labelled pipeline's run starts its live comment
    let r = github.ci_wf_with(&["live", "start"], &[("WF_CI_LABEL", "GitHub-hosted")]);

    // Then both older-head comments go (the PR keeps only the latest head's), and the new one carries the label in its marker and its title
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let mut deleted: Vec<String> = github.sent("DELETE").iter().map(|d| d.url.clone()).collect();
    deleted.sort();
    assert_eq!(deleted, ["/repos/o/r/issues/comments/8000", "/repos/o/r/issues/comments/8001"], "{:?}", github.server.requests());
    let created = github.sent("POST");
    assert_eq!(created.len(), 1);
    let body = posted_body(&created[0]);
    assert!(body.starts_with("<!-- ci-live:github-hosted run:77 -->"), "{body}");
    assert!(body.contains("## 🟡 CI (GitHub-hosted) running") || body.contains("CI (GitHub-hosted)"), "{body}");

    // and the CI's own run removes the older heads' comments of both flavors
    let github = live_github(json!([
        bot(8000, "<!-- ci-live run:70 -->\n## 🟡 CI running"),
        bot(8001, "<!-- ci-live:github-hosted run:70 -->\n## 🟡 CI (GitHub-hosted) running"),
    ]));
    let r = github.ci_wf(&["live", "start"]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let deleted: Vec<String> = github.sent("DELETE").iter().map(|d| d.url.clone()).collect();
    assert_eq!(deleted.len(), 2, "{:?}", github.server.requests());
}

#[test]
fn a_labelled_pipeline_reports_its_own_runners_numbers_instead_of_the_vms() {
    // Given a labelled run whose jobs left their sampled runner numbers in a directory
    let dir = std::env::temp_dir().join(format!("wf-runner-stats-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("stats-flow-tests.jsonl"),
        "{\"t\":1,\"cpu\":40,\"memUsedMb\":2000,\"memTotalMb\":7000,\"load1\":1.0}\n{\"t\":6,\"cpu\":90,\"memUsedMb\":3000,\"memTotalMb\":7000,\"load1\":2.5}\nnot json\n",
    )
    .unwrap();
    let github = live_github(json!([]));

    // When the run's live comment is built
    let r = github.ci_wf_with(&["live", "start"], &[("WF_CI_LABEL", "GitHub-hosted"), ("WF_CI_RUNNER_STATS_DIR", dir.to_str().unwrap())]);

    // Then the comment has the runners table with the job's averages and peaks, and no VM history
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let created = github.sent("POST");
    assert_eq!(created.len(), 1, "{:?}", github.server.requests());
    let body = posted_body(&created[0]);
    assert!(body.contains("### 🖥️ Runners during the run"), "{body}");
    assert!(body.contains("| 🔴 flow-tests | 65% | **90%** | 2.5 GB | **3.0 GB** (43%) | 2.5 | 2 |"), "{body}");
    assert!(!body.contains("VM during the run"), "{body}");
    std::fs::remove_dir_all(&dir).ok();
}

/// The jobs of a run of the shared workflow: the Actions API names each `<caller job> / <job>`, and a sharded one `(i/n)` after its name.
fn called_run() -> Value {
    let steps = json!([{ "name": "Set up job", "conclusion": "success" }, { "name": "Flow tests", "conclusion": "failure" }]);
    json!([
        job(500, "run / plan", "success", "2026-10-23T10:00:20Z", json!([])),
        job(501, "run / flow-tests (2/4)", "failure", "2026-10-23T10:03:05Z", steps),
        job(502, "run / deploy (vm-ops-api)", "skipped", "2026-10-23T10:03:06Z", json!([])),
        job(503, "run / report", "failure", "2026-10-23T10:03:07Z", json!([])),
        job(504, "run / budget", "failure", "2026-10-23T10:03:07Z", json!([]))
    ])
}

#[test]
fn the_report_of_a_called_workflow_names_a_job_without_the_caller_prefix_and_leaves_its_own_report_jobs_out() {
    // Given a run whose jobs the API names `run / <job>`, a sharded job failed, and the report and budget jobs of the run are red too
    let github = Github::start();
    github.serve(&called_run(), json!([]), created(), updated());

    // When the comment is previewed
    let r = github.comment_wf(&["--dry-run"]);

    // Then the failed job is the sharded one under its own name, and neither report job counts as a failure
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["failedJobs"].clone()), (0, json!(["flow-tests (2/4)"])), "{}{}", r.stdout, r.stderr);
    let body = out["body"].as_str().unwrap();
    assert!(body.contains("flow-tests (2/4)") && body.contains("`deploy (vm-ops-api)`"), "{body}");
    assert!(!body.contains("run / "), "the caller's prefix is not shown: {body}");
}

#[test]
fn the_live_comment_of_a_called_workflow_shows_a_sharded_job_by_its_own_name() {
    // Given a run whose running job is shard 2 of 4 of flow-tests, named behind the caller's job
    let mut jobs = live_jobs();
    jobs[0]["name"] = json!("run / preflight");
    jobs[1]["name"] = json!("run / flow-tests (2/4)");
    jobs[2]["name"] = json!("run / clis");
    let github = live_github_of(json!([]), jobs);

    // When the live comment starts
    let r = github.ci_wf(&["live", "start"]);

    // Then it names the running shard and the waiting job without the prefix
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let body = posted_body(&github.sent("POST")[0]);
    assert!(body.contains("flow-tests (2/4)") && body.contains("in: Flow tests") && body.contains("clis"), "{body}");
    assert!(!body.contains("run / "), "{body}");
}

#[test]
fn the_audit_of_a_called_workflow_judges_a_job_by_its_own_name_and_never_judges_its_report_jobs() {
    // Given a run of the shared workflow where a sharded job waited fifteen minutes for a runner, and the run's report job waited as long
    let mut waited = timed_job(500, "run / flow-tests (2/4)", "success", 1500);
    waited["started_at"] = json!(iso(BASE + 900));
    let mut report = timed_job(501, "run / report", "success", 1500);
    report["started_at"] = json!(iso(BASE + 900));
    let github = audit_github_of(&[waited, report], "ok\n", &[]);

    // When the audit judges the run
    let r = Audit::new("called", &[], "", json!({})).run(&github, true);

    // Then only the sharded job's wait is reported, under its own name
    let out = last_json(&r);
    let waits: Vec<&Value> = out["findings"].as_array().unwrap().iter().filter(|b| b["kind"] == "runner-wait").collect();
    assert_eq!(waits.len(), 1, "{out}");
    assert_eq!(waits[0]["subject"], "flow-tests (2/4)", "{out}");
}

// ---- one comment per workflow: converging duplicates and pruning old heads ----

const WORKFLOW_LABELS: [(&str, &str); 6] = [
    ("ci-hosted-backend-core.yml", "backend_core+lint+pre-release-testing"),
    ("ci-hosted-mobile-core.yml", "mobile_core+lint+pre-release-testing"),
    ("ci-hosted-cli-core.yml", "cli_core+lint+pre-release-testing"),
    ("ci-hosted-cli-build.yml", "cli_build+publish+release-testing"),
    ("ci-hosted-backend-build.yml", "backend_build+deploy+release-testing"),
    ("ci-hosted-mobile-build.yml", "mobile_build+publish+release-testing"),
];

/// A workspace whose `.github/workflows` hold the labels of the current workflows.
fn workspace_with_labels() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("ci-workspace-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let workflows = dir.join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    for (file, label) in WORKFLOW_LABELS {
        std::fs::write(workflows.join(file), format!("jobs:\n  ci:\n    with:\n      label: {label}\n")).unwrap();
    }
    dir.display().to_string()
}

/// A pull request at the run's head with `comments`, and the heads the runs behind them were for.
fn pruning(comments: Value, runs: &[(u64, &str)]) -> Github {
    let github = live_github(comments.clone());
    let mut table: Vec<(String, MockReply)> = vec![
        ("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1".into(), MockReply::json(200, json!([{ "number": 7, "head": { "sha": SHA } }]))),
        (format!("GET {PR_COMMENTS}?per_page=100&page=1"), MockReply::json(200, comments.clone())),
        ("GET /repos/o/r/pulls/7".into(), MockReply::json(200, json!({ "head": { "sha": SHA } }))),
        ("GET /repos/o/r/pulls/7/commits?per_page=100&page=1".into(), MockReply::json(200, json!([]))),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1".into(), MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(500, "preflight", "success", 12)] }))),
        ("GET /repos/o/r/actions/jobs/500/logs".into(), MockReply::text(200, "ok\n")),
        (format!("POST {PR_COMMENTS}"), MockReply::json(201, json!({ "id": 9100 }))),
        ("PATCH /repos/o/r/issues/comments/9100".into(), MockReply::json(200, json!({ "id": 9100 }))),
    ];
    for c in comments.as_array().unwrap() {
        let id = c["id"].as_u64().unwrap();
        table.push((format!("DELETE /repos/o/r/issues/comments/{id}"), MockReply::json(204, json!({}))));
        table.push((format!("PATCH /repos/o/r/issues/comments/{id}"), MockReply::json(200, json!({ "id": id }))));
    }
    for (run, sha) in runs {
        table.push((format!("GET /repos/o/r/actions/runs/{run}"), MockReply::json(200, json!({ "head_sha": sha }))));
    }
    github.server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());
    github
}

fn deleted(github: &Github) -> Vec<String> {
    let mut urls: Vec<String> = github.sent("DELETE").into_iter().map(|d| d.url.rsplit('/').next().unwrap().to_string()).collect();
    urls.sort();
    urls
}

#[test]
fn two_comments_of_the_same_run_become_one_and_the_lowest_id_is_edited() {
    // Given the pull request carries two bot comments with this run's marker (two jobs created one each at the same time)
    let github = pruning(json!([bot(31, "<!-- ci-live run:77 -->\nb"), bot(30, "<!-- ci-live run:77 -->\na")]), &[]);

    // When wf ci live runs
    let r = github.ci_wf(&["live"]);

    // Then exactly one PATCH goes to the lowest id, one DELETE to the other, and nothing is created
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(github.sent("PATCH").iter().map(|p| p.url.clone()).collect::<Vec<_>>(), ["/repos/o/r/issues/comments/30"]);
    assert_eq!(deleted(&github), ["31"]);
    assert!(github.sent("POST").is_empty(), "{:?}", github.server.requests());
}

#[test]
fn a_create_that_raced_another_writer_deletes_its_own_extra_comment() {
    // Given the first listing is empty and the listing after the create shows ids 10 and 11 with this run's marker
    let github = pruning(json!([]), &[]);
    let marker = "<!-- ci-live run:77 -->\nx";
    let jobs = MockReply::json(200, json!({ "total_count": 3, "jobs": live_jobs() }));
    github.server.queue(
        vec![
            MockReply::json(200, json!([{ "number": 7, "head": { "sha": SHA } }])),
            jobs,
            MockReply::text(200, "ok\n"),
            MockReply::json(200, json!({ "name": "flow" })),
            MockReply::json(200, json!([])),
            MockReply::json(201, json!({ "id": 11 })),
            MockReply::json(200, json!([bot(10, marker), bot(11, marker)])),
            MockReply::json(204, json!({})),
        ],
        false,
    );

    // When the live comment is created
    let r = github.ci_wf(&["live", "start"]);

    // Then id 11 is deleted and id 10 stays
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    assert_eq!((out["action"].clone(), out["commentId"].clone()), (json!("created"), json!(10)), "{} {:?}", r.stdout, github.server.requests().iter().map(|q| format!("{} {}", q.method, q.url)).collect::<Vec<_>>());
    assert_eq!(deleted(&github), ["11"], "{:?}", github.server.requests());
}

#[test]
fn an_old_head_comment_of_the_same_workflow_is_deleted_and_the_current_one_stays() {
    // Given this run's comment and an older head's comment of the same workflow, both the bot's
    let github = pruning(json!([bot(3, "<!-- ci-live run:70 -->\nold"), bot(4, "<!-- ci-live run:77 -->\nmine")]), &[(70, OLD)]);
    // When wf ci live runs
    let r = github.ci_wf(&["live"]);
    // Then the old one is deleted and this run's is edited
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(deleted(&github), ["3"]);
    assert_eq!(github.sent("PATCH").len(), 1);
}

#[test]
fn a_comment_of_a_retired_label_for_an_old_head_is_deleted() {
    // Given the six-label era workflows and a comment of the label the CI used before, for an older head
    let github = pruning(json!([bot(3, "<!-- ci-live:github-hosted run:70 -->\nold"), bot(5, "<!-- ci-live run:71 -->\nolder still")]), &[(70, OLD), (71, OLD)]);
    let workspace = workspace_with_labels();

    // When the workflow labelled "core backend services" runs
    let r = github.ci_wf_with(&["live"], &[("WF_CI_LABEL", "GitHub-hosted core backend services"), ("GITHUB_WORKSPACE", &workspace)]);

    // Then both retired flavors' old comments go
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(deleted(&github), ["3", "5"]);
}

#[test]
fn an_older_head_comment_of_a_current_workflow_that_did_not_run_again_is_deleted() {
    // Given an older head's comment of another current workflow (path-filtered out at the new head, so it never replaces it)
    let github = pruning(json!([bot(3, "<!-- ci-live:github-hosted-core-cli run:70 -->\nold")]), &[(70, OLD)]);
    let workspace = workspace_with_labels();

    // When the backend workflow runs
    let r = github.ci_wf_with(&["live"], &[("WF_CI_LABEL", "GitHub-hosted core backend services"), ("GITHUB_WORKSPACE", &workspace)]);

    // Then the PR carries only the latest head's comments, so that one goes
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(deleted(&github), ["3"], "{:?}", github.server.requests());
}

#[test]
fn a_run_that_a_newer_run_of_its_workflow_has_passed_writes_nothing() {
    // Given the pull request already carries the comment of run 78 of this workflow
    let github = pruning(json!([bot(3, "<!-- ci-live run:78 -->\nnewer")]), &[]);

    // When the older run 77 updates its live comment
    let r = github.ci_wf(&["live"]);

    // Then it neither creates, edits nor deletes anything
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(github.sent("POST").is_empty() && github.sent("PATCH").is_empty() && github.sent("DELETE").is_empty(), "{:?}", github.server.requests());
}

#[test]
fn a_rerun_on_another_runner_updates_the_commit_comment_instead_of_adding_a_second() {
    // Given a red run with no pull request whose commit already carries this run's comment, written by another runner
    let github = Github::start();
    github.without_pr();
    github.serve(&red_run(), json!([bot(9001, "<!-- ci-comment run:77 runner:other-runner -->\nold")]), created(), updated());
    github.server.clear();

    // When wf comments from this runner
    let r = github.comment_wf(&[]);

    // Then the existing comment is edited and none is created
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((github.sent("PATCH").len(), github.sent("POST").iter().filter(|p| p.url == COMMENTS).count()), (1, 0), "{:?}", github.server.requests());
}

#[test]
fn a_human_or_non_github_actions_comment_that_quotes_a_marker_is_never_deleted() {
    // Given comments that look like old CI comments but are not the Actions bot's, or whose first line is not exactly a marker
    let marker = "<!-- ci-live run:70 -->";
    let github = pruning(
        json!([
            { "id": 3, "body": format!("{marker}\nhuman"), "user": { "login": "octocat", "type": "User" } },
            { "id": 4, "body": format!("{marker}\nother bot"), "user": { "login": "dependabot[bot]", "type": "Bot" } },
            bot(5, &format!("quoting\n{marker}")),
            bot(6, &format!("{marker} extra text\nbody")),
        ]),
        &[(70, OLD)],
    );
    // When wf ci live runs
    let r = github.ci_wf(&["live"]);
    // Then nothing is deleted
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(deleted(&github).is_empty(), "{:?}", github.server.requests());
}

#[test]
fn a_comment_whose_run_cannot_be_resolved_is_kept() {
    // Given an old-looking comment whose run lookup answers 404
    let github = pruning(json!([bot(3, "<!-- ci-live run:70 -->\nold")]), &[]);
    let r = github.ci_wf(&["live"]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(deleted(&github).is_empty(), "{:?}", github.server.requests());
}

#[test]
fn dry_run_lists_what_it_would_delete_and_sends_no_delete() {
    let github = pruning(json!([bot(3, "<!-- ci-live run:70 -->\nold"), bot(4, "<!-- ci-live run:77 -->\nmine")]), &[(70, OLD)]);
    // When wf ci prune runs with --dry-run
    let r = github.ci_wf(&["prune", "--dry-run"]);
    // Then it lists the deletion and sends none
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["dryRun"].clone(), out["deleted"][0]["id"].clone()), (0, json!(true), json!(3)), "{}{}", r.stdout, r.stderr);
    assert!(deleted(&github).is_empty() && github.sent("POST").is_empty() && github.sent("PATCH").is_empty(), "{:?}", github.server.requests());
    // and without the flag it deletes it
    let r = github.ci_wf(&["prune"]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(deleted(&github), ["3"]);
}

#[test]
fn a_superseded_run_posts_nothing() {
    // Given the pull request's head moved past this run's sha
    let github = pruning(json!([]), &[]);
    github.server.routes(&[("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1", MockReply::json(200, json!([{ "number": 7, "head": { "sha": OLD } }])))]);
    // When wf ci live runs
    let r = github.ci_wf(&["live", "start"]);
    // Then nothing is posted, edited or deleted, and it says why
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["posted"].clone()), (0, json!(false)), "{}{}", r.stdout, r.stderr);
    assert!(out["reason"].as_str().unwrap().starts_with("superseded: head moved to aaaaaaa"), "{out}");
    assert!(github.sent("POST").is_empty() && github.sent("PATCH").is_empty() && github.sent("DELETE").is_empty());
}

#[test]
fn a_red_run_with_an_open_pr_posts_no_commit_comment_and_one_without_a_pr_posts_it() {
    // Given a red run on a branch with an open pull request
    let github = Github::start();
    github.serve(&red_run(), json!([]), created(), updated());
    // When wf comments
    let r = github.comment_wf(&[]);
    // Then the pull request carries the report and the commit gets none
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(github.sent("POST").iter().all(|p| p.url == PR_COMMENTS), "{:?}", github.server.requests());
    assert_eq!(github.sent("POST").len(), 1);
    // and with no pull request the commit gets it
    github.without_pr();
    github.serve(&red_run(), json!([]), created(), updated());
    github.server.clear();
    let r = github.comment_wf(&[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(github.sent("POST").iter().filter(|p| p.url == COMMENTS).count(), 1);
}

#[test]
fn the_cap_stops_at_25_deletions() {
    // Given 30 comments of older heads
    let comments: Vec<Value> = (0..30).map(|i| bot(100 + i, &format!("<!-- ci-live run:{} -->\nold", 200 + i))).collect();
    let runs: Vec<(u64, &str)> = (0..30).map(|i| (200 + i, OLD)).collect();
    let github = pruning(json!(comments), &runs);
    // When wf ci prune runs
    let r = github.ci_wf(&["prune"]);
    // Then 25 are deleted and the rest is reported as capped
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["deleted"].as_array().unwrap().len(), github.sent("DELETE").len()), (0, 25, 25), "{}{}", r.stdout, r.stderr);
    assert!(out["errors"].to_string().contains("cap reached"), "{out}");
}

// ---- the job table, the runners, the cache health and the e2e units ----

fn skipped_job(id: u64, name: &str) -> Value {
    json!({ "id": id, "name": name, "status": "completed", "conclusion": "skipped", "started_at": null, "completed_at": null, "created_at": iso(BASE), "html_url": "", "steps": [] })
}

fn dry_body(github: &Github, extra: &[(&str, &str)]) -> String {
    let r = github.comment_wf_with(&["--dry-run"], extra);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    last_json(&r)["body"].as_str().unwrap_or_else(|| panic!("{}{}", r.stdout, r.stderr)).to_string()
}

#[test]
fn a_run_whose_only_unpassed_job_was_replaced_while_pending_reads_as_superseded_not_failed() {
    // Given a finished run of which core-tests (2/3) was cancelled one second after its recorded start, a pending job a newer run of the same inputs replaced
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "core-tests (2/3)", "cancelled", "2026-10-23T10:00:01Z", json!([]))]);
    github.serve(&jobs, json!([]), created(), updated());

    // When the report is built
    let body = dry_body(&github, &[]);

    // Then the title says it was superseded by a newer run of the same inputs, neither failed nor passed, and the job keeps its cancelled mark
    let title = body.lines().nth(1).unwrap_or_default();
    assert!(title.contains("superseded by a newer run of the same inputs") && !title.contains("failed") && !title.contains("passed"), "{body}");
    assert!(body.contains("🚫 1 replaced while pending by a newer run") && body.contains("🚫"), "{body}");
}

#[test]
fn a_cancelled_job_that_never_started_is_a_cancelled_run_not_a_superseded_one() {
    // Given a run a person cancelled whole, so its queued job ended cancelled with no start or end time
    let github = Github::start();
    let mut queued = job(501, "core-tests (2/3)", "cancelled", "2026-10-23T10:00:01Z", json!([]));
    queued["started_at"] = Value::Null;
    queued["completed_at"] = Value::Null;
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), queued]);
    github.serve(&jobs, json!([]), created(), updated());

    // When the report is built
    let body = dry_body(&github, &[]);

    // Then no newer run is blamed and the title says cancelled
    let title = body.lines().nth(1).unwrap_or_default();
    assert!(title.contains("cancelled") && !title.contains("superseded") && !title.contains("passed"), "{body}");
}

#[test]
fn a_job_killed_while_running_beside_a_replaced_one_is_not_a_passed_run() {
    // Given a finished run with one job replaced while pending and one cancelled after 90 s of running
    let github = Github::start();
    let jobs = json!([
        job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])),
        job(501, "core-tests (2/3)", "cancelled", "2026-10-23T10:00:01Z", json!([])),
        job(502, "core-tests (1/3)", "cancelled", "2026-10-23T10:01:30Z", json!([]))
    ]);
    github.serve(&jobs, json!([]), created(), updated());

    // When the report is built
    let body = dry_body(&github, &[]);

    // Then the title is neither passed nor superseded
    let title = body.lines().nth(1).unwrap_or_default();
    assert!(title.contains("cancelled") && !title.contains("superseded") && !title.contains("passed") && !title.contains("✅"), "{body}");
}

#[test]
fn a_replaced_pending_job_beside_a_red_job_is_still_a_failed_run() {
    // Given a finished run with one replaced pending job and one job that failed
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "core-tests (2/3)", "cancelled", "2026-10-23T10:00:01Z", json!([])), job(502, "core-tests (1/3)", "failure", "2026-10-23T10:03:00Z", json!([]))]);
    github.serve(&jobs, json!([]), created(), updated());

    // When the report is built
    let body = dry_body(&github, &[]);

    // Then the run is failed
    let title = body.lines().nth(1).unwrap_or_default();
    assert!(title.contains("failed") && !title.contains("superseded"), "{body}");
}

#[test]
fn the_job_table_lists_only_jobs_that_ran_and_names_shards_as_2_of_4() {
    // Given a run whose shared graph has three skipped jobs, one with a raw template name, and a shard that ran
    let template = "integration-tests${{ (needs.plan.outputs.shard_count > 1 && format(' ({0}/{1})', matrix.shard, 4)) || '' }}";
    let jobs = json!([
        timed_job(500, "plan", "success", 20),
        timed_job(501, "integration-tests (2/4)", "success", 300),
        skipped_job(502, template),
        skipped_job(503, "deploy (vm-ops-api)"),
        skipped_job(504, "android-e2e-tests"),
    ]);
    let github = audit_github_of(jobs.as_array().unwrap(), "ok\n", &[]);
    github.server.routes(&[
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 5, "jobs": jobs }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, "ok\n")),
        ("GET /repos/o/r/actions/jobs/501/logs", MockReply::text(200, "ok\n")),
    ]);

    // When the report is built
    let body = dry_body(&github, &[]);

    // Then the shard is named by its own name, no template and no skipped job's name appears, and one line counts them
    assert!(body.contains("integration-tests (2/4)"), "{body}");
    assert!(!body.contains("${{") && !body.contains("deploy (vm-ops-api)") && !body.contains("android-e2e-tests"), "{body}");
    assert!(body.contains("⏭️ 3 jobs of the shared pipeline did not apply to this workflow"), "{body}");
}

#[test]
fn a_pending_job_is_a_queued_row_and_is_counted_while_the_run_is_live() {
    // Given a live run with one job that has not started
    let github = live_github(json!([]));
    // When the live comment is built
    let r = github.ci_wf(&["live", "start"]);
    let body = posted_body(&github.sent("POST")[0]);
    // Then the job has its own row marked queued, is counted in the headline, and no separate waiting line names it
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(body.lines().any(|l| l.starts_with("| ⏸️ 3 |") && l.contains("clis") && l.ends_with("⏸️ queued |")), "{body}");
    assert!(body.contains("1 queued") && !body.contains("waiting:"), "{body}");
}

#[test]
fn the_runner_table_states_the_sampled_cores_and_memory_not_two_cores() {
    let dir = std::env::temp_dir().join(format!("wf-runner-cores-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("stats-flow-tests.jsonl"), "{\"t\":1,\"cpu\":40,\"memUsedMb\":2000,\"memTotalMb\":15900,\"cores\":4,\"load1\":1.0,\"iowaitPct\":3,\"psiCpu\":5,\"psiMem\":0,\"psiIo\":1,\"swapUsedMb\":0}\n{\"t\":6,\"cpu\":95,\"memUsedMb\":3000,\"memTotalMb\":15900,\"load1\":2.5,\"iowaitPct\":9,\"psiCpu\":25,\"psiMem\":1,\"psiIo\":4,\"swapUsedMb\":512}\n").unwrap();
    let github = live_github(json!([]));
    let extra = [("WF_CI_LABEL", "GitHub-hosted"), ("WF_CI_RUNNER_STATS_DIR", dir.to_str().unwrap())];

    // When the live comment is built
    let r = github.ci_wf_with(&["live", "start"], &extra);
    let body = posted_body(&github.sent("POST")[0]);

    // Then the machine is the sampled one, with the p95, iowait, swap, pressure, idle and saturated columns
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(body.contains("4 vCPU / 16 GB") && !body.contains("2-core"), "{body}");
    assert!(body.contains("| 🔴 flow-tests | 68% | **95%** | 2.5 GB | **3.0 GB** (19%) | 2.5 | 2 | 95% | 6% | 0.5 GB | 25/1/4% | 0 | 5 |"), "{body}");

    // and a sample with no cores says so instead of naming a constant
    std::fs::write(dir.join("stats-flow-tests.jsonl"), "{\"t\":1,\"cpu\":40,\"memUsedMb\":2000,\"memTotalMb\":7000,\"load1\":1.0}\n").unwrap();
    let github = live_github(json!([]));
    github.ci_wf_with(&["live", "start"], &extra);
    let body = posted_body(&github.sent("POST")[0]);
    assert!(body.contains("cores not sampled") && !body.contains("2-core") && body.contains("| 🟢 flow-tests |"), "{body}");
    std::fs::remove_dir_all(&dir).ok();
}

fn cache_log(stage: &str) -> String {
    format!(
        "2026-10-23T10:00:07.0000000Z ts=1 level=info source=ci-cache msg=\"nx.stage.summary\" stage={stage} ran=3 replayed=0\n2026-10-23T10:00:08.0000000Z ts=2 level=warn source=ci-cache msg=\"cache.shim.disabled\" reason=\"WF_BLOB_CACHE=off\"\n"
    )
}

#[test]
fn eleven_jobs_that_replayed_nothing_are_one_row_with_the_job_count_and_the_disabled_shim_is_one_note() {
    // Given eleven jobs whose logs say the stage replayed nothing and the shim was off
    let jobs: Vec<Value> = (0..11).map(|i| timed_job(500 + i, &format!("flow-tests ({}/11)", i + 1), "success", 100)).collect();
    let github = Github::start();
    let mut routes = vec![("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1".to_string(), MockReply::json(200, json!({ "total_count": 11, "jobs": jobs })))];
    for i in 0..11 {
        routes.push((format!("GET /repos/o/r/actions/jobs/{}/logs", 500 + i), MockReply::text(200, &cache_log("flow-tests"))));
    }
    github.server.routes(&routes.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());

    // When the push gate is off for the hosted stages
    let off = dry_body(&github, &[("GITHUB_EVENT_NAME", "push"), ("GITHUB_REF_NAME", "dev"), ("WF_CI_PUSH_GATE", "off")]);
    // Then there is no replayed-nothing row and the shim is one note with the count
    assert!(!off.contains("replayed-nothing"), "{off}");
    assert_eq!(off.matches("The shared build cache is off").count(), 1, "{off}");
    assert!(off.contains("in 11 jobs"), "{off}");

    // and with the gate on, one row with the job count
    let on = dry_body(&github, &[("GITHUB_EVENT_NAME", "push"), ("GITHUB_REF_NAME", "dev"), ("WF_CI_PUSH_GATE", "on")]);
    assert_eq!(on.matches("| replayed-nothing |").count(), 1, "{on}");
    assert!(on.contains("| replayed-nothing | 11 jobs |"), "{on}");
}

fn results_file(units: &[(&str, &str)]) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!("e2e-results-{}-{}.json", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let runs: Vec<Value> = units.iter().map(|(unit, verdict)| json!({ "sha": SHA, "platform": "web", "test": "login", "unit": unit, "verdict": verdict, "durationS": 2.5, "publishedAt": "2026-10-23T10:00:00Z" })).collect();
    std::fs::write(&path, json!({ "runs": runs }).to_string()).unwrap();
    path.display().to_string()
}

#[test]
fn the_e2e_units_are_one_collapsed_block_with_counts_and_failures_first_and_an_all_green_block_is_closed() {
    let github = Github::start();
    let jobs = [timed_job(500, "plan", "success", 20)];
    github.server.routes(&[
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": jobs }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, "ok\n")),
    ]);
    // Given 298 passing units and 2 failing ones
    let mut units: Vec<(String, &str)> = (0..298).map(|i| (format!("unit-{i:03}"), "pass")).collect();
    units.extend([("broken-a".to_string(), "fail"), ("broken-b".to_string(), "fail")]);
    let refs: Vec<(&str, &str)> = units.iter().map(|(u, v)| (u.as_str(), *v)).collect();
    let file = results_file(&refs);

    // When the report is built
    let body = dry_body(&github, &[("WF_CI_RESULTS", &file)]);

    // Then one open block carries the counts, the failing rows come first and the comment is small
    assert_eq!(body.matches("<summary>🧪 E2E units").count(), 1, "{body}");
    assert!(body.contains("298 passed, 2 failed") && body.contains("<details open><summary>🧪"), "{body}");
    assert!(body.find("broken-a").unwrap() < body.find("unit-000").unwrap(), "{body}");
    assert!(body.len() < 60_000, "{}", body.len());

    // and an all-green block is closed
    let file = results_file(&[("a", "pass"), ("b", "pass")]);
    let body = dry_body(&github, &[("WF_CI_RESULTS", &file)]);
    assert!(body.contains("<details><summary>🧪 E2E units: 2 passed, 0 failed</summary>"), "{body}");
}

// ---- the telemetry in the report ----

#[test]
fn the_report_has_a_headline_with_wall_runner_seconds_peak_jobs_efficiency_the_critical_path_and_the_fixed_cost() {
    // Given three jobs whose intervals give a 600 s wall, 1500 s of runner time and at most three at once
    let at = |name: &str, id: u64, from: i64, to: i64| {
        let mut j = job(id, name, "success", &iso(BASE + to), json!([{ "name": "Run ./.github/actions/setup-job", "conclusion": "success", "started_at": iso(BASE + from), "completed_at": iso(BASE + from + 40) }]));
        (j["created_at"], j["started_at"]) = (json!(iso(BASE + from - 45)), json!(iso(BASE + from)));
        j
    };
    let jobs = json!([at("plan", 500, 0, 100), at("core-tests (1/2)", 501, 100, 600), at("core-tests (2/2)", 502, 100, 500), at("clis", 503, 100, 500)]);
    let github = Github::start();
    github.server.routes(&[
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 4, "jobs": jobs }))),
        ("GET /repos/o/r/actions/runs/77", MockReply::json(200, json!({ "name": "CI", "run_started_at": iso(BASE) }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, "ok\n")),
        ("GET /repos/o/r/actions/jobs/501/logs", MockReply::text(200, "ok\n")),
        ("GET /repos/o/r/actions/jobs/502/logs", MockReply::text(200, "ok\n")),
        ("GET /repos/o/r/actions/jobs/503/logs", MockReply::text(200, "ok\n")),
    ]);

    // When the report is built
    let body = dry_body(&github, &[("WF_CI_CORE_SHARD_KEYS", "[\"\",\"k1\",\"k2\"]"), ("GITHUB_WORKFLOW", "CI core")]);

    // Then the headline carries the profile's numbers, and the verdict names the critical path, the wait and the fixed cost
    assert!(body.contains("wall 10m 00s · runner time 23m 20s"), "{body}");
    assert!(body.contains("peak 3 / avg 2.3 jobs at once · parallel efficiency 78%") && body.contains("4 jobs ran"), "{body}");
    assert!(body.contains("The critical path is plan &gt; core-tests (1/2) (10m 00s, 100% of the wall time)"), "{body}");
    assert!(body.contains("Jobs waited 3m 00s for a runner in total, 45s at worst"), "{body}");
    assert!(body.contains("| setup-job | 2m 40s | 4 | 40s |"), "{body}");
    assert!(body.contains("core-tests (1/2) queued 45s; its concurrency group is `hosted-core-tests-CI core-1-k1`") && body.contains("plan waited 45s for a runner"), "{body}");
}

#[test]
fn the_comment_stays_under_the_cap_with_every_section_present() {
    // Given a live run with the sampler's files
    let github = live_github(json!([]));
    let r = github.ci_wf(&["live", "start"]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let body = posted_body(&github.sent("POST")[0]);
    assert!(body.len() < 60_000 && body.contains("### Jobs"), "{}", body.len());
}

#[test]
fn a_job_that_still_runs_with_a_red_step_is_a_failing_now_row_at_the_top_of_the_live_comment() {
    // Given a run whose flow-tests job is still running with its test step already red, beside a passed plan job and a job that runs without failure
    let github = Github::start();
    let mut red = job(501, "flow-tests (2/6)", "failure", "2026-10-23T10:03:05Z", json!([{ "name": "Set up job", "conclusion": "success" }, { "name": "Flow tests", "conclusion": "failure" }, { "name": "Upload", "conclusion": null }]));
    red["status"] = json!("in_progress");
    red["conclusion"] = Value::Null;
    red["completed_at"] = Value::Null;
    let mut well = job(505, "integration-tests (1/4)", "success", "2026-10-23T10:03:05Z", json!([{ "name": "Integration", "conclusion": null }]));
    well["status"] = json!("in_progress");
    well["conclusion"] = Value::Null;
    well["completed_at"] = Value::Null;
    github.serve(&json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), red, well]), json!([]), created(), updated());

    // When the comment is previewed while the run goes on
    let preview = github.comment_wf(&["--dry-run"]);

    // Then the body says the run is going with a failure, names the job and the red step as failing now, and leaves the healthy running job out of it
    let body = last_json(&preview)["body"].as_str().unwrap_or_default().to_string();
    assert_eq!(preview.exit_code, 0, "{}{}", preview.stdout, preview.stderr);
    assert!(body.contains("running, a job failed"), "{body}");
    assert!(body.contains("### 🟠 flow-tests (2/6) failing now") && body.contains("step **Flow tests** is red, the job still runs"), "{body}");
    assert!(!body.contains("integration-tests (1/4) failing now"), "{body}");
    assert!(body.find("failing now") < body.find("### Jobs"), "{body}");
}

const DB_KEY: &str = "0707070707070707070707070707070707070707070707070707070707070707";

/// A sealed CI database holding the profile and one job of each run `(run, workflow, wall, queue, ts, conclusion)`, made the way the report job makes it: a job-local file merged in.
fn sealed_history(dir: &std::path::Path, runs: &[(String, &str, f64, f64, i64, &str)]) -> String {
    let local = dir.join("rows.sqlite");
    let conn = rusqlite::Connection::open(&local).unwrap();
    conn.execute_batch(
        "CREATE TABLE CI_RUN_PROFILE (RUN_ID TEXT NOT NULL PRIMARY KEY, WORKFLOW TEXT, WALL_S REAL, RUNNER_S REAL, PEAK_JOBS INTEGER, AVG_JOBS REAL, QUEUE_S_TOTAL REAL,
           QUEUE_S_MAX REAL, JOBS INTEGER, CRITICAL_PATH_JOB TEXT, CRITICAL_PATH_S REAL, PARALLEL_EFFICIENCY REAL);
         CREATE TABLE CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL, SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL,
           CONCLUSION TEXT, PRIMARY KEY (RUN_ID, JOB));",
    )
    .unwrap();
    for (run, workflow, wall, queue, ts, conclusion) in runs {
        conn.execute("INSERT INTO CI_RUN_PROFILE VALUES (?1, ?2, ?3, ?3, 2, 1.0, ?4, ?4, 2, 'plan', ?3, 0.5)", rusqlite::params![run, workflow, wall, queue]).unwrap();
        conn.execute("INSERT INTO CI_JOBS VALUES (?1, 'abc', ?2, 'push', 'plan', 5, 0, ?3)", rusqlite::params![run, ts, conclusion]).unwrap();
    }
    drop(conn);
    let sealed = dir.join("ci.sealed").display().to_string();
    let merged = run_wf(&["ci", "db", "merge", "--sealed-file", &sealed, &local.display().to_string()], &RunOpts::default().env("CI_DB_KEY", DB_KEY));
    assert_eq!(merged.exit_code, 0, "{}{}", merged.stdout, merged.stderr);
    sealed
}

/// A run `(run, workflow, wall, ts, jobs as (name, seconds))`.
type JobRun = (String, &'static str, f64, i64, Vec<(&'static str, i64)>);

/// A sealed CI database of earlier successful runs `(run, workflow, wall, ts, jobs as (name, seconds))`, each job a successful row; a job of 0 s is stored as skipped.
fn sealed_jobs(dir: &std::path::Path, runs: &[JobRun]) -> String {
    let local = dir.join("job-rows.sqlite");
    let conn = rusqlite::Connection::open(&local).unwrap();
    conn.execute_batch(
        "CREATE TABLE CI_RUN_PROFILE (RUN_ID TEXT NOT NULL PRIMARY KEY, WORKFLOW TEXT, WALL_S REAL, RUNNER_S REAL, PEAK_JOBS INTEGER, AVG_JOBS REAL, QUEUE_S_TOTAL REAL,
           QUEUE_S_MAX REAL, JOBS INTEGER, CRITICAL_PATH_JOB TEXT, CRITICAL_PATH_S REAL, PARALLEL_EFFICIENCY REAL);
         CREATE TABLE CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL, SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL,
           CONCLUSION TEXT, PRIMARY KEY (RUN_ID, JOB));",
    )
    .unwrap();
    for (run, workflow, wall, ts, jobs) in runs {
        conn.execute("INSERT INTO CI_RUN_PROFILE VALUES (?1, ?2, ?3, ?3, 2, 1.0, 0.0, 0.0, 2, 'plan', ?3, 0.5)", rusqlite::params![run, workflow, wall]).unwrap();
        for (name, secs) in jobs {
            let conclusion = if *secs == 0 { "skipped" } else { "success" };
            conn.execute("INSERT INTO CI_JOBS VALUES (?1, 'abc', ?2, 'push', ?3, ?4, 0, ?5)", rusqlite::params![run, ts, name, secs, conclusion]).unwrap();
        }
    }
    drop(conn);
    let sealed = dir.join("jobs.sealed").display().to_string();
    let merged = run_wf(&["ci", "db", "merge", "--sealed-file", &sealed, &local.display().to_string()], &RunOpts::default().env("CI_DB_KEY", DB_KEY));
    assert_eq!(merged.exit_code, 0, "{}{}", merged.stdout, merged.stderr);
    sealed
}

/// The `line [..]` rows of the Trend chart, one per series, in the order they are drawn.
fn chart_lines(body: &str) -> Vec<String> {
    body.lines().filter_map(|l| l.trim().strip_prefix("line [")).map(|l| l.trim_end_matches(']').to_string()).collect()
}

/// Run 77 of `workflow`: a plan job of 100 s, then core-tests of 600 s whose longest step is 550 s, and no wait for a runner.
fn trend_run(github: &Github, workflow: &str) {
    let at = |s: i64| iso(BASE + s);
    let mut plan = job(500, "plan", "success", &at(100), json!([]));
    (plan["created_at"], plan["started_at"]) = (json!(at(0)), json!(at(0)));
    let steps = json!([
        { "name": "Set up job", "conclusion": "success", "started_at": at(100), "completed_at": at(140) },
        { "name": "Flow, typecheck, lint and unit tests", "conclusion": "success", "started_at": at(100), "completed_at": at(650) },
    ]);
    let mut tests = job(501, "core-tests (1/3)", "success", &at(700), steps);
    (tests["created_at"], tests["started_at"]) = (json!(at(100)), json!(at(100)));
    github.extra.borrow_mut().push(("GET /repos/o/r/actions/runs/77".to_string(), MockReply::json(200, json!({ "id": 77, "name": workflow, "run_started_at": at(0), "created_at": at(0) }))));
    github.serve(&json!([plan, tests]), json!([]), created(), updated());
}

fn trend_body(github: &Github, sealed: &str, key: &str) -> String {
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_CI_SEALED_FILE", sealed), ("CI_DB_KEY", key), ("WF_VM_STATS_URL", "http://127.0.0.1:9"), ("WF_TAILNET_SOCKS", "")]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    last_json(&r)["body"].as_str().unwrap_or_default().to_string()
}

#[test]
fn the_comment_charts_the_last_twenty_successful_runs_of_a_workflow_and_sets_this_run_against_their_median() {
    // Given a sealed database with 22 successful backend_core runs (the two oldest very slow), a failed and a cancelled one, and a few of mobile_core and cli_core
    let dir = tempfile::tempdir().unwrap();
    let walls: Vec<f64> = (1..=22).map(|i| if i <= 2 { 3000.0 } else { 300.0 + 10.0 * (i - 2) as f64 }).collect();
    let mut runs: Vec<(String, &str, f64, f64, i64, &str)> = walls.iter().enumerate().map(|(i, w)| (format!("{}", 1000 + i), "backend_core+publish", *w, 10.0, 100 + i as i64, "success")).collect();
    runs.push(("2000".into(), "backend_core+publish", 9999.0, 10.0, 200, "failure"));
    runs.push(("2001".into(), "backend_core+publish", 8888.0, 10.0, 201, "cancelled"));
    runs.extend((0..3).map(|i| (format!("{}", 3000 + i), "mobile_core+publish", 600.0, 10.0, 300 + i, "success")));
    runs.push(("4000".into(), "cli_core+publish", 700.0, 10.0, 400, "success"));
    runs.extend((0..2).map(|i| (format!("{}", 5000 + i), "cli_build+publish", 500.0, 10.0, 500 + i, "success")));
    let sealed = sealed_history(dir.path(), &runs);
    let github = Github::start();
    trend_run(&github, "backend_core+publish");

    // When the comment is previewed
    let body = trend_body(&github, &sealed, DB_KEY);

    // Then the section has a line chart whose first line is the newest twenty walls in minutes, without the failed, the cancelled and the two oldest runs
    let expected: Vec<String> = walls[2..].iter().map(|w| format!("{:.1}", w / 60.0)).collect();
    assert!(body.contains("### Trend") && body.contains("xychart-beta"), "{body}");
    assert!(body.contains(&format!("    line [{}]\n", expected.join(", "))), "{body}");
    assert!(!body.contains("166.") && !body.contains("148.") && !body.contains("50.0"), "{body}");
    // And only this workflow is charted: the other workflows' histories (mobile_core has three runs, enough for a chart) are not in its comment
    assert_eq!(body.matches("title \"backend_core wall time and its longest jobs\"").count(), 1, "{body}");
    assert_eq!(body.matches("title \"mobile_core").count(), 0, "{body}");
    // And this run (11m 40s wall and critical path, no wait) stands against the median of those twenty (6m 45s wall, 10 s wait) with arrows
    assert!(body.contains("| backend_core | 11m 40s ↑ (median 6m 45s) | 11m 40s ↑ (median 6m 45s) | 0s ↓ (median 10s) |"), "{body}");
    // And the longest line names the critical-path job and its slowest step
    assert!(body.contains("**Longest:** critical path job `core-tests (1/3)` 10m 00s, its slowest step `Flow, typecheck, lint and unit tests` 9m 10s."), "{body}");
}

fn job_history_runs(count: usize, workflow: &'static str, id0: usize, jobs: &dyn Fn(usize) -> Vec<(&'static str, i64)>) -> Vec<JobRun> {
    (0..count).map(|i| (format!("{}", id0 + i), workflow, 600.0 + i as f64 * 60.0, (id0 + i) as i64, jobs(i))).collect()
}

#[test]
fn the_trend_chart_draws_the_total_and_each_long_job_as_its_own_coloured_line_with_a_legend_under_it() {
    // Given four successful runs of backend_core whose jobs are core-tests (the slowest), publish and plan, and a deploy that took 0 s (skipped)
    let dir = tempfile::tempdir().unwrap();
    let runs = job_history_runs(4, "backend_core+publish", 1000, &|i| vec![("plan", 60), ("publish", 120 + 60 * i as i64), ("core-tests", 300 + 60 * i as i64), ("deploy", 0)]);
    let sealed = sealed_jobs(dir.path(), &runs);
    let github = Github::start();
    trend_run(&github, "backend_core+publish");

    // When the comment is previewed
    let body = trend_body(&github, &sealed, DB_KEY);

    // Then one chart has the total first and then the jobs longest first, one line each, in minutes
    let lines = chart_lines(&body);
    assert_eq!(lines, ["10.0, 11.0, 12.0, 13.0", "5.0, 6.0, 7.0, 8.0", "2.0, 3.0, 4.0, 5.0", "1.0, 1.0, 1.0, 1.0"], "{body}");
    assert_eq!(body.matches("title \"backend_core wall time and its longest jobs\"").count(), 1, "{body}");
    // And the chart sets one colour per series in the same order, for the light and the dark theme alike (no theme is forced)
    assert!(body.contains(r##"%%{init: {"themeVariables": {"xyChart": {"plotColorPalette": "#388bfd, #f0883e, #3fb950, #a371f7, #d4a72c, #f85149, #a0724a"}}}}%%"##), "{body}");
    assert!(!body.contains("\"theme\""), "{body}");
    // And the line under the chart names each colour: the total first, then the jobs with their median; the skipped job has no line
    assert!(body.contains("🟦 total wall time · 🟧 `core-tests` (median 6m 30s) · 🟩 `publish` (median 3m 30s) · 🟪 `plan` (median 1m 00s)"), "{body}");
    assert!(!body.contains("deploy"), "{body}");
}

#[test]
fn a_matrix_job_is_one_line_at_its_slowest_shard() {
    // Given runs whose core-tests job is three shards of different length, and a flow-tests job that is a plain one
    let dir = tempfile::tempdir().unwrap();
    let runs = job_history_runs(3, "backend_core+publish", 1000, &|i| {
        vec![("core-tests (1/3)", 120), ("core-tests (2/3)", 300 + 60 * i as i64), ("core-tests (3/3)", 180), ("run / flow-tests", 200)]
    });
    let sealed = sealed_jobs(dir.path(), &runs);
    let github = Github::start();
    trend_run(&github, "backend_core+publish");

    // When the comment is previewed
    let body = trend_body(&github, &sealed, DB_KEY);

    // Then the shards are one series (the slowest shard of each run), named without the shard, next to the other job (its caller's prefix dropped)
    assert_eq!(chart_lines(&body)[1..], ["5.0, 6.0, 7.0", "3.3, 3.3, 3.3"], "{body}");
    assert!(body.contains("🟧 `core-tests` (median 6m 00s) · 🟩 `flow-tests` (median 3m 20s)"), "{body}");
    assert!(body.contains("A matrix job is one line at its slowest shard."), "{body}");
}

#[test]
fn only_the_six_longest_jobs_get_a_line_and_the_comment_says_how_many_were_left_out() {
    // Given runs of eight jobs of 1 to 8 minutes
    let dir = tempfile::tempdir().unwrap();
    let names = ["j1", "j2", "j3", "j4", "j5", "j6", "j7", "j8"];
    let runs = job_history_runs(3, "backend_core+publish", 1000, &|_| names.iter().enumerate().map(|(i, n)| (*n, 60 * (i as i64 + 1))).collect());
    let sealed = sealed_jobs(dir.path(), &runs);
    let github = Github::start();
    trend_run(&github, "backend_core+publish");

    // When the comment is previewed
    let body = trend_body(&github, &sealed, DB_KEY);

    // Then the total and six jobs are drawn, the longest first, and the two shortest are not
    let lines = chart_lines(&body);
    assert_eq!(lines.len(), 7, "{body}");
    assert_eq!(lines[1..].iter().map(|l| l.split(',').next().unwrap().to_string()).collect::<Vec<_>>(), ["8.0", "7.0", "6.0", "5.0", "4.0", "3.0"], "{body}");
    assert!(body.contains("🟫 `j3`") && !body.contains("`j2`") && !body.contains("`j1`"), "{body}");
    // And the note says two shorter jobs are left out
    assert!(body.contains("2 shorter jobs are left out."), "{body}");
}

#[test]
fn a_workflow_with_fewer_than_three_runs_has_no_chart_and_other_workflows_jobs_are_not_drawn() {
    // Given two successful runs of backend_core, and three of mobile_core with a job of its own
    let dir = tempfile::tempdir().unwrap();
    let mut runs = job_history_runs(2, "backend_core+publish", 1000, &|_| vec![("core-tests", 300)]);
    runs.extend(job_history_runs(3, "mobile_core+publish", 2000, &|_| vec![("mobile-tests", 900)]));
    let sealed = sealed_jobs(dir.path(), &runs);
    let github = Github::start();
    trend_run(&github, "backend_core+publish");

    // When the comment of a backend_core run is previewed
    let body = trend_body(&github, &sealed, DB_KEY);

    // Then there is no chart and the table of the two runs is still there, and no other workflow's job shows up
    assert!(!body.contains("xychart-beta\n    title \"backend_core") && !body.contains("mobile-tests"), "{body}");
    assert!(body.contains("### Trend") && body.contains("Median of the last 2 successful runs"), "{body}");
    // And with three runs of its own, the chart holds only that workflow's job
    let more = job_history_runs(3, "backend_core+publish", 3000, &|_| vec![("core-tests", 300)]);
    let again = tempfile::tempdir().unwrap();
    let sealed = sealed_jobs(again.path(), &more);
    let github = Github::start();
    trend_run(&github, "backend_core+publish");
    let body = trend_body(&github, &sealed, DB_KEY);
    assert!(body.contains("`core-tests`") && !body.contains("mobile-tests"), "{body}");
}

#[test]
fn a_database_that_does_not_open_leaves_the_trend_out_and_the_rest_of_the_comment_as_it_was() {
    // Given a sealed database with history, and a key that does not open it
    let dir = tempfile::tempdir().unwrap();
    let runs: Vec<(String, &str, f64, f64, i64, &str)> = (0..5).map(|i| (format!("{}", 1000 + i), "backend_core+publish", 400.0, 10.0, 100 + i, "success")).collect();
    let sealed = sealed_history(dir.path(), &runs);
    let github = Github::start();
    trend_run(&github, "backend_core+publish");

    // When the comment is previewed with the wrong key, and then for a run of a workflow that is not a hosted one
    let unreadable = trend_body(&github, &sealed, &"08".repeat(32));
    let not_hosted = {
        let other = Github::start();
        trend_run(&other, "CI core");
        trend_body(&other, &sealed, DB_KEY)
    };

    // Then the section is missing, nothing else says why, and the body is the one a run outside the hosted workflows gets
    assert!(!unreadable.contains("Trend") && unreadable.contains("### Jobs"), "{unreadable}");
    assert_eq!(unreadable, not_hosted);
}

/// A sealed database of five successful earlier runs of backend_core (wall 400 s, no wait; setup 10 s, install 10 s and tests 80 s per job, so 20% overhead; shard
/// core-tests-1 replayed 2 of 4 tasks, and 4 of 4 in the first three, so the median is the whole job), made the way the report job makes it.
fn sealed_actions(dir: &std::path::Path) -> String {
    let local = dir.join("actions-rows.sqlite");
    let conn = rusqlite::Connection::open(&local).unwrap();
    conn.execute_batch(
        "CREATE TABLE CI_RUN_PROFILE (RUN_ID TEXT NOT NULL PRIMARY KEY, WORKFLOW TEXT, WALL_S REAL, RUNNER_S REAL, PEAK_JOBS INTEGER, AVG_JOBS REAL, QUEUE_S_TOTAL REAL,
           QUEUE_S_MAX REAL, JOBS INTEGER, CRITICAL_PATH_JOB TEXT, CRITICAL_PATH_S REAL, PARALLEL_EFFICIENCY REAL);
         CREATE TABLE CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL, SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL,
           CONCLUSION TEXT, PRIMARY KEY (RUN_ID, JOB));
         CREATE TABLE CI_STEPS (RUN_ID TEXT NOT NULL, JOB TEXT NOT NULL, STEP_NO INTEGER NOT NULL, STEP TEXT, STARTED INTEGER, COMPLETED INTEGER, DURATION_S INTEGER, CONCLUSION TEXT,
           PRIMARY KEY (RUN_ID, JOB, STEP_NO));
         CREATE TABLE NX_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL, HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL,
           PASSED INTEGER NOT NULL, STARTED_MS INTEGER, ENDED_MS INTEGER, CPU_S REAL, PRIMARY KEY (RUN_ID, TASK, HASH));",
    )
    .unwrap();
    for i in 0..5 {
        let run = format!("{}", 1000 + i);
        conn.execute("INSERT INTO CI_RUN_PROFILE VALUES (?1, 'backend_core+publish', 400.0, 400.0, 1, 1.0, 0.0, 0.0, 1, 'core-tests', 400.0, 1.0)", [&run]).unwrap();
        conn.execute("INSERT INTO CI_JOBS VALUES (?1, 'abc', ?2, 'push', 'plan', 5, 0, 'success')", rusqlite::params![run, 100 + i]).unwrap();
        for (no, step, secs) in [(1, "Set up job", 10), (2, "Run ./.github/actions/install-test-deps", 10), (3, "Flow tests", 80)] {
            conn.execute("INSERT INTO CI_STEPS VALUES (?1, 'core-tests (1/3)', ?2, ?3, 0, 0, ?4, 'success')", rusqlite::params![run, no, step, secs]).unwrap();
        }
        for task in 0..4 {
            let outcome = if i < 3 || task < 2 { "hit" } else { "ran" };
            conn.execute("INSERT INTO NX_TASKS VALUES (?1, 'abc', 1, 'core-tests-1', ?2, ?2, ?3, 10, 1, 0, 0, 0.0)", rusqlite::params![run, format!("t{task}"), outcome]).unwrap();
        }
    }
    drop(conn);
    let sealed = dir.join("actions.sealed").display().to_string();
    let merged = run_wf(&["ci", "db", "merge", "--sealed-file", &sealed, &local.display().to_string()], &RunOpts::default().env("CI_DB_KEY", DB_KEY));
    assert_eq!(merged.exit_code, 0, "{}{}", merged.stdout, merged.stderr);
    sealed
}

/// Run 77 of backend_core+publish: plan for 100 s, then core-tests (1/3) from 100 s to 520 s with six steps (30 s setup, 100 s install, 60 s nx-cache, 200 s tests, 10 s and 20 s
/// of teardown), the sampler's file for it (CPU idle but for the first 110 s of the tests and the last 90 s of them idle again; 12% I/O wait; a 45% I/O pressure peak) and
/// its Nx ledger (four tasks: three replayed, one ran). Returns the directory holding both.
fn actions_run(github: &Github, with_steps: bool) -> std::path::PathBuf {
    let at = |s: i64| iso(BASE + s);
    let mut plan = job(500, "plan", "success", &at(100), json!([]));
    (plan["created_at"], plan["started_at"]) = (json!(at(0)), json!(at(0)));
    let timeline = [("Set up job", 100, 130), ("Run ./.github/actions/install-test-deps", 130, 230), ("Run ./.github/actions/nx-cache", 230, 290), ("Flow tests", 290, 490), ("Post Run ./.github/actions/nx-cache", 490, 500), ("Complete job", 500, 520)];
    let steps: Vec<Value> = if with_steps { timeline.iter().map(|(n, a, b)| json!({ "name": n, "conclusion": "success", "started_at": at(*a), "completed_at": at(*b) })).collect() } else { Vec::new() };
    let mut tests = job(501, "core-tests (1/3)", "success", &at(520), json!(steps));
    (tests["created_at"], tests["started_at"]) = (json!(at(100)), json!(at(100)));
    github.extra.borrow_mut().push(("GET /repos/o/r/actions/runs/77".to_string(), MockReply::json(200, json!({ "id": 77, "name": "backend_core+publish", "run_started_at": at(0), "created_at": at(0) }))));
    github.serve(&json!([plan, tests]), json!([]), created(), updated());

    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("wf-actions-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::remove_dir_all(&dir).ok();
    let (stats, ledgers) = (dir.join("runner-stats"), dir.join("ci-db/ci-db-core-tests-1"));
    std::fs::create_dir_all(&stats).unwrap();
    std::fs::create_dir_all(&ledgers).unwrap();
    let samples: Vec<String> = (1..=84)
        .map(|i| {
            let t = 100 + i * 5;
            let cpu = if (291..=400).contains(&t) { 95 } else if (401..=490).contains(&t) { 10 } else { 5 };
            let once = if i == 1 { r#","cores":4,"runner":"GitHub Actions 7""# } else { "" };
            format!(r#"{{"t":{},"cpu":{cpu},"memUsedMb":1000,"memTotalMb":16000,"load1":1.0,"iowaitPct":12,"psiCpu":0,"psiMem":0,"psiIo":{}{once}}}"#, BASE + t, if t == 300 { 45 } else { 0 })
        })
        .collect();
    std::fs::write(stats.join("stats-core-tests-1.jsonl"), samples.join("\n")).unwrap();
    let conn = rusqlite::Connection::open(ledgers.join("rows.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE NX_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL, HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL,
           PASSED INTEGER NOT NULL, STARTED_MS INTEGER, ENDED_MS INTEGER, CPU_S REAL, PRIMARY KEY (RUN_ID, TASK, HASH));",
    )
    .unwrap();
    for (task, outcome) in [("t0", "hit"), ("t1", "hit"), ("t2", "hit"), ("t3", "ran")] {
        conn.execute("INSERT INTO NX_TASKS VALUES ('77', 'abc', 1, 'core-tests-1', ?1, ?1, ?2, 10, 1, 0, 0, 0.0)", rusqlite::params![task, outcome]).unwrap();
    }
    dir
}

fn actions_body(github: &Github, dir: &std::path::Path, sealed: &str, key: &str) -> String {
    let (stats, ledgers) = (dir.join("runner-stats").display().to_string(), dir.join("ci-db").display().to_string());
    let r = github.comment_wf_with(
        &["--dry-run"],
        &[("WF_CI_SEALED_FILE", sealed), ("CI_DB_KEY", key), ("WF_VM_STATS_URL", "http://127.0.0.1:9"), ("WF_TAILNET_SOCKS", ""), ("WF_CI_RUNNER_STATS_DIR", &stats), ("WF_CI_LEDGERS", &ledgers)],
    );
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    last_json(&r)["body"].as_str().unwrap_or_default().to_string()
}

#[test]
fn the_comment_names_the_five_steps_with_the_most_idle_seconds_in_a_category_with_their_share_and_the_overhead_against_the_median() {
    // Given a finished run whose steps the sampler's file covers, and a sealed database of five earlier runs at 20% overhead
    let (db, github) = (tempfile::tempdir().unwrap(), Github::start());
    let sealed = sealed_actions(db.path());
    let dir = actions_run(&github, true);

    // When the comment is previewed
    let body = actions_body(&github, &dir, &sealed, DB_KEY);

    // Then the block lists the five steps with the most idle seconds, each with its job, idle time, share of the run's 310 idle seconds and category; ties keep step order
    let block = body.split("### Where the idle time is").nth(1).unwrap_or_default().split("\n### ").next().unwrap_or_default().to_string();
    let rows: Vec<&str> = block.lines().filter(|l| l.starts_with("| ") && !l.starts_with("| Step") && !l.starts_with("|---")).collect();
    assert_eq!(
        rows,
        [
            "| Run ./.github/actions/install-test-deps | core-tests (1/3) | 1m 40s | 32% | install |",
            "| Flow tests | core-tests (1/3) | 1m 30s | 29% | test wait |",
            "| Run ./.github/actions/nx-cache | core-tests (1/3) | 1m 00s | 19% | cache |",
            "| Set up job | core-tests (1/3) | 30s | 10% | setup |",
            "| Complete job | core-tests (1/3) | 20s | 6% | teardown |",
        ],
        "{body}"
    );
    // And the overhead is 160 of 420 step seconds against the 20% median of the five earlier runs, with the arrow
    assert!(block.contains("**Overhead** (setup, install, runner start and stop, teardown): 38% of all step seconds ↑ (median 20% of the last 5 successful runs)."), "{block}");
}

#[test]
fn every_amber_or_red_signal_has_one_action_line_from_the_fixed_table_and_a_green_one_has_none() {
    // Given the same run: wall 30% over the median (red), overhead 38% (amber), idle 74% (red), I/O wait 12% (amber), pressure 45% (amber), replay 25 points under the median (amber),
    // while the parallel efficiency is 1.00 and the queue wait 0 s (both green)
    let (db, github) = (tempfile::tempdir().unwrap(), Github::start());
    let sealed = sealed_actions(db.path());
    let dir = actions_run(&github, true);

    // When the comment is previewed
    let body = actions_body(&github, &dir, &sealed, DB_KEY);

    // Then "What to do" has exactly six lines, one per signal that is not green, each with its colour, value, thresholds and the action of the table
    let lines: Vec<&str> = body.split("### What to do").nth(1).unwrap_or_default().lines().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(lines.len(), 6, "{body}");
    let line = |label: &str| lines.iter().find(|l| l.contains(label)).copied().unwrap_or_else(|| panic!("no line for {label}: {body}"));
    assert!(line("Wall time").starts_with("- 🔴 **Wall time +30%** (red; amber over 10, red over 25 (% slower than the median)): Start with the longest job of the critical path"), "{lines:?}");
    assert!(line("Overhead share").starts_with("- 🟠 **Overhead share 38%** (amber; amber over 30, red over 45"), "{lines:?}");
    assert!(line("Overhead share").contains("Biggest part is install: cache the dependency install or merge shards that each install it."), "{lines:?}");
    assert!(line("Idle share").starts_with("- 🔴 **Idle share 74%** (red; amber over 35, red over 55"), "{lines:?}");
    assert!(line("Idle share").contains("Most idle is in install steps: cache the dependency install or merge shards that each install it."), "{lines:?}");
    assert!(line("I/O wait average").starts_with("- 🟠 **I/O wait average 12%**") && line("I/O wait average").contains("Worst: core-tests-1."), "{lines:?}");
    assert!(line("Pressure peak").starts_with("- 🟠 **Pressure peak 45%**"), "{lines:?}");
    assert!(line("Nx replay").starts_with("- 🟠 **Nx replay 25 points under**") && line("Nx replay").contains("skip the runner"), "{lines:?}");
    // And the two green signals say nothing
    assert!(!body.contains("Parallel efficiency") && !body.contains("Queue wait in total"), "{body}");
}

#[test]
fn a_run_with_no_step_rows_and_no_samples_has_a_none_idle_block_and_no_overhead_and_an_unreadable_database_drops_only_what_it_held() {
    // Given the finished run with no steps and no sampler file (the first run after the table existed), and the run with steps and samples
    let db = tempfile::tempdir().unwrap();
    let sealed = sealed_actions(db.path());
    let (bare, full) = (Github::start(), Github::start());
    let bare_dir = actions_run(&bare, false);
    std::fs::remove_dir_all(bare_dir.join("runner-stats")).unwrap();
    std::fs::create_dir_all(bare_dir.join("runner-stats")).unwrap();
    let full_dir = actions_run(&full, true);

    // When both comments are previewed, and the second again with a key that does not open the database
    let without = actions_body(&bare, &bare_dir, &sealed, DB_KEY);
    let unreadable = actions_body(&full, &full_dir, &sealed, &"08".repeat(32));

    // Then the first has its idle heading over a none sentence, no overhead line and no loading text
    assert!(without.contains("### Where the idle time is\n\n_none: no step has CPU samples over it_") && !without.contains("Overhead") && !without.contains("loading"), "{without}");
    // And with the database unreadable the idle block stays without its median, and the lines that need history (wall, replay) are left out while the idle one remains
    assert!(unreadable.contains("### Where the idle time is") && unreadable.contains("**Overhead** (setup, install, runner start and stop, teardown): 38% of all step seconds."), "{unreadable}");
    assert!(!unreadable.contains("(median") && !unreadable.contains("**Wall time") && !unreadable.contains("**Nx replay"), "{unreadable}");
    assert!(unreadable.contains("**Idle share 74%**"), "{unreadable}");
}

/// The `###` headings of a comment, in order. The title (`##`) names the verdict, so it differs by design; a blockquote (a failure notice) is not a section.
fn headings(body: &str) -> Vec<String> {
    body.lines().filter(|l| l.starts_with("### ")).map(str::to_string).collect()
}

/// What stands under `heading` up to the next heading.
fn under<'a>(body: &'a str, heading: &str) -> &'a str {
    let from = body.find(&format!("\n{heading}\n")).unwrap_or_else(|| panic!("no {heading} in {body}")) + heading.len() + 2;
    let rest = &body[from..];
    &rest[..rest.find("\n### ").unwrap_or(rest.len())]
}

const SKELETON: [&str; 10] = [
    "### ⏱️ Where the time went",
    "### Jobs",
    "### Fixed cost and the runner pool",
    "### Minutes per job",
    "### Trend",
    "### Where the idle time is",
    "### What to do",
    "### 🖥️ VM during the run",
    "### 🗄️ Cache",
    "### 🗄️ Cache health",
];

/// Run 77 of `workflow` with the sampler's files and a sealed history that `key` opens, as a comment preview: finished (`partial` false, the jobs of `actions_run`) or going
/// on (the plan job done, core-tests running, one more job queued). Returns the body.
fn skeleton_body(partial: bool, workflow: &str, key: &str) -> String {
    let (db, github) = (tempfile::tempdir().unwrap(), Github::start());
    let sealed = sealed_actions(db.path());
    let dir = actions_run(&github, true);
    github.extra.borrow_mut().push(("GET /repos/o/r/actions/runs/77".to_string(), MockReply::json(200, json!({ "id": 77, "name": workflow, "run_started_at": iso(BASE), "created_at": iso(BASE) }))));
    if partial {
        let at = |s: i64| iso(BASE + s);
        let mut plan = job(500, "plan", "success", &at(100), json!([]));
        (plan["created_at"], plan["started_at"]) = (json!(at(0)), json!(at(0)));
        let steps = json!([{ "name": "Set up job", "status": "completed", "conclusion": "success", "started_at": at(100), "completed_at": at(130) }, { "name": "Flow tests", "status": "in_progress", "conclusion": null, "started_at": at(130), "completed_at": null }]);
        let running = json!({ "id": 501, "name": "core-tests (1/3)", "status": "in_progress", "conclusion": null, "created_at": at(100), "started_at": at(100), "completed_at": null, "html_url": "https://github.com/o/r/actions/runs/77/job/501", "steps": steps });
        let queued = json!({ "id": 502, "name": "integration-tests (1/2)", "status": "queued", "conclusion": null, "created_at": at(100), "started_at": null, "completed_at": null, "html_url": "https://github.com/o/r/actions/runs/77/job/502", "steps": [] });
        github.serve(&json!([plan, running, queued]), json!([]), created(), updated());
    }
    actions_body(&github, &dir, &sealed, key)
}

#[test]
fn a_partial_comment_and_the_final_one_have_the_same_headings_in_the_same_order() {
    // Given the same run of a hosted workflow, once with a job running and one queued, and once finished
    let (partial, last) = (skeleton_body(true, "backend_core+publish", DB_KEY), skeleton_body(false, "backend_core+publish", DB_KEY));

    // When only the headings are read
    let (before, after) = (headings(&partial), headings(&last));

    // Then both are the one skeleton, in the same order
    assert_eq!(before, SKELETON, "{partial}");
    assert_eq!(after, SKELETON, "{last}");
}

#[test]
fn a_section_that_waits_for_data_says_so_in_one_line_and_the_data_replaces_it_where_it_stood() {
    // Given a comment while one job is done, one runs and one is queued
    let partial = skeleton_body(true, "backend_core+publish", DB_KEY);

    // When the sections that need the whole run are read
    // Then each is its heading over one fixed line saying what it waits for
    for (heading, waits) in [("### Minutes per job", "two finished jobs"), ("### What to do", "the run to finish"), ("### Where the idle time is", "the first step with CPU samples")] {
        assert_eq!(under(&partial, heading).trim(), format!("_loading: waits for {waits}_"), "{partial}");
    }
    // And the headline lines wait too, and the jobs table already has a row for the running job and one for the queued job, and the fixed cost shows what finished so far
    assert!(partial.contains("_loading: waits for the run to finish for its wall, runner time and parallel efficiency_"), "{partial}");
    assert!(partial.lines().any(|l| l.starts_with("| 🔄 2 |") && l.contains("core-tests (1/3)")) && partial.lines().any(|l| l.starts_with("| ⏸️ 3 |") && l.ends_with("⏸️ queued |")), "{partial}");
    assert!(under(&partial, "### Fixed cost and the runner pool").contains("| job plan | 1m 40s | 1 |"), "{partial}");
    // And the trend shows what is known (the history chart and the median) with this run's cells pending, its wall so far
    let trend = under(&partial, "### Trend");
    assert!(trend.contains("xychart-beta") && trend.contains("so far (median 6m 40s)") && trend.contains("pending (median 0s)"), "{trend}");
    assert!(trend.contains("**Longest:** _loading: waits for the run to finish_"), "{trend}");

    // When the run ends and the comment is made again
    let last = skeleton_body(false, "backend_core+publish", DB_KEY);

    // Then the same sections hold their data and the placeholders are gone
    assert!(under(&last, "### Minutes per job").contains("xychart-beta"), "{last}");
    assert!(under(&last, "### Fixed cost and the runner pool").contains("| Fixed cost |"), "{last}");
    assert!(under(&last, "### What to do").contains("**Idle share"), "{last}");
    assert!(under(&last, "### Trend").contains("| backend_core | 8m 40s") && under(&last, "### Trend").contains("**Longest:** critical path job"), "{last}");
    assert!(!last.contains("loading") && !last.contains("pending") && !last.contains("so far"), "{last}");
}

#[test]
fn a_section_that_does_not_apply_is_in_no_render() {
    // Given a run of a workflow that is not one of the hosted ones, on a labelled pipeline whose runners were not sampled into any directory
    let plain = |partial| {
        let (github, dir) = (Github::start(), tempfile::tempdir().unwrap());
        let _ = actions_run(&github, true);
        github.extra.borrow_mut().push(("GET /repos/o/r/actions/runs/77".to_string(), MockReply::json(200, json!({ "id": 77, "name": "ci", "run_started_at": iso(BASE), "created_at": iso(BASE) }))));
        let queued = json!({ "id": 502, "name": "integration-tests (1/2)", "status": "queued", "conclusion": null, "created_at": iso(BASE), "started_at": null, "completed_at": null, "html_url": "", "steps": [] });
        let jobs = if partial { json!([job(500, "plan", "success", &iso(BASE + 100), json!([])), queued]) } else { json!([job(500, "plan", "success", &iso(BASE + 100), json!([]))]) };
        github.serve(&jobs, json!([]), created(), updated());
        let sealed = sealed_actions(dir.path());
        let body = dry_body(&github, &[("WF_CI_SEALED_FILE", sealed.as_str()), ("CI_DB_KEY", DB_KEY), ("WF_CI_LABEL", "GitHub-hosted"), ("WF_VM_STATS_URL", "http://127.0.0.1:9")]);
        (body, partial)
    };

    // When it is rendered while it runs and when it is over, and a hosted run is rendered with a key that does not open its history
    let hosted = |partial| (skeleton_body(partial, "backend_core+publish", &"08".repeat(32)), partial);
    for (body, partial) in [plain(true), plain(false)] {
        // Then neither has a trend (not a hosted workflow), a runners section (no sampler directory) or a VM section (a labelled pipeline), and both keep the sections of every run
        let list = headings(&body);
        assert!(!list.iter().any(|h| h.contains("Trend") || h.contains("Runners during") || h.contains("VM during")), "{partial} {list:?}");
        assert!(list.contains(&"### Jobs".to_string()) && list.contains(&"### 🗄️ Cache".to_string()), "{partial} {list:?}");
    }
    // And the unreadable history leaves the trend out of the running render and the finished one alike, with every other section as it was
    let expected: Vec<&str> = SKELETON.iter().copied().filter(|h| *h != "### Trend").collect();
    for (body, partial) in [hosted(true), hosted(false)] {
        assert_eq!(headings(&body), expected, "{partial} {body}");
    }
}

#[test]
fn a_finished_run_never_says_loading_even_with_nothing_to_show_and_a_running_one_says_what_it_waits_for() {
    // Given a finished run with one quick job, no sampler file and no history, a failed run, and a finished hosted run
    let bare = {
        let github = audit_github_of(&[timed_job(500, "plan", "success", 20)], "ok\n", &[]);
        dry_body(&github, &[])
    };
    let red = {
        let github = Github::start();
        github.serve(&red_run(), json!([]), created(), updated());
        dry_body(&github, &[])
    };
    let hosted = skeleton_body(false, "backend_core+publish", DB_KEY);

    // When their text is read
    // Then none says loading, and a section with nothing to show says none in a fixed sentence under its heading
    for body in [&bare, &red, &hosted] {
        assert!(!body.contains("loading"), "{body}");
    }
    assert_eq!(under(&bare, "### Minutes per job").trim(), "_none: fewer than two jobs have a time to chart_", "{bare}");
    assert_eq!(under(&bare, "### 🗄️ Cache health").trim(), "_none: no job log held a cache event_", "{bare}");
    assert_eq!(under(&bare, "### What to do").trim(), "_none: no signal is amber or red_", "{bare}");
    // And the comment of a run that goes on says what each section waits for and has no none sentence there
    let partial = skeleton_body(true, "backend_core+publish", DB_KEY);
    assert!(under(&partial, "### What to do").contains("loading") && !under(&partial, "### What to do").contains("none"), "{partial}");
}
